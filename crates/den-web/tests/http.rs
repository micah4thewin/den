use axum::body::Body;
use axum::http::{Request, StatusCode};
use axum::Router;
use den_core::Den;
use den_web::{router, SharedDen};
use http_body_util::BodyExt;
use std::sync::{Arc, Mutex};
use tower::util::ServiceExt;

fn test_den() -> (tempfile::TempDir, SharedDen) {
    let tmp = tempfile::tempdir().expect("tempdir");
    let den = Den::open(&tmp.path().join("den")).expect("open library");
    (tmp, Arc::new(Mutex::new(den)))
}

struct Answer {
    status: StatusCode,
    content_type: String,
    body: Vec<u8>,
}

impl Answer {
    fn json(&self) -> serde_json::Value {
        serde_json::from_slice(&self.body).unwrap_or(serde_json::Value::Null)
    }

    fn text(&self) -> String {
        String::from_utf8_lossy(&self.body).into_owned()
    }

    fn error(&self) -> String {
        self.json()["error"]
            .as_str()
            .unwrap_or_default()
            .to_string()
    }
}

async fn ask(router: Router, request: Request<Body>) -> Answer {
    let response = router.oneshot(request).await.unwrap();
    let status = response.status();
    let content_type = response
        .headers()
        .get(axum::http::header::CONTENT_TYPE)
        .and_then(|v| v.to_str().ok())
        .unwrap_or_default()
        .to_string();
    let body = response.into_body().collect().await.unwrap().to_bytes();
    Answer {
        status,
        content_type,
        body: body.to_vec(),
    }
}

async fn get(router: Router, uri: &str) -> Answer {
    ask(
        router,
        Request::builder().uri(uri).body(Body::empty()).unwrap(),
    )
    .await
}

async fn post(router: Router, uri: &str) -> Answer {
    ask(
        router,
        Request::builder()
            .method("POST")
            .uri(uri)
            .body(Body::empty())
            .unwrap(),
    )
    .await
}

#[tokio::test]
async fn library_starts_empty_and_serves() {
    let (_tmp, den) = test_den();
    let answer = get(router(den), "/api/library").await;
    assert_eq!(answer.status, StatusCode::OK);
    assert_eq!(answer.json()["games"], serde_json::json!([]));
    assert!(answer.json()["retroarch"].is_object());
}

#[tokio::test]
async fn missing_game_is_a_404_in_words() {
    let (_tmp, den) = test_den();
    let answer = get(router(den), "/api/game/999").await;
    assert_eq!(answer.status, StatusCode::NOT_FOUND);
    assert!(answer.error().contains("not found"));
}

#[tokio::test]
async fn launching_a_missing_game_fails_in_words() {
    let (_tmp, den) = test_den();
    let answer = post(router(den), "/api/launch/999").await;
    assert_ne!(answer.status, StatusCode::OK);
    assert!(!answer.error().is_empty());
}

#[tokio::test]
async fn status_counts_nothing_running_and_names_nothing() {
    let (_tmp, den) = test_den();
    let answer = get(router(den), "/api/status").await;
    assert_eq!(answer.status, StatusCode::OK);
    assert_eq!(answer.json()["running"], 0);
    assert_eq!(answer.json()["playing"], serde_json::json!([]));
}

#[tokio::test]
async fn stopping_what_is_not_playing_is_the_answer_that_was_wanted() {
    let (_tmp, den) = test_den();
    let answer = post(router(den.clone()), "/api/stop/999").await;
    assert_eq!(answer.status, StatusCode::OK);
    assert_eq!(answer.json()["stopped"], 0);

    let all = post(router(den), "/api/stop").await;
    assert_eq!(all.status, StatusCode::OK);
    assert_eq!(all.json()["stopped"], 0);
}

#[tokio::test]
async fn the_client_ships_inside_the_binary() {
    let (_tmp, den) = test_den();
    let page = get(router(den.clone()), "/").await;
    assert_eq!(page.status, StatusCode::OK);
    assert!(page.text().contains("Play"));

    for (path, expected) in [
        ("/app.js", "javascript"),
        ("/style.css", "text/css"),
        ("/sw.js", "javascript"),
        ("/manifest.webmanifest", "application/manifest+json"),
        ("/icon-192.png", "image/png"),
        ("/icon-512-maskable.png", "image/png"),
    ] {
        let answer = get(router(den.clone()), path).await;
        assert_eq!(answer.status, StatusCode::OK, "{path} was not served");
        assert!(
            answer.content_type.contains(expected),
            "{path} came back as {}",
            answer.content_type
        );
        assert!(!answer.body.is_empty(), "{path} was empty");
    }
}

#[tokio::test]
async fn a_phone_is_told_how_to_keep_the_shelf_on_its_home_screen() {
    let (_tmp, den) = test_den();
    let answer = get(router(den), "/manifest.webmanifest").await;
    let manifest: serde_json::Value = answer.json();
    assert_eq!(manifest["display"], "standalone");
    assert_eq!(manifest["start_url"], "/");
    let purposes: Vec<&str> = manifest["icons"]
        .as_array()
        .expect("icons")
        .iter()
        .filter_map(|icon| icon["purpose"].as_str())
        .collect();
    // Android masks the home-screen icon, so one has to be drawn for it.
    assert!(
        purposes.contains(&"maskable"),
        "no maskable icon: {purposes:?}"
    );
    assert!(purposes.contains(&"any"));
}

#[tokio::test]
async fn a_page_that_is_not_here_says_so_in_words() {
    let (_tmp, den) = test_den();
    let answer = get(router(den), "/wherever").await;
    assert_eq!(answer.status, StatusCode::NOT_FOUND);
    assert!(!answer.error().is_empty());
}

#[tokio::test]
async fn somebody_elses_page_cannot_start_a_game() {
    let (_tmp, den) = test_den();
    let answer = ask(
        router(den.clone()),
        Request::builder()
            .method("POST")
            .uri("/api/launch/1")
            .header("host", "192.168.1.5:5555")
            .header("origin", "https://elsewhere.example")
            .body(Body::empty())
            .unwrap(),
    )
    .await;
    assert_eq!(answer.status, StatusCode::FORBIDDEN);
    assert!(
        answer.error().contains("elsewhere.example"),
        "{}",
        answer.error()
    );

    // The shelf's own page is not refused; the game simply is not there.
    let mine = ask(
        router(den),
        Request::builder()
            .method("POST")
            .uri("/api/launch/1")
            .header("host", "192.168.1.5:5555")
            .header("origin", "http://192.168.1.5:5555")
            .body(Body::empty())
            .unwrap(),
    )
    .await;
    assert_ne!(mine.status, StatusCode::FORBIDDEN);
}

#[tokio::test]
async fn a_name_pointed_at_this_machine_from_outside_is_refused() {
    let (_tmp, den) = test_den();
    let answer = ask(
        router(den.clone()),
        Request::builder()
            .uri("/api/library")
            .header("host", "games.example.com")
            .body(Body::empty())
            .unwrap(),
    )
    .await;
    assert_eq!(answer.status, StatusCode::FORBIDDEN);
    assert!(answer.error().contains("DEN_WEB_HOSTS"));

    // The names a private network actually uses are answered to.
    for host in [
        "192.168.1.5:5555",
        "100.101.20.33:5555",
        "box.tail1234.ts.net",
        "desktop",
    ] {
        let ok = ask(
            router(den.clone()),
            Request::builder()
                .uri("/api/library")
                .header("host", host)
                .body(Body::empty())
                .unwrap(),
        )
        .await;
        assert_eq!(ok.status, StatusCode::OK, "{host} was turned away");
    }
}
