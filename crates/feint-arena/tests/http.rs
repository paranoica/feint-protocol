use std::io::{Read, Write};
use std::net::{SocketAddr, TcpStream};
use std::path::PathBuf;
use std::sync::Arc;
use std::sync::atomic::{AtomicU64, Ordering};
use std::time::{Duration, Instant};

use feint_ai::decode_profile;
use feint_arena::{Arena, Store, router};
use feint_web::Session;

struct Server {
    addr: SocketAddr,
    data: PathBuf,
    clock: Arc<AtomicU64>,
    _rt: tokio::runtime::Runtime,
}

fn start(name: &str) -> Server {
    start_with(name, 4)
}

fn start_with(name: &str, workers: usize) -> Server {
    let root = std::env::temp_dir().join(format!("feint-http-{}-{name}", std::process::id()));
    let _ = std::fs::remove_dir_all(&root);
    let dist = root.join("dist");
    std::fs::create_dir_all(dist.join("assets")).unwrap();
    std::fs::write(dist.join("index.html"), "<p>feint</p>").unwrap();
    std::fs::write(dist.join("assets/app.wasm"), [0u8, 97, 115, 109]).unwrap();
    std::fs::write(root.join("secret.txt"), "no").unwrap();
    let data = root.join("data");
    let store = Store::open(&data).unwrap();
    let clock = Arc::new(AtomicU64::new(1_000_000));
    let ticking = clock.clone();
    let arena = Arena::new(
        store,
        dist,
        Arc::new(move || ticking.load(Ordering::SeqCst)),
        7,
    );
    let rt = tokio::runtime::Builder::new_multi_thread()
        .worker_threads(workers)
        .enable_all()
        .build()
        .unwrap();
    let listener = rt
        .block_on(tokio::net::TcpListener::bind("127.0.0.1:0"))
        .unwrap();
    let addr = listener.local_addr().unwrap();
    rt.spawn(async move { axum::serve(listener, router(arena)).await });
    Server {
        addr,
        data,
        clock,
        _rt: rt,
    }
}

struct Reply {
    status: u16,
    head: String,
    body: Vec<u8>,
}

fn call(addr: SocketAddr, method: &str, path: &str, body: &[u8]) -> Reply {
    let mut s = TcpStream::connect(addr).unwrap();
    let head = format!(
        "{method} {path} HTTP/1.1\r\nHost: x\r\nConnection: close\r\nContent-Length: {}\r\n\r\n",
        body.len()
    );
    s.write_all(head.as_bytes()).unwrap();
    let _ = s.write_all(body);
    let mut raw = Vec::new();
    let _ = s.read_to_end(&mut raw);
    let split = raw.windows(4).position(|w| w == b"\r\n\r\n").unwrap();
    let head = String::from_utf8_lossy(&raw[..split]).to_lowercase();
    let status = head[9..12].parse().unwrap();
    Reply {
        status,
        head,
        body: raw[split + 4..].to_vec(),
    }
}

fn json_field(body: &[u8], key: &str) -> String {
    let text = String::from_utf8_lossy(body);
    let at = text.find(&format!("\"{key}\":")).unwrap() + key.len() + 3;
    text[at..]
        .trim_start_matches('"')
        .split(['"', ',', '}'])
        .next()
        .unwrap()
        .to_owned()
}

fn ticket(s: &Server) -> (String, u64) {
    let r = call(s.addr, "GET", "/api/seed", &[]);
    assert_eq!(r.status, 200);
    (
        json_field(&r.body, "ticket"),
        json_field(&r.body, "seed").parse().unwrap(),
    )
}

fn finished(level: u8, seed: u64, profile: Option<&feint_ai::Profile>) -> Session {
    let mut s = Session::with_profile(level, seed, profile).unwrap();
    while !s.finished() && s.tick() < 240 * 60 * 15 {
        let p = s.positions();
        s.set_pointer(p[0], p[1]);
        s.advance(4);
    }
    assert!(s.finished());
    s
}

fn count(s: &Server, sql: &str) -> i64 {
    let conn = rusqlite::Connection::open(s.data.join("feint.db")).unwrap();
    conn.query_row(sql, [], |r| r.get(0)).unwrap()
}

#[test]
fn static_files_are_served_and_nothing_outside_dist() {
    let s = start("static");
    let index = call(s.addr, "GET", "/", &[]);
    assert_eq!(
        (index.status, index.body.as_slice()),
        (200, b"<p>feint</p>".as_slice())
    );
    assert!(index.head.contains("content-type: text/html"));
    let wasm = call(s.addr, "GET", "/assets/app.wasm", &[]);
    assert_eq!(wasm.status, 200);
    assert!(wasm.head.contains("content-type: application/wasm"));
    for bad in [
        "/../secret.txt",
        "/%2e%2e/secret.txt",
        "/assets/..%2f..%2fsecret.txt",
        "/assets/%2E%2E/%2E%2E/secret.txt",
        "/missing.js",
    ] {
        let r = call(s.addr, "GET", bad, &[]);
        assert!(r.status == 404 || r.status == 400, "{bad}: {}", r.status);
        assert_ne!(r.body, b"no", "{bad} leaked a file outside dist");
    }
}

