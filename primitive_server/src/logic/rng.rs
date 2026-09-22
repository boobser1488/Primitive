//! A random number generator, and the argument for writing one.
//!
//! ## Why not a crate
//!
//! `rand` is the obvious answer and it is a dozen transitive
//! dependencies for what three of the mechanics here actually need:
//! "pick one of these", "roll a chance", "a number in this range". None
//! of them is cryptography, none of them is a simulation whose results
//! anyone will publish, and none of them is on a hot path -- an animal
//! decides where to walk about once a second.
//!
//! ## Why not the world generator's hash
//!
//! `worldgen::hash2` is deterministic *by position*, which is exactly
//! what terrain needs and exactly wrong here: two deer standing in the
//! same cell would make the same decision forever, and a bush picked
//! twice would regrow at the same instant both times. What these want is
//! a stream, not a field.
//!
//! ## What this is
//!
//! xorshift64*, which is sixty-four bits of state, three shifts and a
//! multiply. It passes the statistical tests anybody would apply to a
//! game's wandering monsters and it is under twenty lines. The one
//! property worth stating: seeded from the clock, so two servers started
//! in the same second do not produce identical weather -- and seedable
//! explicitly, so a test can.

/// A stream of pseudorandom numbers.
///
/// `Copy` deliberately *not* derived: a copy of a generator is a second
/// stream that produces exactly the same numbers, which is a bug that
/// looks like bad luck.
#[derive(Debug, Clone)]
pub struct Rng {
    state: u64,
}

impl Rng {
    /// A generator seeded from the clock.
    ///
    /// Every mechanic that wants one calls this at startup, and they all
    /// land on different states because the nanoseconds differ -- the
    /// counter is there for the case where they do not, which on a
    /// coarse clock is every one of them starting in the same tick.
    pub fn from_clock() -> Self {
        use std::sync::atomic::{AtomicU64, Ordering};
        static COUNTER: AtomicU64 = AtomicU64::new(0);
        let nanos = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map(|d| d.as_nanos() as u64)
            .unwrap_or(0x2545_F491_4F6C_DD1D);
        Self::seeded(nanos ^ COUNTER.fetch_add(0x9E37_79B9_7F4A_7C15, Ordering::Relaxed))
    }

    /// A generator with a stated seed, for tests and for anything that
    /// wants to be repeatable.
    pub fn seeded(seed: u64) -> Self {
        // Zero is the one state xorshift cannot leave, so it is the one
        // seed that must not be taken at face value.
        Self {
            state: if seed == 0 { 0x9E37_79B9_7F4A_7C15 } else { seed },
        }
    }

    /// The next sixty-four bits.
    #[inline]
    pub fn next_u64(&mut self) -> u64 {
        let mut x = self.state;
        x ^= x >> 12;
        x ^= x << 25;
        x ^= x >> 27;
        self.state = x;
        x.wrapping_mul(0x2545_F491_4F6C_DD1D)
    }

    /// A float in 0..1.
    #[inline]
    pub fn next_f32(&mut self) -> f32 {
        // Top 24 bits, which is exactly the mantissa of an f32: taking
        // the low bits of an xorshift is the one way to get a visibly
        // bad sequence out of it.
        (self.next_u64() >> 40) as f32 / (1u32 << 24) as f32
    }

    /// A float in `low..high`. Backwards ranges give `low`, rather than
    /// a value outside both ends.
    #[inline]
    pub fn range(&mut self, low: f32, high: f32) -> f32 {
        if high <= low {
            return low;
        }
        low + self.next_f32() * (high - low)
    }

    /// An integer in `0..n`. Zero for an empty range, so a caller that
    /// indexes with it does not panic on an empty list.
    #[inline]
    pub fn below(&mut self, n: u32) -> u32 {
        if n == 0 {
            return 0;
        }
        (self.next_u64() % n as u64) as u32
    }

    /// True with probability `chance`, clamped to 0..1.
    #[inline]
    pub fn chance(&mut self, chance: f32) -> bool {
        self.next_f32() < chance.clamp(0.0, 1.0)
    }

