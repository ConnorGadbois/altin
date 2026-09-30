//! Check-in pacing.
//!
//! Tombili keeps this in module-level `var`s; Midas uses atomics so `setsleep`
//! can run on a worker thread while the check-in loop is sleeping.

use std::sync::atomic::{AtomicU64, Ordering};
use std::time::Duration;

use crate::config;

static SLEEP_SECS: AtomicU64 = AtomicU64::new(10);
static JITTER_SECS: AtomicU64 = AtomicU64::new(5);

pub fn init() {
    SLEEP_SECS.store(config::checkin_sleep(), Ordering::Relaxed);
    JITTER_SECS.store(config::checkin_jitter(), Ordering::Relaxed);
}

pub fn get() -> (u64, u64) {
    (
        SLEEP_SECS.load(Ordering::Relaxed),
        JITTER_SECS.load(Ordering::Relaxed),
    )
}

/// Returns an error instead of applying a nonsensical value, so the operator
/// gets the reason back in the task result.
pub fn set(sleep_secs: u64, jitter_secs: u64) -> Result<(), String> {
    if sleep_secs == 0 {
        return Err("sleep time must be greater than zero".into());
    }
    if jitter_secs > sleep_secs {
        return Err("jitter cannot be greater than the sleep time".into());
    }

    SLEEP_SECS.store(sleep_secs, Ordering::Relaxed);
    JITTER_SECS.store(jitter_secs, Ordering::Relaxed);
    Ok(())
}

/// Sleeps for the configured interval plus uniform jitter.
///
/// A jitter of 0 must produce exactly the base interval. Using the full
/// `[-jitter, +jitter]` range would halve the effective sleep, which is
/// surprising when jitter is 0 and impossible when jitter equals the sleep.
pub fn do_sleep() {
    let (sleep_secs, jitter_secs) = get();

    let base_ms = sleep_secs.saturating_mul(1000) as i64;
    let jitter_ms = jitter_secs.saturating_mul(1000) as i64;

    let offset = if jitter_ms == 0 {
        0
    } else {
        rand_range(-jitter_ms..=jitter_ms)
    };

    let total = (base_ms + offset).max(0) as u64;

    crate::log::log_lazy(|| {
        format!("sleeping {sleep_secs}s + {offset}ms jitter ({total}ms total)")
    });

    std::thread::sleep(Duration::from_millis(total));
}

/// Uniform value in `range` from a small xorshift64 generator.
///
/// Deliberately not seeded from a CSPRNG and not used for anything security
/// relevant - this only spreads check-in times so a fleet does not beat in
/// lockstep. Kept in-crate to avoid pulling in `rand` for one call.
fn rand_range(range: std::ops::RangeInclusive<i64>) -> i64 {
    use std::cell::Cell;
    use std::time::{SystemTime, UNIX_EPOCH};

    thread_local! {
        static STATE: Cell<u64> = const { Cell::new(0) };
    }

    let value = STATE.with(|state| {
        let mut x = state.get();
        if x == 0 {
            let nanos = SystemTime::now()
                .duration_since(UNIX_EPOCH)
                .map(|d| d.as_nanos() as u64)
                .unwrap_or(0x2545_F491_4F6C_DD1D);
            x = nanos ^ (std::process::id() as u64).wrapping_mul(0x9E37_79B9_7F4A_7C15);
            if x == 0 {
                x = 0x2545_F491_4F6C_DD1D;
            }
        }

        // xorshift64
        x ^= x << 13;
        x ^= x >> 7;
        x ^= x << 17;
        state.set(x);
        x
    });

    let span = range.end() - range.start() + 1;
    if span <= 0 {
        return *range.start();
    }

    range.start() + (value % span as u64) as i64
}