#[test]
fn a_genuine_upload_is_stored_once() {
    let s = start("upload");
    let (t, seed) = ticket(&s);
    let game = finished(1, seed, None);
    let path = format!("/api/matches?nick=ann%20b&ticket={t}");
    let r = call(s.addr, "POST", &path, &game.record());
    assert_eq!(r.status, 201, "{}", String::from_utf8_lossy(&r.body));
    let id: i64 = json_field(&r.body, "id").parse().unwrap();
    assert_eq!(
        json_field(&r.body, "profile"),
        "null",
        "level 1 keeps no profile"
    );
    assert_eq!(count(&s, "SELECT COUNT(*) FROM matches"), 1);
    assert_eq!(
        count(&s, "SELECT COUNT(*) FROM players WHERE nick = 'ann b'"),
        1
    );
    assert!(s.data.join(format!("matches/{id}.rec")).exists());
    assert!(s.data.join(format!("matches/{id}.parquet")).exists());
    assert_eq!(
        std::fs::read(s.data.join(format!("matches/{id}.rec"))).unwrap(),
        game.record()
    );
    assert_eq!(
        call(s.addr, "POST", &path, &game.record()).status,
        409,
        "a ticket is spent once"
    );
    assert_eq!(count(&s, "SELECT COUNT(*) FROM matches"), 1);
}

#[test]
fn bad_uploads_are_refused_without_a_trace() {
    let s = start("refuse");
    let (t, seed) = ticket(&s);
    let game = finished(1, seed, None);
    let mut forged = feint_ai::decode_record(&game.record()).unwrap();
    forged.score = vec![7, 0];
    let forged = feint_ai::encode_record(&forged);
    let post = |q: &str, b: &[u8]| call(s.addr, "POST", &format!("/api/matches?{q}"), b).status;
    assert_eq!(post(&format!("nick=ann&ticket={t}"), &forged), 400);
    assert_eq!(post("nick=ann&ticket=nope", &game.record()), 409);
    assert_eq!(
        post(&format!("nick=%01ann&ticket={t}"), &game.record()),
        400
    );
    assert_eq!(
        post(
            &format!("nick={}&ticket={t}", "a".repeat(25)),
            &game.record()
        ),
        400
    );
    assert_eq!(post(&format!("ticket={t}"), &game.record()), 400);
    let (other, _) = ticket(&s);
    assert_eq!(
        post(&format!("nick=ann&ticket={other}"), &game.record()),
        400,
        "another ticket's seed"
    );
    assert_eq!(count(&s, "SELECT COUNT(*) FROM matches"), 0);
    s.clock.fetch_add(46 * 60, Ordering::SeqCst);
    assert_eq!(
        post(&format!("nick=ann&ticket={t}"), &game.record()),
        409,
        "an expired ticket"
    );
}

#[test]
fn body_and_ticket_limits_hold() {
    let s = start("limits");
    let (t, _) = ticket(&s);
    let over = vec![0u8; feint_arena::MAX_BODY + 1];
    assert_eq!(
        call(
            s.addr,
            "POST",
            &format!("/api/matches?nick=a&ticket={t}"),
            &over
        )
        .status,
        413
    );
    let under = vec![0u8; feint_arena::MAX_BODY - 1024];
    assert_eq!(
        call(
            s.addr,
            "POST",
            &format!("/api/matches?nick=a&ticket={t}"),
            &under
        )
        .status,
        400,
        "within the cap, refused as a record"
    );
    for _ in 0..511 {
        ticket(&s);
    }
    assert_eq!(call(s.addr, "GET", "/api/seed", &[]).status, 429);
    s.clock.fetch_add(46 * 60, Ordering::SeqCst);
    assert_eq!(
        call(s.addr, "GET", "/api/seed", &[]).status,
        200,
        "expired tickets free their slots"
    );
}

