//! A decoded sound, and the random numbers that choose between them.
//!
//! **This was `synth`, and the synthesiser is gone.** Every sound effect
//! used to be a recipe -- noise through resonances, run three times with
//! three seeds -- and the recordings in [`super::recorded`] were laid over
//! them one by one. The player's order was to make every sound a real
//! recording under CC0 and to delete what was generated, so the filters,
//! envelopes, modes and grains went with the recipes. What is left is what
//! the recordings still need: the clip they decode into, and the generator
//! the mixer and the soundscape use to pick a variant and jitter a pitch.
//!
//! The music (`music.rs`) is still composed at run time and uses [`Rng`]
//! from here; it synthesises its own voices and never touched the recipe
//! building blocks.

/// A finished sound: mono, 16-bit, at the device's sample rate.
///
/// 16-bit rather than `f32`, and mono rather than stereo, because four
/// hundred clips live in memory for the whole session and halving them
/// costs nothing anybody can hear. It is also exactly what `hound` writes,
/// so `--export-sounds` and the played sound are the same bytes. Position
/// and panning happen at playback, where the listener is, so a stereo clip
/// would be a picture of a stereo field the mixer is about to overwrite.
#[derive(Debug, Clone)]
pub struct Clip {
    pub samples: Vec<i16>,
    pub sample_rate: u32,
}

impl Clip {
    pub fn len(&self) -> usize {
        self.samples.len()
    }

    /// How long it lasts. Printed at startup and held under a limit by the
    /// tests, so a recording cannot quietly become a ten-second one.
    pub fn seconds(&self) -> f32 {
        self.samples.len() as f32 / self.sample_rate.max(1) as f32
    }

    /// Resamples to `rate`, linearly.
    ///
    /// Every recording is stored at the rate it was cut at (24 kHz for the
    /// newer ones, 44.1 or 48 for the older) and the device runs at
    /// whatever it runs at, so this runs once per file at startup. Linear
    /// interpolation is audibly imperfect on a pure tone and invisible on
    /// foley, rain and animals, which is all there is.
    pub fn resampled(&self, rate: u32) -> Clip {
        if rate == self.sample_rate || self.samples.is_empty() {
            return self.clone();
        }
        let ratio = self.sample_rate as f64 / rate as f64;
        let out_len = ((self.samples.len() as f64) / ratio).round().max(1.0) as usize;
        let mut samples = Vec::with_capacity(out_len);
        for i in 0..out_len {
            let at = i as f64 * ratio;
            let index = at as usize;
            let frac = (at - index as f64) as f32;
            let a = self.samples.get(index).copied().unwrap_or(0) as f32;
            let b = self.samples.get(index + 1).copied().unwrap_or(0) as f32;
            samples.push((a + (b - a) * frac) as i16);
        }
        Clip { samples, sample_rate: rate }
    }
}

/// The random number generator: which variant plays, how much a pitch is
/// jittered, where a bee is heard from.
///
/// xorshift64*, which is four instructions. `rand` would be a dependency
/// carried into the binary for this and nothing else. The music uses one
/// too, on the audio thread, where allocating or locking is not allowed
/// and a generator with eight bytes of state is the only kind welcome.
pub struct Rng(u64);

impl Rng {
    pub fn new(seed: u64) -> Self {
        // Any non-zero state will do; xorshift is dead at zero.
        Rng(seed.wrapping_mul(0x9E37_79B9_7F4A_7C15) | 1)
    }

    #[inline]
    pub fn next_u64(&mut self) -> u64 {
        let mut x = self.0;
        x ^= x >> 12;
        x ^= x << 25;
        x ^= x >> 27;
        self.0 = x;
        x.wrapping_mul(0x2545_F491_4F6C_DD1D)
    }

    /// Uniform in 0..1.
    #[inline]
    pub fn unit(&mut self) -> f32 {
        (self.next_u64() >> 40) as f32 / (1u32 << 24) as f32
    }

    /// Uniform in -1..1.
    #[inline]
    pub fn bipolar(&mut self) -> f32 {
        self.unit() * 2.0 - 1.0
    }

    /// Uniform in `low..high`.
    #[inline]
    pub fn range(&mut self, low: f32, high: f32) -> f32 {
        low + (high - low) * self.unit()
    }

    /// One of `count`, uniformly. Zero if asked for nothing, so a caller
    /// indexing an empty table gets an index rather than a panic.
    #[inline]
    pub fn below(&mut self, count: usize) -> usize {
        if count == 0 {
            0
        } else {
            (self.next_u64() % count as u64) as usize
        }
    }

    /// True with probability `chance`.
    #[inline]
    pub fn chance(&mut self, chance: f32) -> bool {
        self.unit() < chance
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn noise(seed: u64, len: usize) -> Vec<f32> {
        let mut rng = Rng::new(seed);
        (0..len).map(|_| rng.bipolar()).collect()
    }

    #[test]
    fn the_same_seed_gives_the_same_numbers_and_a_different_one_does_not() {
        assert_eq!(noise(7, 400), noise(7, 400), "a seeded pick has to be reproducible");
        assert_ne!(noise(7, 400), noise(8, 400));
    }

    #[test]
    fn resampling_keeps_the_duration() {
        let samples = noise(5, 11_025).into_iter().map(|v| (v * 16_000.0) as i16).collect();
        let clip = Clip { samples, sample_rate: 44_100 };
        let up = clip.resampled(48_000);
        assert!((up.seconds() - clip.seconds()).abs() < 0.001);
        let down = clip.resampled(22_050);
        assert!((down.seconds() - clip.seconds()).abs() < 0.001);
    }
}
