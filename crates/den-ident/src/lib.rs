//! Identification for Den: hashing, magic-byte sniffing, and the DAT index.
//!
//! This crate is deliberately pure: it reads files and returns answers, and
//! nothing else. No platform code, no state, so it builds headless and can
//! be fuzzed against the corpus.

mod system;

pub mod dat;
pub mod hash;
pub mod magic;

pub use system::System;