#[test]
fn level_five_profiles_advance_only_from_the_latest_version() {
    let s = start("profiles");
    assert_eq!(
        call(s.addr, "GET", "/api/profile?nick=ann", &[]).status,
        204
    );
    let post = |q: String, b: &[u8]| call(s.addr, "POST", &format!("/api/matches?{q}"), b);

    let (t, seed) = ticket(&s);
    let r = post(
        format!("nick=ann&ticket={t}"),
        &finished(5, seed, None).record(),
    );
    assert_eq!(
        (r.status, json_field(&r.body, "profile")),
        (201, "1".to_owned())
    );
    let got = call(s.addr, "GET", "/api/profile?nick=ann", &[]);
    assert_eq!(got.status, 200);
    assert!(got.head.contains("x-profile-version: 1"));
    let v1 = decode_profile(&got.body).unwrap();

    let (t, seed) = ticket(&s);
    let r = post(
        format!("nick=ann&ticket={t}&profile=1"),
        &finished(5, seed, Some(&v1)).record(),
    );
    assert_eq!(
        (r.status, json_field(&r.body, "profile")),
        (201, "2".to_owned())
    );

    let (t, seed) = ticket(&s);
    let r = post(
        format!("nick=ann&ticket={t}&profile=1"),
        &finished(5, seed, Some(&v1)).record(),
    );
    assert_eq!(
        (r.status, json_field(&r.body, "profile")),
        (201, "null".to_owned()),
        "stale parent"
    );

    let (t, seed) = ticket(&s);
    let r = post(
        format!("nick=ann&ticket={t}"),
        &finished(5, seed, None).record(),
    );
    assert_eq!(
        (r.status, json_field(&r.body, "profile")),
        (201, "null".to_owned()),
        "no parent named while one exists"
    );

    let (t, seed) = ticket(&s);
    assert_eq!(
        post(
            format!("nick=bob&ticket={t}&profile=1"),
            &finished(5, seed, Some(&v1)).record()
        )
        .status,
        400,
        "another nick's version"
    );
    let (t, seed) = ticket(&s);
    assert_eq!(
        post(
            format!("nick=ann&ticket={t}&profile=1"),
            &finished(3, seed, None).record()
        )
        .status,
        400,
        "a profile below level 5"
    );
    assert_eq!(count(&s, "SELECT COUNT(*) FROM profiles"), 2);
}

#[test]
fn a_failed_write_leaves_no_row_and_keeps_the_ticket() {
    use std::os::unix::fs::PermissionsExt;
    let s = start("atomic");
    let (t, seed) = ticket(&s);
    let game = finished(1, seed, None);
    let matches = s.data.join("matches");
    std::fs::set_permissions(&matches, std::fs::Permissions::from_mode(0o500)).unwrap();
    let r = call(
        s.addr,
        "POST",
        &format!("/api/matches?nick=ann&ticket={t}"),
        &game.record(),
    );
    std::fs::set_permissions(&matches, std::fs::Permissions::from_mode(0o755)).unwrap();
    assert_eq!(r.status, 500);
    assert_eq!(count(&s, "SELECT COUNT(*) FROM matches"), 0);
    assert_eq!(
        std::fs::read_dir(&matches).unwrap().count(),
        0,
        "no orphan files"
    );
    let again = call(
        s.addr,
        "POST",
        &format!("/api/matches?nick=ann&ticket={t}"),
        &game.record(),
    );
    assert_eq!(again.status, 201, "the ticket survived the failure");
}

#[test]
fn a_slow_verification_does_not_stall_static_files() {
    let s = start_with("slow", 1);
    let (t, seed) = ticket(&s);
    let mut game = Session::new(5, seed).unwrap();
    for _ in 0..240 * 60 * 4 / 8 {
        if game.finished() {
            break;
        }
        game.set_pointer(-0.6, if game.tick() % 960 < 480 { 0.3 } else { -0.3 });
        game.advance(8);
    }
    let bytes = game.record();
    let addr = s.addr;
    let upload = std::thread::spawn(move || {
        let began = Instant::now();
        let r = call(
            addr,
            "POST",
            &format!("/api/matches?nick=ann&ticket={t}"),
            &bytes,
        );
        (r.status, began.elapsed())
    });
    std::thread::sleep(Duration::from_millis(50));
    let began = Instant::now();
    assert_eq!(call(s.addr, "GET", "/", &[]).status, 200);
    let page = began.elapsed();
    let (status, verifying) = upload.join().unwrap();
    assert_eq!(status, 201);
    assert!(
        page < verifying / 2,
        "a page took {page:?} while a verification took {verifying:?}"
    );
}

#[test]
fn two_uploads_racing_on_one_ticket_store_one_match() {
    let s = start("race");
    let (t, seed) = ticket(&s);
    let bytes = finished(1, seed, None).record();
    let addr = s.addr;
    let racers: Vec<_> = (0..2)
        .map(|i| {
            let (b, t) = (bytes.clone(), t.clone());
            std::thread::spawn(move || {
                call(
                    addr,
                    "POST",
                    &format!("/api/matches?nick=r{i}&ticket={t}"),
                    &b,
                )
                .status
            })
        })
        .collect();
    let mut statuses: Vec<u16> = racers.into_iter().map(|h| h.join().unwrap()).collect();
    statuses.sort_unstable();
    assert_eq!(statuses, vec![201, 409]);
    assert_eq!(count(&s, "SELECT COUNT(*) FROM matches"), 1);
    let left: Vec<String> = std::fs::read_dir(s.data.join("matches"))
        .unwrap()
        .map(|e| e.unwrap().file_name().to_string_lossy().into_owned())
        .collect();
    assert_eq!(left.len(), 2, "only the winner's files: {left:?}");
    assert!(left.iter().all(|f| !f.ends_with(".tmp")), "{left:?}");
}
