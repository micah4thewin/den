//! Adding games from another device's browser.

use axum::body::Body;
use axum::http::{Request, StatusCode};
use http_body_util::BodyExt;
use play_core::Play;
use play_web::{router, SharedPlay};
use std::sync::{Arc, Mutex};
use tower::util::ServiceExt;

fn test_play() -> (tempfile::TempDir, SharedPlay) {
    let tmp = tempfile::tempdir().expect("tempdir");
    let play = Play::open(&tmp.path().join("play")).expect("open library");
    (tmp, Arc::new(Mutex::new(play)))
}

async fn send(
    play: &SharedPlay,
    method: &str,
    uri: &str,
    body: Vec<u8>,
) -> (StatusCode, serde_json::Value) {
    let request = Request::builder()
        .method(method)
        .uri(uri)
        .header("host", "192.168.1.20:5555")
        .header("origin", "http://192.168.1.20:5555")
        .body(Body::from(body))
        .unwrap();
    let response = router(play.clone()).oneshot(request).await.unwrap();
    let status = response.status();
    let bytes = response.into_body().collect().await.unwrap().to_bytes();
    (
        status,
        serde_json::from_slice(&bytes).unwrap_or(serde_json::Value::Null),
    )
}

#[tokio::test]
async fn files_uploaded_from_a_browser_land_on_the_shelf() {
    let (_tmp, play) = test_play();
    let mut rom = b"NES\x1a\x02\x01\x01\x00".to_vec();
    rom.extend([7u8; 2048]);

    let (status, _) = send(
        &play,
        "PUT",
        "/api/upload/batch-0001/My%20Folder/Remote%20Game%20(USA).nes",
        rom,
    )
    .await;
    assert_eq!(status, StatusCode::OK);

    let (status, report) = send(&play, "POST", "/api/intake/batch-0001", Vec::new()).await;
    assert_eq!(status, StatusCode::OK, "{report}");
    let entries = report["entries"].as_array().unwrap();
    assert_eq!(entries.len(), 1, "{report}");
    assert_eq!(entries[0]["outcome"]["word"], "probable");
    assert_eq!(entries[0]["input"], "My Folder/Remote Game (USA).nes");

    let library = play.lock().unwrap().library.clone();
    assert_eq!(play.lock().unwrap().db().game_count().unwrap(), 1);
    assert!(
        !library.join("_uploads").exists(),
        "the upload was not cleared away"
    );
}

#[tokio::test]
async fn an_upload_cannot_climb_out_of_its_folder() {
    let (_tmp, play) = test_play();
    let (status, _) = send(
        &play,
        "PUT",
        "/api/upload/batch-0002/..%2F..%2Fescape.txt",
        b"x".to_vec(),
    )
    .await;
    assert_eq!(status, StatusCode::BAD_REQUEST);
    let (status, _) = send(&play, "PUT", "/api/upload/../x", b"x".to_vec()).await;
    assert_ne!(status, StatusCode::OK);
}

#[tokio::test]
async fn somebody_elses_page_cannot_upload() {
    let (_tmp, play) = test_play();
    let request = Request::builder()
        .method("PUT")
        .uri("/api/upload/batch-0003/game.nes")
        .header("host", "192.168.1.20:5555")
        .header("origin", "https://evil.example.com")
        .body(Body::from(b"x".to_vec()))
        .unwrap();
    let response = router(play.clone()).oneshot(request).await.unwrap();
    assert_eq!(response.status(), StatusCode::FORBIDDEN);
}

#[tokio::test]
async fn a_cover_is_served_and_a_missing_one_is_a_404() {
    let (_tmp, play) = test_play();
    let library = play.lock().unwrap().library.clone();
    std::fs::create_dir_all(library.join("_art")).unwrap();
    std::fs::write(library.join("_art/7.png"), b"\x89PNG\r\n\x1a\nimage").unwrap();
    let get = |uri: &'static str| {
        let play = play.clone();
        async move {
            let request = Request::builder().uri(uri).body(Body::empty()).unwrap();
            router(play).oneshot(request).await.unwrap()
        }
    };
    let found = get("/api/cover/7").await;
    assert_eq!(found.status(), StatusCode::OK);
    assert_eq!(found.headers()["content-type"], "image/png");
    assert_eq!(get("/api/cover/8").await.status(), StatusCode::NOT_FOUND);
}
