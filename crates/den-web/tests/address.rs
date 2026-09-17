//! `DEN_WEB_PORT` and `DEN_WEB_BIND` are process-wide, so they get a test
//! binary to themselves.

use den_web::{addr_from_env, reachable, DEFAULT_PORT};
use std::sync::{Mutex, MutexGuard};

static ENV: Mutex<()> = Mutex::new(());

/// One test at a time may own the two keys.
fn exclusive() -> MutexGuard<'static, ()> {
    ENV.lock().unwrap_or_else(|e| e.into_inner())
}

#[test]
fn the_remote_is_on_5555_until_told_otherwise_and_zero_turns_it_off() {
    let _owned = exclusive();
    std::env::remove_var("DEN_WEB_PORT");
    std::env::remove_var("DEN_WEB_BIND");
    let addr = addr_from_env().expect("a default address");
    assert_eq!(addr.port(), DEFAULT_PORT);
    assert!(
        addr.ip().is_unspecified(),
        "the default answers the network"
    );

    std::env::set_var("DEN_WEB_PORT", "0");
    assert!(addr_from_env().is_none(), "port 0 turns the remote off");

    std::env::set_var("DEN_WEB_PORT", "7000");
    assert_eq!(addr_from_env().expect("addr").port(), 7000);

    // Nonsense is said out loud and the default is used, not a panic.
    std::env::set_var("DEN_WEB_PORT", "banana");
    assert_eq!(addr_from_env().expect("addr").port(), DEFAULT_PORT);
    std::env::remove_var("DEN_WEB_PORT");
}

#[test]
fn binding_to_the_tailnet_alone_is_the_only_address_offered() {
    let _owned = exclusive();
    std::env::set_var("DEN_WEB_PORT", "5555");
    std::env::set_var("DEN_WEB_BIND", "127.0.0.1");
    let addr = addr_from_env().expect("addr");
    assert_eq!(addr.ip().to_string(), "127.0.0.1");
    let rows = reachable(addr);
    assert_eq!(rows.len(), 1);
    assert_eq!(rows[0].url, "http://127.0.0.1:5555");
    assert_eq!(rows[0].reach, "this machine");
    std::env::remove_var("DEN_WEB_BIND");
    std::env::remove_var("DEN_WEB_PORT");
}
