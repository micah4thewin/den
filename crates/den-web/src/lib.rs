use std::net::{IpAddr, SocketAddr};
use std::sync::{Arc, Mutex};

use axum::extract::{Path, State};
use axum::http::{header, StatusCode};
use axum::response::{Html, IntoResponse, Response};
use axum::routing::{get, post};
use axum::{Json, Router};
use den_core::Den;
use serde::Serialize;

pub mod views;

pub const DEFAULT_PORT: u16 = 5555;

pub type SharedDen = Arc<Mutex<Den>>;

const INDEX_HTML: &str = include_str!("../web/index.html");
const APP_JS: &str = include_str!("../web/app.js");
const STYLE_CSS: &str = include_str!("../web/style.css");

// The remote is open to the network it is on, the same trust the family's
// media server extends: it can read the shelf and start games that are
// already on it, nothing else. DEN_WEB_PORT=0 turns it off.
pub fn addr_from_env() -> Option<SocketAddr> {
    let port = match std::env::var("DEN_WEB_PORT") {
        Ok(raw) => match raw.trim().parse::<u16>() {
            Ok(0) => return None,
            Ok(p) => p,
            Err(_) => {
                log::warn!("DEN_WEB_PORT is not a port number; using {DEFAULT_PORT}");
                DEFAULT_PORT
            }
        },
        Err(_) => DEFAULT_PORT,
    };
    let bind = match std::env::var("DEN_WEB_BIND") {
        Ok(raw) => match raw.trim().parse::<IpAddr>() {
            Ok(ip) => ip,
            Err(_) => {
                log::warn!("DEN_WEB_BIND is not an address; using 0.0.0.0");
                IpAddr::from([0, 0, 0, 0])
            }
        },
        Err(_) => IpAddr::from([0, 0, 0, 0]),
    };
    Some(SocketAddr::new(bind, port))
}

pub fn router(den: SharedDen) -> Router {
    Router::new()
        .route("/", get(|| async { Html(INDEX_HTML) }))
        .route(
            "/app.js",
            get(|| async {
                (
                    [(
                        header::CONTENT_TYPE,
                        "application/javascript; charset=utf-8",
                    )],
                    APP_JS,
                )
            }),
        )
        .route(
            "/style.css",
            get(|| async {
                (
                    [(header::CONTENT_TYPE, "text/css; charset=utf-8")],
                    STYLE_CSS,
                )
            }),
        )
        .route("/api/library", get(api_library))
        .route("/api/game/{id}", get(api_game))
        .route("/api/launch/{id}", post(api_launch))
        .route("/api/status", get(api_status))
        .with_state(den)
}

pub async fn serve(den: SharedDen, addr: SocketAddr) -> std::io::Result<()> {
    let listener = tokio::net::TcpListener::bind(addr).await?;
    let local = listener.local_addr()?;
    log::info!("web remote listening on http://{local}");
    for url in reachable_urls(local) {
        log::info!("web remote reachable at {url}");
    }
    axum::serve(listener, router(den)).await
}

#[derive(Serialize)]
struct StatusView {
    running: usize,
}

async fn api_library(State(den): State<SharedDen>) -> Response {
    respond(den, views::library_view).await
}

async fn api_game(State(den): State<SharedDen>, Path(id): Path<i64>) -> Response {
    respond(den, move |den| views::game_view(den, id)).await
}

async fn api_launch(State(den): State<SharedDen>, Path(id): Path<i64>) -> Response {
    respond(den, move |den| den.launch(id).map_err(|e| e.to_string())).await
}

async fn api_status(State(den): State<SharedDen>) -> Response {
    respond(den, |den| {
        den.reap();
        Ok(StatusView {
            running: den.running_count(),
        })
    })
    .await
}

// rusqlite's connection is Send but not Sync, so every request takes the
// same lock the desktop shell takes, on a blocking thread, never across
// an await.
async fn respond<T, F>(den: SharedDen, f: F) -> Response
where
    T: Serialize + Send + 'static,
    F: FnOnce(&Den) -> Result<T, String> + Send + 'static,
{
    let outcome = tokio::task::spawn_blocking(move || {
        let guard = den.lock().map_err(|_| "the library is busy".to_string())?;
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
            (status, Json(serde_json::json!({ "error": message }))).into_response()
        }
        Err(_) => (
            StatusCode::INTERNAL_SERVER_ERROR,
            Json(serde_json::json!({ "error": "the request did not finish" })),
        )
            .into_response(),
    }
}

