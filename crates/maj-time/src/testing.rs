//! Deterministic test utilities.
//!
//! # Why not a property-testing framework
//!
//! Majestat's claim is that the same inputs always produce the same outputs. A test
//! harness that samples randomly from an unseeded source contradicts that claim:
//! a failure would not be reproducible from the report alone. `maj-time` therefore
//! uses a **seeded, fixed-sequence PRNG**, so every property test in this workspace
//! replays the identical case sequence on every machine and every run.
//!
//! The host toolchain for M0/M1 has no C compiler and no binutils `dlltool`, which
//! also rules out `proptest` (it pulls `windows-sys`) and `getrandom`. See
//! `docs/adr/0009-toolchain-and-dependency-policy.md`.
//!
//! ```
//! use maj_time::testing::DeterministicRng;
//! let mut rng = DeterministicRng::new(0x4D41_4A);
//! let first: Vec<i64> = (0..3).map(|_| rng.next_i64_in(-5, 5)).collect();
//! let mut replay = DeterministicRng::new(0x4D41_4A);
//! let second: Vec<i64> = (0..3).map(|_| replay.next_i64_in(-5, 5)).collect();
//! assert_eq!(first, second, "the same seed must replay the same sequence");
//! ```

/// A `xorshift64*` generator: tiny, dependency-free and exactly reproducible.
///
/// Not cryptographically secure, and not intended to be. It exists to drive
/// deterministic test cases.
#[derive(Debug, Clone)]
pub struct DeterministicRng {
    state: u64,
}

/// Seed used when the caller passes zero, because `xorshift64*` cannot leave zero.
const NON_ZERO_FALLBACK: u64 = 0x9E37_79B9_7F4A_7C15;

impl DeterministicRng {
    /// Creates a generator from a seed.
    ///
    /// A seed of `0` is replaced by a fixed non-zero constant so the sequence never
    /// collapses.
    #[must_use]
    pub const fn new(seed: u64) -> Self {
        Self {
            state: if seed == 0 { NON_ZERO_FALLBACK } else { seed },
        }
    }

    /// Produces the next raw 64-bit value.
    pub fn next_u64(&mut self) -> u64 {
        let mut x = self.state;
        x ^= x >> 12;
        x ^= x << 25;
        x ^= x >> 27;
        self.state = x;
        x.wrapping_mul(0x2545_F491_4F6C_DD1D)
    }

    /// Produces the next value in `[lo, hi)`.
    ///
    /// # Panics
    ///
    /// Panics if `hi <= lo`.
    pub fn next_i64_in(&mut self, lo: i64, hi: i64) -> i64 {
        assert!(hi > lo, "empty range for next_i64_in: [{lo}, {hi})");
        let span = (i128::from(hi) - i128::from(lo)) as u128;
        let offset = u128::from(self.next_u64()) % span;
        i64::try_from(i128::from(lo) + offset as i128).unwrap_or(lo)
    }

    /// Produces the next value in `[lo, hi)`.
    ///
    /// # Panics
    ///
    /// Panics if `hi <= lo`.
    pub fn next_u32_in(&mut self, lo: u32, hi: u32) -> u32 {
        assert!(hi > lo, "empty range for next_u32_in: [{lo}, {hi})");
        let span = u64::from(hi - lo);
        lo + u32::try_from(self.next_u64() % span).unwrap_or(0)
    }

    /// Produces any `i64`.
    pub fn next_i64(&mut self) -> i64 {
        self.next_u64() as i64
    }

    /// Produces a coin flip.
    pub fn next_bool(&mut self) -> bool {
        self.next_u64() & 1 == 1
    }

    /// Picks one element from a non-empty slice.
    ///
    /// # Panics
    ///
    /// Panics if `items` is empty.
    pub fn pick<'a, T>(&mut self, items: &'a [T]) -> &'a T {
        assert!(!items.is_empty(), "cannot pick from an empty slice");
        let index = usize::try_from(u128::from(self.next_u64()) % items.len() as u128).unwrap_or(0);
        &items[index]
    }

    /// The current internal state, useful for reproducing a failure by hand.
    #[must_use]
    pub const fn state(&self) -> u64 {
        self.state
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_same_seed_replays_the_same_sequence() {
        let mut a = DeterministicRng::new(42);
        let mut b = DeterministicRng::new(42);
        for _ in 0..64 {
            assert_eq!(a.next_u64(), b.next_u64());
        }
    }

    #[test]
    fn different_seeds_diverge() {
        let mut a = DeterministicRng::new(1);
        let mut b = DeterministicRng::new(2);
        let left: Vec<u64> = (0..8).map(|_| a.next_u64()).collect();
        let right: Vec<u64> = (0..8).map(|_| b.next_u64()).collect();
        assert_ne!(left, right);
    }

    #[test]
    fn a_zero_seed_does_not_collapse() {
        let mut rng = DeterministicRng::new(0);
        let values: Vec<u64> = (0..8).map(|_| rng.next_u64()).collect();
        assert!(values.iter().any(|v| *v != 0));
    }

    #[test]
    fn ranged_values_stay_inside_the_range() {
        let mut rng = DeterministicRng::new(7);
        for _ in 0..1_000 {
            let v = rng.next_i64_in(-10, 10);
            assert!((-10..10).contains(&v), "{v} escaped [-10, 10)");
        }
    }

    #[test]
    fn pick_returns_a_member() {
        let mut rng = DeterministicRng::new(11);
        let items = ["a", "b", "c"];
        for _ in 0..50 {
            assert!(items.contains(rng.pick(&items)));
        }
    }
}
