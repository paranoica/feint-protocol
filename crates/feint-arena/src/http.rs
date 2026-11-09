use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};

use axum::Router;
use axum::body::{Body, to_bytes};
use axum::extract::{DefaultBodyLimit, Request, State};
use axum::http::{HeaderMap, HeaderValue, StatusCode, Uri, header};
use axum::middleware::{self, Next};
use axum::response::{IntoResponse, Response};
use axum::routing::{get, post};
use feint_ai::decode_profile;
use rand_chacha::ChaCha8Rng;
use rand_chacha::rand_core::{RngCore, SeedableRng};
use tokio::sync::Semaphore;

use crate::store::{Saved, Store, StoreError, Upload, clean_nick, stage};
use crate::verify::verify;

pub const MAX_BODY: usize = 32 << 20;
pub const VERIFY_SLOTS: usize = 2;

pub type Clock = Arc<dyn Fn() -> u64 + Send + Sync>;

pub struct Arena {
    store: Mutex<Store>,
    matches: PathBuf,
    dist: PathBuf,
    gate: Semaphore,
    clock: Clock,
    rng: Mutex<ChaCha8Rng>,
}

impl Arena {
    #[must_use]
    pub fn new(store: Store, dist: PathBuf, clock: Clock, seed: [u8; 32]) -> Arc<Self> {
        Arc::new(Self {
            matches: store.matches_dir(),
            store: Mutex::new(store),
            dist,
            gate: Semaphore::new(VERIFY_SLOTS),
            clock,
            rng: Mutex::new(ChaCha8Rng::from_seed(seed)),
        })
    }

    fn with_store<T>(
        &self,
        f: impl FnOnce(&mut Store) -> Result<T, StoreError>,
    ) -> Result<T, StoreError> {
        let mut store = self
            .store
            .lock()
            .map_err(|_| StoreError::Db("poisoned".into()))?;
        f(&mut store)
    }
}

pub fn router(arena: Arc<Arena>) -> Router {
    Router::new()
        .route("/api/seed", post(seed))
        .route("/api/profile", get(profile))
        .route("/api/matches", post(upload))
        .fallback(get(file))
        .layer(DefaultBodyLimit::max(MAX_BODY))
        .layer(middleware::from_fn(local_only))
        .with_state(arena)
}

fn local_host(headers: &HeaderMap) -> bool {
    let Some(host) = headers.get(header::HOST).and_then(|h| h.to_str().ok()) else {
        return false;
    };
    let name = match host.strip_prefix('[') {
        Some(rest) => rest.split(']').next().unwrap_or_default(),
        None => host.split(':').next().unwrap_or_default(),
    };
    matches!(name, "127.0.0.1" | "localhost" | "::1")
}

async fn local_only(request: Request, next: Next) -> Response {
    if !local_host(request.headers()) {
        return error(StatusCode::FORBIDDEN, "host");
    }
    next.run(request).await
}

fn scripted(headers: &HeaderMap) -> bool {
    headers.get("x-feint").is_some_and(|v| v == "1")
}

fn reply(status: StatusCode, json: String) -> Response {
    let mut response = (status, json).into_response();
    response.headers_mut().insert(
        header::CONTENT_TYPE,
        HeaderValue::from_static("application/json"),
    );
    response
}

fn error(status: StatusCode, what: &str) -> Response {
    reply(status, format!("{{\"error\":\"{what}\"}}"))
}

fn decode_component(raw: &str) -> Option<String> {
    let bytes = raw.as_bytes();
    let mut out = Vec::with_capacity(bytes.len());
    let mut i = 0;
    while i < bytes.len() {
        match bytes[i] {
            b'%' => {
                let hex = bytes.get(i + 1..i + 3)?;
                if !hex.iter().all(u8::is_ascii_hexdigit) {
                    return None;
                }
                out.push(u8::from_str_radix(std::str::from_utf8(hex).ok()?, 16).ok()?);
                i += 3;
            }
            b'+' => {
                out.push(b' ');
                i += 1;
            }
            b => {
                out.push(b);
                i += 1;
            }
        }
    }
    String::from_utf8(out).ok()
}