    /// One of these, or `None` if there are none.
    pub fn pick<'a, T>(&mut self, items: &'a [T]) -> Option<&'a T> {
        if items.is_empty() {
            return None;
        }
        items.get(self.below(items.len() as u32) as usize)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_same_seed_is_the_same_stream() {
        let mut a = Rng::seeded(12345);
        let mut b = Rng::seeded(12345);
        for _ in 0..100 {
            assert_eq!(a.next_u64(), b.next_u64());
        }
    }

    #[test]
    fn different_seeds_are_different_streams() {
        let mut a = Rng::seeded(1);
        let mut b = Rng::seeded(2);
        let differ = (0..20).filter(|_| a.next_u64() != b.next_u64()).count();
        assert!(differ > 15, "two seeds produced the same numbers {} times", 20 - differ);
    }

    #[test]
    fn a_zero_seed_still_produces_numbers() {
        // The one state xorshift cannot leave. Without the guard this is
        // a generator that returns zero forever, which reads as a
        // mechanic that has stopped working rather than as a bad seed.
        let mut rng = Rng::seeded(0);
        assert!((0..10).any(|_| rng.next_u64() != 0));
    }

    #[test]
    fn floats_stay_inside_the_unit_interval() {
        let mut rng = Rng::seeded(7);
        for _ in 0..10_000 {
            let v = rng.next_f32();
            assert!((0.0..1.0).contains(&v), "{v} is outside 0..1");
        }
    }

    #[test]
    fn floats_are_spread_rather_than_clustered() {
        // The cheap check that would catch taking the low bits: count
        // how many land in each tenth of the range.
        let mut rng = Rng::seeded(99);
        let mut buckets = [0usize; 10];
        for _ in 0..10_000 {
            buckets[(rng.next_f32() * 10.0) as usize % 10] += 1;
        }
        for (i, &count) in buckets.iter().enumerate() {
            assert!(
                (700..1300).contains(&count),
                "bucket {i} got {count} of 10000, which is not a flat distribution"
            );
        }
    }

    #[test]
    fn a_range_never_leaves_its_ends() {
        let mut rng = Rng::seeded(3);
        for _ in 0..1000 {
            let v = rng.range(-4.0, 9.5);
            assert!((-4.0..9.5).contains(&v));
        }
        // A backwards range is a caller's mistake, and the answer is one
        // of the two ends rather than something outside both.
        assert_eq!(rng.range(5.0, 1.0), 5.0);
        assert_eq!(rng.range(2.0, 2.0), 2.0);
    }

    #[test]
    fn below_stays_below_and_covers_the_range() {
        let mut rng = Rng::seeded(11);
        let mut seen = [false; 6];
        for _ in 0..1000 {
            let v = rng.below(6);
            assert!(v < 6);
            seen[v as usize] = true;
        }
        assert!(seen.iter().all(|&s| s), "some values never came up");
        assert_eq!(rng.below(0), 0, "an empty range must not panic the caller");
    }

    #[test]
    fn a_chance_is_roughly_the_chance_it_says() {
        let mut rng = Rng::seeded(2024);
        let hits = (0..10_000).filter(|_| rng.chance(0.25)).count();
        assert!((2200..2800).contains(&hits), "a quarter came up {hits} times in 10000");
        assert!(!rng.chance(0.0));
        assert!(rng.chance(1.0));
        // Nonsense probabilities are clamped rather than believed.
        assert!(!rng.chance(-3.0));
        assert!(rng.chance(9.0));
    }

    #[test]
    fn picking_from_nothing_is_nothing() {
        let mut rng = Rng::seeded(5);
        let empty: [u8; 0] = [];
        assert!(rng.pick(&empty).is_none());
        assert_eq!(rng.pick(&[7]), Some(&7));
    }

    #[test]
    fn two_generators_from_the_clock_are_not_the_same_one() {
        // Two mechanics that started in the same tick used to be the
        // problem this counter exists for: identical weather and
        // identical wandering, on every restart.
        let mut a = Rng::from_clock();
        let mut b = Rng::from_clock();
        assert_ne!(a.next_u64(), b.next_u64());
    }
}
