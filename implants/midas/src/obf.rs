//! Compile-time string obfuscation.
//!
//! The Rust equivalent of Tombili's `obf()` macro in `utils.nim`. A string
//! literal is XOR-encoded during const evaluation and decoded at the point of
//! use, so the plaintext never appears in the compiled binary.
//!
//! This is obfuscation, not encryption. It defeats a casual `strings` pass; it
//! does not survive anything that debugs the process.

include!(concat!(env!("OUT_DIR"), "/seed.rs"));

/// Upper bound on a single obfuscated literal.
pub const MAX_LEN: usize = 256;

/// XOR-encode `s` at compile time. `out` is zero-padded past `s.len()`; the
/// padding is never decoded because every call site passes the original length.
pub const fn encode_const(s: &[u8], seed: u32) -> [u8; MAX_LEN] {
    let mut out = [0u8; MAX_LEN];
    let mut k = seed | 1;
    let mut i = 0;

    while i < s.len() && i < MAX_LEN {
        k = k.wrapping_add(1);
        out[i] = s[i] ^ (k & 0xFF) as u8;
        i += 1;
    }

    out
}

/// Encodes a string that is only known at compile time from *outside* the
/// source, such as a value baked in by `option_env!`.
///
/// `obf!` cannot cover those: it needs a literal written in the source, and
/// `option_env!` expands to a literal the build environment supplied, which the
/// compiler would otherwise embed verbatim. This takes the already-materialised
/// `&str` and returns `(length, bytes)` for `decode`.
pub const fn encode_static(s: &str) -> (usize, [u8; MAX_LEN]) {
    (s.len(), encode_const(s.as_bytes(), SEED))
}

/// Decode the first `len` bytes of `encoded`.
pub fn decode(encoded: &[u8; MAX_LEN], len: usize, seed: u32) -> String {
    let mut k = seed | 1;
    let mut out = Vec::with_capacity(len);

    for &b in encoded.iter().take(len) {
        k = k.wrapping_add(1);
        out.push(b ^ (k & 0xFF) as u8);
    }

    String::from_utf8_lossy(&out).into_owned()
}

/// Obfuscate a string literal.
///
/// Encoding happens in a `const` item, so the work is done by the compiler and
/// nothing is computed at startup. Takes a literal rather than an expression so
/// that `$s.as_bytes()` and `$s.len()` are both const-evaluable.
#[macro_export]
macro_rules! obf {
    ($s:literal) => {{
        const ENCODED: [u8; $crate::obf::MAX_LEN] =
            $crate::obf::encode_const($s.as_bytes(), $crate::obf::SEED);
        $crate::obf::decode(&ENCODED, $s.len(), $crate::obf::SEED)
    }};
}
