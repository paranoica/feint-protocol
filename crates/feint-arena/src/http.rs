use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Arc, Mutex};

use axum::Router;
use axum::body::{Body, Bytes};
use axum::extract::{DefaultBodyLimit, State};
use axum::http::{HeaderValue, StatusCode, Uri, header};
use axum::response::{IntoResponse, Response};
use axum::routing::{get, post};
use feint_ai::decode_profile;
use rand_chacha::ChaCha8Rng;
use rand_chacha::rand_core::{RngCore, SeedableRng};
use tokio::sync::Semaphore;

use crate::store::{Store, StoreError, clean_nick};
use crate::verify::verify;

pub const MAX_BODY: usize = 32 << 20;
pub const VERIFY_SLOTS: usize = 2;

pub type Clock = Arc<dyn Fn() -> u64 + Send + Sync>;

pub struct Arena {
    store: Mutex<Store>,
    dist: PathBuf,
    gate: Semaphore,
    clock: Clock,
    rng: Mutex<ChaCha8Rng>,
    counter: AtomicU64,
}

impl Arena {
    #[must_use]
    pub fn new(store: Store, dist: PathBuf, clock: Clock, seed: u64) -> Arc<Self> {
        Arc::new(Self {
            store: Mutex::new(store),
            dist,
            gate: Semaphore::new(VERIFY_SLOTS),
            clock,
            rng: Mutex::new(ChaCha8Rng::seed_from_u64(seed)),
            counter: AtomicU64::new(0),
        })
    }
}

pub fn router(arena: Arc<Arena>) -> Router {
    Router::new()
        .route("/api/seed", get(seed))
        .route("/api/profile", get(profile))
        .route("/api/matches", post(upload))
        .fallback(get(file))
        .layer(DefaultBodyLimit::max(MAX_BODY))
        .with_state(arena)
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
                let hex = std::str::from_utf8(bytes.get(i + 1..i + 3)?).ok()?;
                out.push(u8::from_str_radix(hex, 16).ok()?);
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

async fn seed(State(arena): State<Arc<Arena>>) -> Response {
    let (token, seed) = {
        let Ok(mut rng) = arena.rng.lock() else {
            return error(StatusCode::INTERNAL_SERVER_ERROR, "rng");
        };
        let n = arena.counter.fetch_add(1, Ordering::Relaxed);
        let token = format!("{:016x}{:016x}{n:x}", rng.next_u64(), rng.next_u64());
        (token, rng.next_u32())
    };
    let now = (arena.clock)();
    let issued = arena
        .store
        .lock()
        .map_err(|_| StoreError::Db("poisoned".into()))
        .and_then(|mut store| store.issue_ticket(now, &token, seed));
    match issued {
        Ok(()) => reply(
            StatusCode::OK,
            format!("{{\"ticket\":\"{token}\",\"seed\":{seed}}}"),
        ),
        Err(StoreError::Busy) => error(StatusCode::TOO_MANY_REQUESTS, "too many open tickets"),
        Err(_) => error(StatusCode::INTERNAL_SERVER_ERROR, "store"),
    }
}

async fn profile(State(arena): State<Arc<Arena>>, uri: Uri) -> Response {
    let Some(nick) = query(&uri, "nick").and_then(|n| clean_nick(&n).ok()) else {
        return error(StatusCode::BAD_REQUEST, "nick");
    };
    let latest = arena
        .store
        .lock()
        .map_err(|_| StoreError::Db("poisoned".into()))
        .and_then(|store| store.latest_profile(&nick));
    match latest {
        Ok(Some((version, blob))) => {
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
        Ok(None) => StatusCode::NO_CONTENT.into_response(),
        Err(_) => error(StatusCode::INTERNAL_SERVER_ERROR, "store"),
    }
}

async fn upload(State(arena): State<Arc<Arena>>, uri: Uri, body: Bytes) -> Response {
    let nick = query(&uri, "nick").map(|n| clean_nick(&n));
    let Some(Ok(nick)) = nick else {
        return error(StatusCode::BAD_REQUEST, "nick");
    };
    let Some(ticket) = query(&uri, "ticket") else {
        return error(StatusCode::BAD_REQUEST, "ticket");
    };
    let used = match query(&uri, "profile").map(|v| v.parse::<i64>()) {
        None => None,
        Some(Ok(v)) => Some(v),
        Some(Err(_)) => return error(StatusCode::BAD_REQUEST, "profile"),
    };
    let Ok(_permit) = arena.gate.acquire().await else {
        return error(StatusCode::SERVICE_UNAVAILABLE, "closing");
    };
    let work = arena.clone();
    let done =
        tokio::task::spawn_blocking(move || accept(&work, &nick, &ticket, used, &body)).await;
    match done {
        Ok(Ok(saved)) => {
            let version = saved
                .profile_version
                .map_or_else(|| "null".to_owned(), |v| v.to_string());
            reply(
                StatusCode::CREATED,
                format!("{{\"id\":{},\"profile\":{version}}}", saved.id),
            )
        }
        Ok(Err(Refusal::Bad(what))) => error(StatusCode::BAD_REQUEST, what),
        Ok(Err(Refusal::Conflict)) => error(StatusCode::CONFLICT, "ticket"),
        Ok(Err(Refusal::Internal)) | Err(_) => error(StatusCode::INTERNAL_SERVER_ERROR, "store"),
    }
}

enum Refusal {
    Bad(&'static str),
    Conflict,
    Internal,
}

fn accept(
    arena: &Arena,
    nick: &str,
    ticket: &str,
    used: Option<i64>,
    body: &[u8],
) -> Result<crate::store::Saved, Refusal> {
    let now = (arena.clock)();
    let (seed, blob) = {
        let store = arena.store.lock().map_err(|_| Refusal::Internal)?;
        let seed = store
            .ticket_seed(now, ticket)
            .map_err(|_| Refusal::Conflict)?;
        let blob = match used {
            Some(v) => Some(
                store
                    .profile_blob(nick, v)
                    .map_err(|_| Refusal::Bad("profile"))?,
            ),
            None => None,
        };
        (seed, blob)
    };
    let profile = match &blob {
        Some(b) => Some(decode_profile(b).map_err(|_| Refusal::Internal)?),
        None => None,
    };
    let verified = verify(body, profile.as_ref()).map_err(|_| Refusal::Bad("record"))?;
    if verified.record.header.seed != seed {
        return Err(Refusal::Bad("seed"));
    }
    let next = verified.profile.as_ref().map(feint_ai::encode_profile);
    let mut store = arena.store.lock().map_err(|_| Refusal::Internal)?;
    store
        .save(&crate::store::Upload {
            now,
            nick,
            token: ticket,
            used,
            bytes: body,
            verified: &verified,
            profile: next,
        })
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
    let raw = uri.path();
    let decoded = decode_component(raw)?;
    let mut path = dist.to_path_buf();
    for segment in decoded.split('/').filter(|s| !s.is_empty()) {
        let ok = segment != ".."
            && segment != "."
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
