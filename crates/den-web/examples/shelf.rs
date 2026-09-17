//! Serve a library to the network without building the desktop app.
//!
//! ```sh
//! cargo run -p den-web --example shelf              # the real library
//! cargo run -p den-web --example shelf -- /some/den # another one
//! ```
//!
//! The remote's page is the one part of Play that cannot be looked at without
//! a WebView, and the desktop shell needs system packages that a headless
//! machine will not have. This serves the same page, from the same code the
//! app runs, with nothing but a Rust toolchain — so the shelf can be opened on
//! a phone and worked on. `DEN_WEB_PORT` and `DEN_WEB_BIND` apply as usual.

use den_core::Den;
use std::sync::{Arc, Mutex};

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    env_logger::Builder::from_env(env_logger::Env::default().default_filter_or("info")).init();

    let library = std::env::args()
        .nth(1)
        .map(std::path::PathBuf::from)
        .unwrap_or_else(Den::default_library);
    let den = Den::open(&library)?;
    log::info!("serving the shelf at {}", library.display());

    let Some(addr) = den_web::addr_from_env() else {
        log::warn!("DEN_WEB_PORT=0, so there is nothing to serve");
        return Ok(());
    };
    den_web::serve(Arc::new(Mutex::new(den)), addr).await?;
    Ok(())
}