fn query(uri: &Uri, key: &str) -> Option<String> {
    uri.query()?
        .split('&')
        .filter_map(|pair| pair.split_once('='))
        .find(|(k, _)| *k == key)
        .and_then(|(_, v)| decode_component(v))
}

async fn blocking<T: Send + 'static>(f: impl FnOnce() -> T + Send + 'static) -> Option<T> {
    tokio::task::spawn_blocking(f).await.ok()
}

async fn seed(State(arena): State<Arc<Arena>>, headers: HeaderMap) -> Response {
    if !scripted(&headers) {
        return error(StatusCode::FORBIDDEN, "header");
    }
    let work = arena.clone();
    let issued = blocking(move || {
        let (token, seed) = {
            let mut rng = work.rng.lock().map_err(|_| StoreError::Db("rng".into()))?;
            let token = format!("{:016x}{:016x}", rng.next_u64(), rng.next_u64());
            (token, rng.next_u32())
        };
        let now = (work.clock)();
        work.with_store(|store| store.issue_ticket(now, &token, seed))?;
        Ok::<_, StoreError>((token, seed))
    })
    .await;
    match issued {
        Some(Ok((token, seed))) => reply(
            StatusCode::OK,
            format!("{{\"ticket\":\"{token}\",\"seed\":{seed}}}"),
        ),
        Some(Err(StoreError::Busy)) => {
            error(StatusCode::TOO_MANY_REQUESTS, "too many open tickets")
        }
        _ => error(StatusCode::INTERNAL_SERVER_ERROR, "store"),
    }
}

async fn profile(State(arena): State<Arc<Arena>>, uri: Uri) -> Response {
    let Some(nick) = query(&uri, "nick").and_then(|n| clean_nick(&n).ok()) else {
        return error(StatusCode::BAD_REQUEST, "nick");
    };
    let work = arena.clone();
    match blocking(move || work.with_store(|store| store.latest_profile(&nick))).await {
        Some(Ok(Some((version, blob)))) => {
            let mut response = (StatusCode::OK, blob).into_response();
            let headers = response.headers_mut();
            headers.insert(
                header::CONTENT_TYPE,
                HeaderValue::from_static("application/octet-stream"),
            );
            if let Ok(v) = HeaderValue::from_str(&version.to_string()) {
                headers.insert("x-profile-version", v);
            }
            response
        }
        Some(Ok(None)) => StatusCode::NO_CONTENT.into_response(),
        _ => error(StatusCode::INTERNAL_SERVER_ERROR, "store"),
    }
}

