use std::net::{IpAddr, SocketAddr};
use std::path::PathBuf;
use std::sync::{Arc, Mutex};

use axum::extract::{Path, Request, State};
use axum::http::{header, HeaderName, StatusCode};
use axum::middleware::Next;
use axum::response::{IntoResponse, Response};
use axum::routing::{get, post};
use axum::{Json, Router};
use play_core::{Play, RunningGame};
use serde::Serialize;
use tower::util::ServiceExt;
use tower_http::services::ServeFile;

pub mod emulator;
pub mod guard;
pub mod net;
pub mod views;

pub use net::{reachable, Reach, Reachable};

pub const DEFAULT_PORT: u16 = 5555;

pub type SharedPlay = Arc<Mutex<Play>>;

const INDEX_HTML: &[u8] = include_bytes!("../web/index.html");
const APP_JS: &[u8] = include_bytes!("../web/app.js");
const PHONE_HTML: &[u8] = include_bytes!("../web/phone.html");
const PHONE_JS: &[u8] = include_bytes!("../web/phone.js");
const STYLE_CSS: &[u8] = include_bytes!("../web/style.css");
const SERVICE_WORKER: &[u8] = include_bytes!("../web/sw.js");
const MANIFEST: &[u8] = include_bytes!("../web/manifest.webmanifest");
const ICON_192: &[u8] = include_bytes!("../web/icon-192.png");
const ICON_512: &[u8] = include_bytes!("../web/icon-512.png");
const ICON_192_MASKABLE: &[u8] = include_bytes!("../web/icon-192-maskable.png");
const ICON_512_MASKABLE: &[u8] = include_bytes!("../web/icon-512-maskable.png");

/// The page and its code are the binary's, so an old copy is a stale app:
/// fetch them every time. It is a few kilobytes over a network the machine
/// is already on. The mark does not change, so it is worth keeping.
const REVALIDATE: &str = "no-cache";
const KEEP_A_WHILE: &str = "public, max-age=86400";

// The remote is open to the network it is on, the same trust the family's
// media server extends: it can read the shelf and start or stop games that
// are already on it, nothing else. PLAY_WEB_PORT=0 turns it off.
pub fn addr_from_env() -> Option<SocketAddr> {
    let port = match std::env::var("PLAY_WEB_PORT") {
        Ok(raw) => match raw.trim().parse::<u16>() {
            Ok(0) => return None,
            Ok(p) => p,
            Err(_) => {
                log::warn!("PLAY_WEB_PORT is not a port number; using {DEFAULT_PORT}");
                DEFAULT_PORT
            }
        },
        Err(_) => DEFAULT_PORT,
    };
    let bind = match std::env::var("PLAY_WEB_BIND") {
        Ok(raw) => match raw.trim().parse::<IpAddr>() {
            Ok(ip) => ip,
            Err(_) => {
                log::warn!("PLAY_WEB_BIND is not an address; using 0.0.0.0");
                IpAddr::from([0, 0, 0, 0])
            }
        },
        Err(_) => IpAddr::from([0, 0, 0, 0]),
    };
    Some(SocketAddr::new(bind, port))
}

fn asset(body: &'static [u8], mime: &'static str, cache: &'static str) -> Response {
    (
        [(header::CONTENT_TYPE, mime), (header::CACHE_CONTROL, cache)],
        body,
    )
        .into_response()
}

fn page(body: &'static [u8], mime: &'static str) -> Response {
    asset(body, mime, REVALIDATE)
}

fn image(body: &'static [u8], mime: &'static str) -> Response {
    asset(body, mime, KEEP_A_WHILE)
}

#[derive(Clone)]
struct Shelf {
    play: SharedPlay,
    /// Where the phone's emulator is kept once it has been downloaded.
    emulator: PathBuf,
}

