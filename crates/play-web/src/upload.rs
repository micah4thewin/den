//! Adding games from another device: drop files or a folder on the remote's
//! page and they are filed on the machine the library lives on, exactly as
//! if they had been dropped on the desktop window.
//!
//! Each file goes up on its own, `PUT /api/upload/<batch>/<path>`, streamed
//! straight to disk, so a disc image never has to fit in memory. Then
//! `POST /api/intake/<batch>` files the whole batch as one drop and answers
//! with the intake report. The upload folder is removed afterwards; what was
//! worth keeping is on the shelf by then.
//!
//! `PLAY_WEB_UPLOAD=0` turns this off and leaves the remote read-and-play
//! only. The same guard as every other change applies: a page that is not
//! Play's own cannot upload.

use crate::{words, WebState};
use axum::body::Body;
use axum::extract::{Path, State};
use axum::http::StatusCode;
use axum::response::{IntoResponse, Response};
use axum::Json;
use http_body_util::BodyExt;
use std::path::PathBuf;
use tokio::io::AsyncWriteExt;

const UPLOADS: &str = "_uploads";

fn allowed() -> bool {
    !std::env::var("PLAY_WEB_UPLOAD")
        .is_ok_and(|v| v.trim() == "0" || v.eq_ignore_ascii_case("off"))
}

fn batch_ok(batch: &str) -> bool {
    (8..=64).contains(&batch.len())
        && batch
            .bytes()
            .all(|b| b.is_ascii_alphanumeric() || b == b'-')
}

/// `path` under `root`, or nothing if any part of it would climb out.
fn inside(root: &std::path::Path, path: &str) -> Option<PathBuf> {
    let mut out = root.to_path_buf();
    let mut parts = 0;
    for part in path.split(['/', '\\']) {
        match part {
            "" | "." => continue,
            ".." => return None,
            p if p.contains(':') || p.contains('\0') => return None,
            p => {
                out.push(p);
                parts += 1;
            }
        }
    }
    (parts > 0).then_some(out)
}

pub(crate) async fn put_file(
    State(state): State<WebState>,
    Path((batch, path)): Path<(String, String)>,
    body: Body,
) -> Response {
    if !allowed() {
        return words(
            StatusCode::FORBIDDEN,
            "Adding games from the remote is turned off.",
        );
    }
    if !batch_ok(&batch) {
        return words(StatusCode::BAD_REQUEST, "That upload has no proper name.");
    }
    let root = state.library.join(UPLOADS).join(&batch);
    let Some(target) = inside(&root, &path) else {
        return words(
            StatusCode::BAD_REQUEST,
            "That file name is not one Play can keep.",
        );
    };
    if let Some(parent) = target.parent() {
        if let Err(e) = tokio::fs::create_dir_all(parent).await {
            return words(StatusCode::INTERNAL_SERVER_ERROR, &e.to_string());
        }
    }
    let partial = target.with_extension(match target.extension() {
        Some(ext) => format!("{}.part", ext.to_string_lossy()),
        None => "part".to_string(),
    });
    let mut file = match tokio::fs::File::create(&partial).await {
        Ok(f) => f,
        Err(e) => return words(StatusCode::INTERNAL_SERVER_ERROR, &e.to_string()),
    };
    let mut body = body;
    let mut written: u64 = 0;
    loop {
        match body.frame().await {
            Some(Ok(frame)) => {
                if let Ok(data) = frame.into_data() {
                    written += data.len() as u64;
                    if let Err(e) = file.write_all(&data).await {
                        let _ = tokio::fs::remove_file(&partial).await;
                        return words(StatusCode::INSUFFICIENT_STORAGE, &e.to_string());
                    }
                }
            }
            Some(Err(e)) => {
                let _ = tokio::fs::remove_file(&partial).await;
                return words(
                    StatusCode::BAD_REQUEST,
                    &format!("the upload was cut off: {e}"),
                );
            }
            None => break,
        }
    }
    if let Err(e) = file.flush().await {
        return words(StatusCode::INTERNAL_SERVER_ERROR, &e.to_string());
    }
    drop(file);
    if let Err(e) = tokio::fs::rename(&partial, &target).await {
        return words(StatusCode::INTERNAL_SERVER_ERROR, &e.to_string());
    }
    Json(serde_json::json!({ "written": written })).into_response()
}

#[derive(serde::Deserialize, Default)]
pub(crate) struct IntakeAsk {
    password: Option<String>,
}

pub(crate) async fn run_batch(
    State(state): State<WebState>,
    Path(batch): Path<String>,
    ask: Option<Json<IntakeAsk>>,
) -> Response {
    if !allowed() {
        return words(
            StatusCode::FORBIDDEN,
            "Adding games from the remote is turned off.",
        );
    }
    if !batch_ok(&batch) {
        return words(StatusCode::BAD_REQUEST, "That upload has no proper name.");
    }
    let root = state.library.join(UPLOADS).join(&batch);
    if !root.is_dir() {
        return words(
            StatusCode::NOT_FOUND,
            "Nothing was uploaded under that name.",
        );
    }
    let password = ask.and_then(|Json(a)| a.password).filter(|p| !p.is_empty());
    let play = state.play.clone();
    let folder = root.clone();
    let outcome = tokio::task::spawn_blocking(move || {
        let drops: Vec<PathBuf> = std::fs::read_dir(&folder)
            .map(|rd| rd.flatten().map(|e| e.path()).collect())
            .unwrap_or_default();
        let guard = play.lock().map_err(|_| "the library is busy".to_string())?;
        let report = guard
            .intake_many(&drops, password)
            .map_err(|e| e.to_string());
        guard.fetch_art_in_background();
        report
    })
    .await;
    let keep_for_retry = matches!(&outcome, Ok(Ok(report)) if report.entries.iter().any(|e|
        matches!(&e.outcome, play_core::Outcome::Quarantined { reason } if reason.contains("password"))));
    if !keep_for_retry {
        let _ = tokio::fs::remove_dir_all(&root).await;
        let _ = tokio::fs::remove_dir(state.library.join(UPLOADS)).await;
    }
    match outcome {
        Ok(Ok(report)) => Json(report).into_response(),
        Ok(Err(message)) => words(StatusCode::BAD_REQUEST, &message),
        Err(_) => words(
            StatusCode::INTERNAL_SERVER_ERROR,
            "the intake did not finish",
        ),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn upload_paths_stay_inside() {
        let root = std::path::Path::new("/lib/_uploads/abc");
        assert_eq!(
            inside(root, "Folder/Game (USA).nes"),
            Some(root.join("Folder").join("Game (USA).nes"))
        );
        assert_eq!(inside(root, "../escape"), None);
        assert_eq!(inside(root, "a/../../b"), None);
        assert_eq!(inside(root, "C:/Windows"), None);
        assert_eq!(inside(root, ""), None);
        assert_eq!(inside(root, "//"), None);
    }

    #[test]
    fn batch_names_are_plain() {
        assert!(batch_ok("abcdef12-3456"));
        assert!(!batch_ok("short"));
        assert!(!batch_ok("../../../etc"));
    }
}