enum Refusal {
    Bad(&'static str),
    Conflict,
    Internal,
}

impl Refusal {
    fn response(self) -> Response {
        match self {
            Self::Bad(what) => error(StatusCode::BAD_REQUEST, what),
            Self::Conflict => error(StatusCode::CONFLICT, "ticket"),
            Self::Internal => error(StatusCode::INTERNAL_SERVER_ERROR, "store"),
        }
    }
}

struct Claim {
    now: u64,
    seed: u64,
    nick: String,
    ticket: String,
    used: Option<i64>,
}

async fn upload(State(arena): State<Arc<Arena>>, request: Request) -> Response {
    let (parts, body) = request.into_parts();
    if !scripted(&parts.headers) {
        return error(StatusCode::FORBIDDEN, "header");
    }
    let Some(Ok(nick)) = query(&parts.uri, "nick").map(|n| clean_nick(&n)) else {
        return error(StatusCode::BAD_REQUEST, "nick");
    };
    let Some(ticket) = query(&parts.uri, "ticket") else {
        return error(StatusCode::BAD_REQUEST, "ticket");
    };
    let used = match query(&parts.uri, "profile").map(|v| v.parse::<i64>()) {
        None => None,
        Some(Ok(v)) => Some(v),
        Some(Err(_)) => return error(StatusCode::BAD_REQUEST, "profile"),
    };
    let length = parts
        .headers
        .get(header::CONTENT_LENGTH)
        .and_then(|v| v.to_str().ok())
        .and_then(|v| v.parse::<usize>().ok());
    match length {
        None => return error(StatusCode::LENGTH_REQUIRED, "length"),
        Some(n) if n > MAX_BODY => return error(StatusCode::PAYLOAD_TOO_LARGE, "size"),
        Some(_) => {}
    }
    let now = (arena.clock)();
    let work = arena.clone();
    let token = ticket.clone();
    let seed = match blocking(move || work.with_store(|store| store.try_ticket(now, &token))).await
    {
        Some(Ok(seed)) => seed,
        Some(Err(StoreError::Ticket)) => return Refusal::Conflict.response(),
        _ => return Refusal::Internal.response(),
    };
    let Ok(_permit) = arena.gate.try_acquire() else {
        return error(StatusCode::SERVICE_UNAVAILABLE, "busy");
    };
    let Ok(bytes) = to_bytes(body, MAX_BODY).await else {
        return error(StatusCode::PAYLOAD_TOO_LARGE, "size");
    };
    let claim = Claim {
        now,
        seed,
        nick,
        ticket,
        used,
    };
    let work = arena.clone();
    match blocking(move || accept(&work, &claim, &bytes)).await {
        Some(Ok(saved)) => {
            let version = saved
                .profile_version
                .map_or_else(|| "null".to_owned(), |v| v.to_string());
            reply(
                StatusCode::CREATED,
                format!("{{\"id\":{},\"profile\":{version}}}", saved.id),
            )
        }
        Some(Err(refusal)) => refusal.response(),
        None => Refusal::Internal.response(),
    }
}

fn accept(arena: &Arena, claim: &Claim, body: &[u8]) -> Result<Saved, Refusal> {
    let blob = match claim.used {
        Some(v) => Some(
            arena
                .with_store(|store| store.profile_blob(&claim.nick, v))
                .map_err(|_| Refusal::Bad("profile"))?,
        ),
        None => None,
    };
    let profile = match &blob {
        Some(b) => Some(decode_profile(b).map_err(|_| Refusal::Internal)?),
        None => None,
    };
    let verified = verify(body, profile.as_ref()).map_err(|_| Refusal::Bad("record"))?;
    if verified.record.header.seed != claim.seed {
        return Err(Refusal::Bad("seed"));
    }
    if !verified.finished {
        return Err(Refusal::Bad("unfinished"));
    }
    let upload = Upload {
        now: claim.now,
        nick: &claim.nick,
        token: &claim.ticket,
        used: claim.used,
        bytes: body,
        verified: &verified,
        profile: verified.profile.as_ref().map(feint_ai::encode_profile),
    };
    let staged = stage(&arena.matches, &upload).map_err(|_| Refusal::Internal)?;
    arena
        .with_store(|store| store.commit_staged(&upload, staged))
        .map_err(|e| match e {
            StoreError::Ticket => Refusal::Conflict,
            _ => Refusal::Internal,
        })
}

fn content_type(path: &Path) -> &'static str {
    match path.extension().and_then(|e| e.to_str()) {
        Some("html") => "text/html; charset=utf-8",
        Some("js") => "text/javascript",
        Some("css") => "text/css",
        Some("wasm") => "application/wasm",
        Some("json") => "application/json",
        Some("svg") => "image/svg+xml",
        _ => "application/octet-stream",
    }
}

fn safe_path(dist: &Path, uri: &Uri) -> Option<PathBuf> {
    let decoded = decode_component(uri.path())?;
    let mut path = dist.to_path_buf();
    for segment in decoded.split('/').filter(|s| !s.is_empty()) {
        let ok = !segment.starts_with('.')
            && segment
                .chars()
                .all(|c| c.is_ascii_alphanumeric() || matches!(c, '.' | '-' | '_'));
        if !ok {
            return None;
        }
        path.push(segment);
    }
    if decoded.ends_with('/') || path == dist {
        path.push("index.html");
    }
    Some(path)
}

async fn file(State(arena): State<Arc<Arena>>, uri: Uri) -> Response {
    let Some(path) = safe_path(&arena.dist, &uri) else {
        return StatusCode::NOT_FOUND.into_response();
    };
    match tokio::fs::read(&path).await {
        Ok(bytes) => {
            let mut response = Response::new(Body::from(bytes));
            response.headers_mut().insert(
                header::CONTENT_TYPE,
                HeaderValue::from_static(content_type(&path)),
            );
            response
        }
        Err(_) => StatusCode::NOT_FOUND.into_response(),
    }
}