pub fn router(play: SharedPlay) -> Router {
    let emulator = play
        .lock()
        .map(|play| play.library.join("emulator"))
        .unwrap_or_else(|poisoned| poisoned.into_inner().library.join("emulator"));
    let shelf = Shelf { play, emulator };
    Router::new()
        .route(
            "/",
            get(|| async { page(INDEX_HTML, "text/html; charset=utf-8") }),
        )
        .route(
            "/app.js",
            get(|| async { page(APP_JS, "application/javascript; charset=utf-8") }),
        )
        .route(
            "/phone",
            get(|| async { page(PHONE_HTML, "text/html; charset=utf-8") }),
        )
        .route(
            "/phone.js",
            get(|| async { page(PHONE_JS, "application/javascript; charset=utf-8") }),
        )
        .route("/emulator/{*path}", get(emulator_file))
        .route(
            "/style.css",
            get(|| async { page(STYLE_CSS, "text/css; charset=utf-8") }),
        )
        // A worker may only control what sits at or below its own path, so
        // this one is served from the root and no deeper.
        .route(
            "/sw.js",
            get(|| async { page(SERVICE_WORKER, "application/javascript; charset=utf-8") }),
        )
        .route(
            "/manifest.webmanifest",
            get(|| async { page(MANIFEST, "application/manifest+json; charset=utf-8") }),
        )
        .route(
            "/icon-192.png",
            get(|| async { image(ICON_192, "image/png") }),
        )
        .route(
            "/icon-512.png",
            get(|| async { image(ICON_512, "image/png") }),
        )
        .route(
            "/icon-192-maskable.png",
            get(|| async { image(ICON_192_MASKABLE, "image/png") }),
        )
        .route(
            "/icon-512-maskable.png",
            get(|| async { image(ICON_512_MASKABLE, "image/png") }),
        )
        .route("/api/library", get(api_library))
        .route("/api/game/{id}", get(api_game))
        .route("/api/rom/{id}/{file}", get(api_rom))
        .route("/api/launch/{id}", post(api_launch))
        .route("/api/stop/{id}", post(api_stop))
        .route("/api/stop", post(api_stop_all))
        .route("/api/status", get(api_status))
        .fallback(not_a_page)
        .with_state(shelf)
        .layer(axum::middleware::from_fn(check_who_is_asking))
}

pub async fn serve(play: SharedPlay, addr: SocketAddr) -> std::io::Result<()> {
    let listener = tokio::net::TcpListener::bind(addr).await?;
    let local = listener.local_addr()?;
    log::info!("web remote listening on http://{local}");
    for row in reachable(local) {
        log::info!("web remote reachable from {}: {}", row.reach, row.url);
    }
    axum::serve(listener, router(play)).await
}

/// Every request, before it reaches the shelf. See [`guard`] for what is
/// refused and why.
async fn check_who_is_asking(request: Request, next: Next) -> Response {
    // Everything borrowed from the request is read and dropped here: a body
    // is not `Sync`, so a borrow of it held across the await below would make
    // this future unable to move between threads.
    let refused = {
        let headers = request.headers();
        let text = |name: HeaderName| headers.get(name).and_then(|v| v.to_str().ok());
        let host = text(header::HOST)
            .map(str::to_string)
            .or_else(|| request.uri().host().map(str::to_string));
        let origin = text(header::ORIGIN).map(str::to_string);
        guard::refusal(
            request.method().as_str(),
            host.as_deref(),
            origin.as_deref(),
        )
    };
    if let Some(why) = refused {
        log::warn!("refused a request: {why}");
        return in_words(StatusCode::FORBIDDEN, &why);
    }
    next.run(request).await
}

async fn not_a_page() -> Response {
    in_words(StatusCode::NOT_FOUND, "There is no such page on the shelf.")
}

fn in_words(status: StatusCode, message: &str) -> Response {
    (status, Json(serde_json::json!({ "error": message }))).into_response()
}

#[derive(Serialize)]
struct StatusView {
    running: usize,
    playing: Vec<RunningGame>,
}

#[derive(Serialize)]
struct StoppedView {
    stopped: usize,
}

async fn api_library(State(Shelf { play, .. }): State<Shelf>) -> Response {
    respond(play, views::library_view).await
}

async fn api_game(State(Shelf { play, .. }): State<Shelf>, Path(id): Path<i64>) -> Response {
    respond(play, move |play| views::game_view(play, id)).await
}

