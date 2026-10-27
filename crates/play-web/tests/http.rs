use axum::body::Body;
use axum::http::{Request, StatusCode};
use axum::Router;
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
    let (_tmp, play) = test_play();
    let answer = get(router(play), "/api/library").await;
    assert_eq!(answer.status, StatusCode::OK);
    assert_eq!(answer.json()["games"], serde_json::json!([]));
    assert!(answer.json()["retroarch"].is_object());
}

#[tokio::test]
async fn missing_game_is_a_404_in_words() {
    let (_tmp, play) = test_play();
    let answer = get(router(play), "/api/game/999").await;
    assert_eq!(answer.status, StatusCode::NOT_FOUND);
    assert!(answer.error().contains("not found"));
}

#[tokio::test]
async fn launching_a_missing_game_fails_in_words() {
    let (_tmp, play) = test_play();
    let answer = post(router(play), "/api/launch/999").await;
    assert_ne!(answer.status, StatusCode::OK);
    assert!(!answer.error().is_empty());
}

#[tokio::test]
async fn status_counts_nothing_running_and_names_nothing() {
    let (_tmp, play) = test_play();
    let answer = get(router(play), "/api/status").await;
    assert_eq!(answer.status, StatusCode::OK);
    assert_eq!(answer.json()["running"], 0);
    assert_eq!(answer.json()["playing"], serde_json::json!([]));
}

#[tokio::test]
async fn stopping_what_is_not_playing_is_the_answer_that_was_wanted() {
    let (_tmp, play) = test_play();
    let answer = post(router(play.clone()), "/api/stop/999").await;
    assert_eq!(answer.status, StatusCode::OK);
    assert_eq!(answer.json()["stopped"], 0);

    let all = post(router(play), "/api/stop").await;
    assert_eq!(all.status, StatusCode::OK);
    assert_eq!(all.json()["stopped"], 0);
}

#[tokio::test]
async fn the_client_ships_inside_the_binary() {
    let (_tmp, play) = test_play();
    let page = get(router(play.clone()), "/").await;
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
        let answer = get(router(play.clone()), path).await;
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
    let (_tmp, play) = test_play();
    let answer = get(router(play), "/manifest.webmanifest").await;
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
    let (_tmp, play) = test_play();
    let answer = get(router(play), "/wherever").await;
    assert_eq!(answer.status, StatusCode::NOT_FOUND);
    assert!(!answer.error().is_empty());
}

#[tokio::test]
async fn somebody_elses_page_cannot_start_a_game() {
    let (_tmp, play) = test_play();
    let answer = ask(
        router(play.clone()),
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
        router(play),
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
    let (_tmp, play) = test_play();
    let answer = ask(
        router(play.clone()),
        Request::builder()
            .uri("/api/library")
            .header("host", "games.example.com")
            .body(Body::empty())
            .unwrap(),
    )
    .await;
    assert_eq!(answer.status, StatusCode::FORBIDDEN);
    assert!(answer.error().contains("PLAY_WEB_HOSTS"));

    // The names a private network actually uses are answered to.
    for host in [
        "192.168.1.5:5555",
        "100.101.20.33:5555",
        "box.tail1234.ts.net",
        "desktop",
    ] {
        let ok = ask(
            router(play.clone()),
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

fn shelve(play: &SharedPlay, title: &str, system: &str, file: &std::path::Path) -> i64 {
    let play = play.lock().unwrap();
    play.db()
        .add_game(title, system, file, None, None, "added")
        .expect("add game")
}

#[tokio::test]
async fn an_n64_game_plays_on_the_phone_from_the_shelf() {
    let (tmp, play) = test_play();
    let rom = tmp.path().join("GoldenEye 007 (USA).z64");
    std::fs::write(&rom, b"\x80\x37\x12\x40 not really goldeneye").unwrap();
    let id = shelve(&play, "GoldenEye 007", "N64", &rom);

    let view = get(router(play.clone()), &format!("/api/game/{id}")).await;
    assert_eq!(view.status, StatusCode::OK);
    let phone = &view.json()["phone"];
    assert_eq!(phone["system"], "n64");
    assert_eq!(phone["problem"], serde_json::Value::Null);
    let url = phone["rom"].as_str().expect("a rom address").to_string();
    // The emulator picks the core by the address's extension.
    assert!(url.ends_with("/game.z64"), "{url}");

    let file = get(router(play.clone()), &url).await;
    assert_eq!(file.status, StatusCode::OK);
    assert_eq!(file.body, std::fs::read(&rom).unwrap());

    let page = get(router(play), &format!("/phone?id={id}")).await;
    assert_eq!(page.status, StatusCode::OK);
    assert!(page.text().contains("/phone.js"));
}

#[tokio::test]
async fn a_game_that_is_several_files_says_why_it_stays_on_the_machine() {
    let (tmp, play) = test_play();
    let cue = tmp.path().join("Disc.cue");
    std::fs::write(&cue, b"FILE \"Disc.bin\" BINARY").unwrap();
    let id = shelve(&play, "Disc", "PlayStation", &cue);

    let view = get(router(play.clone()), &format!("/api/game/{id}")).await;
    let phone = &view.json()["phone"];
    assert_eq!(phone["system"], serde_json::Value::Null);
    assert!(phone["problem"].as_str().unwrap().contains("several files"));

    let file = get(router(play), &format!("/api/rom/{id}/game.cue")).await;
    assert_eq!(file.status, StatusCode::BAD_REQUEST);
}

#[tokio::test]
async fn the_phones_emulator_is_served_from_the_machines_copy() {
    let (tmp, play) = test_play();
    let library = play.lock().unwrap().library.clone();
    let pkg = library
        .join("emulator")
        .join(play_web::emulator::VERSION)
        .join("emulatorjs");
    std::fs::create_dir_all(pkg.join("package/data")).unwrap();
    std::fs::write(pkg.join("package/data/loader.js"), "// the loader").unwrap();
    std::fs::write(pkg.join(".complete"), "").unwrap();

    let answer = get(router(play.clone()), "/emulator/loader.js").await;
    assert_eq!(answer.status, StatusCode::OK);
    assert!(answer.content_type.contains("javascript"));
    assert_eq!(answer.text(), "// the loader");

    let outside = get(router(play), "/emulator/../library.db").await;
    assert_ne!(outside.status, StatusCode::OK);
    drop(tmp);
}
