//! The desktop entry point.
//!
//! Everything is in the library so other targets can link the same code with
//! their own entry point.

#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

fn main() {
    den_desktop_lib::run();
}
