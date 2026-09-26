//! Pawn demo: a tiny turn-based web server.
//!
//! * Binds to a random port above 1000 and prints the URL on startup.
//! * Serves a single page application (embedded at compile time).
//! * Keeps all user data in an in-memory store keyed by a 64-bit session
//!   cookie drawn from the operating system's CSPRNG.
//! * The client talks to the server with plain AJAX (`fetch`) calls.

use std::collections::HashMap;
use std::net::{IpAddr, Ipv4Addr, SocketAddr};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use axum::extract::State;
use axum::http::{HeaderMap, HeaderValue, StatusCode, header};
use axum::response::{Html, IntoResponse, Response};
use axum::routing::{get, post};
use axum::{Json, Router};
use serde::{Deserialize, Serialize};
use tokio::net::TcpListener;

const BOARD_SIZE: u8 = 5;
const COOKIE_NAME: &str = "pawn_sid";
const SESSION_IDLE_LIMIT: Duration = Duration::from_secs(60 * 60);
const SWEEP_INTERVAL: Duration = Duration::from_secs(5 * 60);
const MIN_PORT: u16 = 1001;

const INDEX_HTML: &str = include_str!("../static/index.html");

/// Everything the server knows about one visitor.
#[derive(Debug, Clone)]
struct UserData {
    x: u8,
    y: u8,
    moves: u32,
    last_seen: Instant,
}

impl UserData {
    fn new() -> Self {
        Self {
            x: BOARD_SIZE / 2,
            y: BOARD_SIZE - 1,
            moves: 0,
            last_seen: Instant::now(),
        }
    }

    fn view(&self) -> StateView {
        StateView {
            x: self.x,
            y: self.y,
            moves: self.moves,
            size: BOARD_SIZE,
        }
    }
}

/// The in-memory database: session id -> that user's data.
type Store = Arc<Mutex<HashMap<u64, UserData>>>;

#[derive(Serialize)]
struct StateView {
    x: u8,
    y: u8,
    moves: u32,
    size: u8,
}

#[derive(Serialize)]
struct ErrorView {
    error: &'static str,
    state: StateView,
}

#[derive(Deserialize)]
struct MoveRequest {
    x: u8,
    y: u8,
}

/// 64 bits from the OS cryptographic random number generator.
fn secure_u64() -> u64 {
    getrandom::u64().expect("OS random number generator unavailable")
}

fn session_from_headers(headers: &HeaderMap) -> Option<u64> {
    headers
        .get_all(header::COOKIE)
        .iter()
        .filter_map(|v| v.to_str().ok())
        .flat_map(|v| v.split(';'))
        .filter_map(|pair| pair.trim().split_once('='))
        .find(|(name, _)| *name == COOKIE_NAME)
        .and_then(|(_, value)| {
            (value.len() == 16)
                .then(|| u64::from_str_radix(value, 16).ok())
                .flatten()
        })
}

fn session_cookie(id: u64) -> HeaderValue {
    HeaderValue::from_str(&format!(
        "{COOKIE_NAME}={id:016x}; Path=/; HttpOnly; SameSite=Strict"
    ))
    .expect("cookie is valid ASCII")
}

/// Runs `f` against the caller's user data, creating a new session (and a
/// `Set-Cookie` header) when the request carries no known session id.
fn with_user<T>(
    store: &Store,
    headers: &HeaderMap,
    f: impl FnOnce(&mut UserData) -> T,
) -> (T, Option<HeaderValue>) {
    let mut map = store.lock().expect("store lock poisoned");
    let existing = session_from_headers(headers).filter(|id| map.contains_key(id));
    let (id, new_cookie) = match existing {
        Some(id) => (id, None),
        None => {
            let id = loop {
                let candidate = secure_u64();
                if !map.contains_key(&candidate) {
                    break candidate;
                }
            };
            map.insert(id, UserData::new());
            (id, Some(session_cookie(id)))
        }
    };
    let user = map.get_mut(&id).expect("session just ensured");
    user.last_seen = Instant::now();
    (f(user), new_cookie)
}

fn respond(status: StatusCode, body: impl Serialize, cookie: Option<HeaderValue>) -> Response {
    let mut response = (status, Json(body)).into_response();
    let headers = response.headers_mut();
    headers.insert(header::CACHE_CONTROL, HeaderValue::from_static("no-store"));
    if let Some(cookie) = cookie {
        headers.insert(header::SET_COOKIE, cookie);
    }
    response
}