/// The game itself, for the phone's emulator to run. Read-only, like the
/// rest of the shelf, and served in pieces when the browser asks for ranges.
async fn api_rom(
    State(Shelf { play, .. }): State<Shelf>,
    Path((id, _file)): Path<(i64, String)>,
    request: Request,
) -> Response {
    let found = tokio::task::spawn_blocking(move || {
        let play = play.lock().map_err(|_| "the library is busy".to_string())?;
        let game = play
            .db()
            .get_game(id)
            .map_err(|e| e.to_string())?
            .ok_or_else(|| "game not found".to_string())?;
        if let Some(why) = views::phone_view(&game).problem {
            return Err(why);
        }
        let path = PathBuf::from(&game.path);
        if !path.is_file() {
            return Err(format!(
                "{} is no longer where the shelf put it: {}",
                game.title, game.path
            ));
        }
        Ok(path)
    })
    .await;
    let path = match found {
        Ok(Ok(path)) => path,
        Ok(Err(message)) => {
            let status = if message.contains("not found") {
                StatusCode::NOT_FOUND
            } else {
                StatusCode::BAD_REQUEST
            };
            return in_words(status, &message);
        }
        Err(_) => {
            return in_words(
                StatusCode::INTERNAL_SERVER_ERROR,
                "the request did not finish",
            )
        }
    };
    match ServeFile::new(path).oneshot(request).await {
        Ok(response) => {
            let mut response = response.into_response();
            set(
                &mut response,
                header::CONTENT_TYPE,
                "application/octet-stream",
            );
            response
        }
        Err(_) => in_words(
            StatusCode::INTERNAL_SERVER_ERROR,
            "the game could not be read",
        ),
    }
}

fn set(response: &mut Response, name: HeaderName, value: &'static str) {
    response
        .headers_mut()
        .insert(name, axum::http::HeaderValue::from_static(value));
}

/// A file of the phone's emulator, downloaded the first time any phone asks.
async fn emulator_file(
    State(Shelf { emulator, .. }): State<Shelf>,
    Path(path): Path<String>,
    request: Request,
) -> Response {
    let wanted = path.clone();
    let found = tokio::task::spawn_blocking(move || emulator::file(&emulator, &wanted)).await;
    let file = match found {
        Ok(Ok(file)) => file,
        Ok(Err(message)) => {
            let status = if message.starts_with("There is no such file") {
                StatusCode::NOT_FOUND
            } else {
                StatusCode::BAD_GATEWAY
            };
            return in_words(status, &message);
        }
        Err(_) => {
            return in_words(
                StatusCode::INTERNAL_SERVER_ERROR,
                "the request did not finish",
            )
        }
    };
    match ServeFile::new(file).oneshot(request).await {
        Ok(response) => {
            let mut response = response.into_response();
            set(
                &mut response,
                header::CONTENT_TYPE,
                emulator::content_type(&path),
            );
            // Pinned to one version, so a file at this address never changes.
            set(&mut response, header::CACHE_CONTROL, KEEP_A_WHILE);
            response
        }
        Err(_) => in_words(
            StatusCode::INTERNAL_SERVER_ERROR,
            "the emulator could not be read",
        ),
    }
}

async fn api_launch(State(Shelf { play, .. }): State<Shelf>, Path(id): Path<i64>) -> Response {
    respond(play, move |play| play.launch(id).map_err(|e| e.to_string())).await
}

async fn api_stop(State(Shelf { play, .. }): State<Shelf>, Path(id): Path<i64>) -> Response {
    respond(play, move |play| {
        Ok(StoppedView {
            stopped: play.stop(id),
        })
    })
    .await
}

async fn api_stop_all(State(Shelf { play, .. }): State<Shelf>) -> Response {
    respond(play, |play| {
        Ok(StoppedView {
            stopped: play.stop_all(),
        })
    })
    .await
}

async fn api_status(State(Shelf { play, .. }): State<Shelf>) -> Response {
    respond(play, |play| {
        let playing = play.running();
        Ok(StatusView {
            running: playing.len(),
            playing,
        })
    })
    .await
}

// rusqlite's connection is Send but not Sync, so every request takes the
// same lock the desktop shell takes, on a blocking thread, never across
// an await.
async fn respond<T, F>(play: SharedPlay, f: F) -> Response
where
    T: Serialize + Send + 'static,
    F: FnOnce(&Play) -> Result<T, String> + Send + 'static,
{
    let outcome = tokio::task::spawn_blocking(move || {
        let guard = play.lock().map_err(|_| "the library is busy".to_string())?;
        f(&guard)
    })
    .await;
    match outcome {
        Ok(Ok(value)) => Json(value).into_response(),
        Ok(Err(message)) => {
            let status = if message.contains("not found") {
                StatusCode::NOT_FOUND
            } else {
                StatusCode::BAD_REQUEST
            };
            in_words(status, &message)
        }
        Err(_) => in_words(
            StatusCode::INTERNAL_SERVER_ERROR,
            "the request did not finish",
        ),
    }
}
