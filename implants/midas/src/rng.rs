//! Minimal xorshift for the animated overlays.
//!
//! Shared by both rendering backends so `static`, `glitch` and the matrix rain
//! look the same whichever one is drawing. Only cosmetic, so there is no reason
//! to reach for a dependency.

pub struct Rng(u64);

impl Rng {
    pub fn new(seed: u64) -> Rng {
        // A zero state is absorbing, so it is forced away from it.
        Rng(seed | 1)
    }

    fn next(&mut self) -> u64 {
        self.0 ^= self.0 << 13;
        self.0 ^= self.0 >> 7;
        self.0 ^= self.0 << 17;
        self.0
    }

    /// Uniform in `[0, 1)`, from the top 24 bits.
    pub fn f32(&mut self) -> f32 {
        (self.next() >> 40) as f32 / 16_777_216.0
    }

    /// Uniform in `[low, high)` as a float.
    pub fn range(&mut self, low: f32, high: f32) -> f32 {
        low + self.f32() * (high - low)
    }

    /// Uniform in `[low, high)` as an integer.
    ///
    /// The high bound is exclusive, which is what every caller here wants: a
    /// rect of height `0` is not something to hand the rasteriser.
    pub fn range_i32(&mut self, low: i32, high: i32) -> i32 {
        if high <= low {
            return low;
        }
        low + (self.next() % (high - low) as u64) as i32
    }
}