async fn index() -> impl IntoResponse {
    Html(INDEX_HTML)
}

async fn get_state(State(store): State<Store>, headers: HeaderMap) -> Response {
    let (view, cookie) = with_user(&store, &headers, |u| u.view());
    respond(StatusCode::OK, view, cookie)
}

async fn post_move(
    State(store): State<Store>,
    headers: HeaderMap,
    Json(req): Json<MoveRequest>,
) -> Response {
    let (result, cookie) = with_user(&store, &headers, |u| {
        let outcome = try_move(u, req.x, req.y);
        (outcome, u.view())
    });
    match result {
        (Ok(()), view) => respond(StatusCode::OK, view, cookie),
        (Err(error), state) => respond(
            StatusCode::UNPROCESSABLE_ENTITY,
            ErrorView { error, state },
            cookie,
        ),
    }
}

/// Moves the pawn one square in any direction (orthogonal or diagonal).
fn try_move(u: &mut UserData, x: u8, y: u8) -> Result<(), &'static str> {
    if x >= BOARD_SIZE || y >= BOARD_SIZE {
        return Err("target square is off the board");
    }
    let (dx, dy) = (u.x.abs_diff(x), u.y.abs_diff(y));
    if dx == 0 && dy == 0 {
        return Err("pawn is already on that square");
    }
    if dx > 1 || dy > 1 {
        return Err("pawn may only move to an adjacent square");
    }
    u.x = x;
    u.y = y;
    u.moves += 1;
    Ok(())
}

async fn post_reset(State(store): State<Store>, headers: HeaderMap) -> Response {
    let (view, cookie) = with_user(&store, &headers, |u| {
        *u = UserData::new();
        u.view()
    });
    respond(StatusCode::OK, view, cookie)
}

/// Drops sessions that have been idle for longer than `SESSION_IDLE_LIMIT`.
async fn sweep_idle_sessions(store: Store) {
    let mut interval = tokio::time::interval(SWEEP_INTERVAL);
    loop {
        interval.tick().await;
        let mut map = store.lock().expect("store lock poisoned");
        map.retain(|_, u| u.last_seen.elapsed() < SESSION_IDLE_LIMIT);
    }
}

/// Binds to a random port in `MIN_PORT..=65535`, retrying on collisions.
async fn bind_random_port(ip: IpAddr) -> std::io::Result<TcpListener> {
    let span = u32::from(u16::MAX - MIN_PORT) + 1;
    let mut last_err = None;
    for _ in 0..64 {
        let port = MIN_PORT + (getrandom::u32().expect("OS RNG unavailable") % span) as u16;
        match TcpListener::bind(SocketAddr::new(ip, port)).await {
            Ok(listener) => return Ok(listener),
            Err(e) => last_err = Some(e),
        }
    }
    Err(last_err.expect("at least one bind attempt"))
}

#[tokio::main]
async fn main() -> std::io::Result<()> {
    // Local-only by default; set PAWN_BIND=0.0.0.0 to expose on the network.
    let ip: IpAddr = std::env::var("PAWN_BIND")
        .ok()
        .and_then(|s| s.parse().ok())
        .unwrap_or(IpAddr::V4(Ipv4Addr::LOCALHOST));

    let store: Store = Arc::new(Mutex::new(HashMap::new()));
    tokio::spawn(sweep_idle_sessions(store.clone()));

    let app = Router::new()
        .route("/", get(index))
        .route("/api/state", get(get_state))
        .route("/api/move", post(post_move))
        .route("/api/reset", post(post_reset))
        .with_state(store);

    let listener = bind_random_port(ip).await?;
    let addr = listener.local_addr()?;
    let shown_host = if addr.ip().is_unspecified() {
        "127.0.0.1".to_string()
    } else {
        addr.ip().to_string()
    };
    println!("Pawn demo listening on port {}", addr.port());
    println!(
        "Open http://{shown_host}:{}/ in your browser (Ctrl+C to stop)",
        addr.port()
    );

    axum::serve(listener, app)
        .with_graceful_shutdown(async {
            let _ = tokio::signal::ctrl_c().await;
        })
        .await
}
