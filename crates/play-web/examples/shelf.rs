//! Serve a library to the network without building the desktop app.
//!
//! ```sh
//! cargo run -p play-web --example shelf              # the real library
//! cargo run -p play-web --example shelf -- /some/play # another one
//! ```
//!
//! The remote's page is the one part of Play that cannot be looked at without
//! a WebView, and the desktop shell needs system packages that a headless
//! machine will not have. This serves the same page, from the same code the
//! app runs, with nothing but a Rust toolchain — so the shelf can be opened on
//! a phone and worked on. `PLAY_WEB_PORT` and `PLAY_WEB_BIND` apply as usual.

use play_core::Play;
use std::sync::{Arc, Mutex};

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    env_logger::Builder::from_env(env_logger::Env::default().default_filter_or("info")).init();

    let library = std::env::args()
        .nth(1)
        .map(std::path::PathBuf::from)
        .unwrap_or_else(Play::default_library);
    let play = Play::open(&library)?;
    log::info!("serving the shelf at {}", library.display());

    let Some(addr) = play_web::addr_from_env() else {
        log::warn!("PLAY_WEB_PORT=0, so there is nothing to serve");
        return Ok(());
    };
    play_web::serve(Arc::new(Mutex::new(play)), addr).await?;
    Ok(())
}