// Route probes, not interface enumeration: ask the kernel which local
// address it would use to reach each of these. No packet leaves the
// machine for a connected UDP socket.
const ROUTE_PROBES: &[&str] = &["1.1.1.1:9", "192.168.1.1:9", "10.0.0.1:9", "172.16.0.1:9"];

pub fn reachable_urls(local: SocketAddr) -> Vec<String> {
    let port = local.port();
    if !local.ip().is_unspecified() {
        return vec![format!("http://{local}")];
    }
    let mut found: Vec<IpAddr> = Vec::new();
    for target in ROUTE_PROBES {
        let Ok(socket) = std::net::UdpSocket::bind("0.0.0.0:0") else {
            continue;
        };
        if socket.connect(target).is_err() {
            continue;
        }
        let Ok(addr) = socket.local_addr() else {
            continue;
        };
        let ip = addr.ip();
        if ip.is_loopback() || ip.is_unspecified() || found.contains(&ip) {
            continue;
        }
        found.push(ip);
    }
    found
        .into_iter()
        .map(|ip| match ip {
            IpAddr::V4(v4) => format!("http://{v4}:{port}"),
            IpAddr::V6(v6) => format!("http://[{v6}]:{port}"),
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use http_body_util::BodyExt;
    use tower::util::ServiceExt;

    fn test_den() -> (tempfile::TempDir, SharedDen) {
        let tmp = tempfile::tempdir().expect("tempdir");
        let den = Den::open(&tmp.path().join("den")).expect("open library");
        (tmp, Arc::new(Mutex::new(den)))
    }

    async fn get_json(router: Router, uri: &str) -> (StatusCode, serde_json::Value) {
        let response = router
            .oneshot(
                axum::http::Request::builder()
                    .uri(uri)
                    .body(axum::body::Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        let status = response.status();
        let bytes = response.into_body().collect().await.unwrap().to_bytes();
        let value = serde_json::from_slice(&bytes).unwrap_or(serde_json::Value::Null);
        (status, value)
    }

    #[tokio::test]
    async fn library_starts_empty_and_serves() {
        let (_tmp, den) = test_den();
        let (status, body) = get_json(router(den), "/api/library").await;
        assert_eq!(status, StatusCode::OK);
        assert_eq!(body["games"], serde_json::json!([]));
        assert!(body["retroarch"].is_object());
    }

    #[tokio::test]
    async fn missing_game_is_a_404_in_words() {
        let (_tmp, den) = test_den();
        let (status, body) = get_json(router(den), "/api/game/999").await;
        assert_eq!(status, StatusCode::NOT_FOUND);
        assert!(body["error"].as_str().unwrap().contains("not found"));
    }

    #[tokio::test]
    async fn launching_a_missing_game_fails_in_words() {
        let (_tmp, den) = test_den();
        let response = router(den)
            .oneshot(
                axum::http::Request::builder()
                    .method("POST")
                    .uri("/api/launch/999")
                    .body(axum::body::Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_ne!(response.status(), StatusCode::OK);
    }

    #[tokio::test]
    async fn status_counts_nothing_running() {
        let (_tmp, den) = test_den();
        let (status, body) = get_json(router(den), "/api/status").await;
        assert_eq!(status, StatusCode::OK);
        assert_eq!(body["running"], 0);
    }

    #[tokio::test]
    async fn the_client_ships_inside_the_binary() {
        let (_tmp, den) = test_den();
        let response = router(den)
            .oneshot(
                axum::http::Request::builder()
                    .uri("/")
                    .body(axum::body::Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(response.status(), StatusCode::OK);
        let bytes = response.into_body().collect().await.unwrap().to_bytes();
        let page = String::from_utf8_lossy(&bytes);
        assert!(page.contains("Play"));
    }

    #[test]
    fn env_port_zero_disables_the_remote() {
        // Env vars are process-wide; this test owns both keys.
        std::env::set_var("DEN_WEB_PORT", "0");
        assert!(addr_from_env().is_none());
        std::env::set_var("DEN_WEB_PORT", "5555");
        let addr = addr_from_env().expect("addr");
        assert_eq!(addr.port(), 5555);
        std::env::remove_var("DEN_WEB_PORT");
    }
}
