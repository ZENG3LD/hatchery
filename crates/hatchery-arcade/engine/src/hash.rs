//! Stable, cross-version, cross-process hash for replay verification and
//! the headless harness's own determinism checks.
//!
//! [`StableHasher`] is FNV-1a over an explicit byte sequence a
//! [`crate::game::MiniGame`] writes itself, field by field, in a fixed
//! order. NEVER `#[derive(Hash)]` fed into
//! `std::collections::hash_map::DefaultHasher` -- std's own docs state
//! `DefaultHasher`'s algorithm is unspecified and MAY change between Rust
//! versions, which would silently break every stored
//! [`crate::replay::ReplayV1`]'s `final_hash` the next time the toolchain
//! is upgraded. A `MiniGame` state holding a `HashMap`/`HashSet` must
//! convert to `BTreeMap`/`BTreeSet` (or sort explicitly) before feeding
//! this -- `HashMap`'s default `RandomState` hasher is ALSO
//! per-process-random by default, making naive iteration-order hashing
//! non-reproducible even within one Rust version.

/// FNV-1a accumulator over canonical bytes.
pub struct StableHasher(u64);

const FNV_OFFSET_BASIS: u64 = 0xcbf2_9ce4_8422_2325;
const FNV_PRIME: u64 = 0x0000_0100_0000_01b3;

impl StableHasher {
    pub fn new() -> Self {
        Self(FNV_OFFSET_BASIS)
    }

    pub fn write(&mut self, bytes: &[u8]) {
        for b in bytes {
            self.0 ^= *b as u64;
            self.0 = self.0.wrapping_mul(FNV_PRIME);
        }
    }

    pub fn write_u64(&mut self, v: u64) {
        self.write(&v.to_le_bytes());
    }

    pub fn write_i64(&mut self, v: i64) {
        self.write(&v.to_le_bytes());
    }

    pub fn finish(&self) -> u64 {
        self.0
    }
}

impl Default for StableHasher {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn identical_writes_produce_identical_hashes() {
        let mut a = StableHasher::new();
        a.write_u64(1);
        a.write_i64(-7);
        a.write(b"tag");

        let mut b = StableHasher::new();
        b.write_u64(1);
        b.write_i64(-7);
        b.write(b"tag");

        assert_eq!(a.finish(), b.finish());
    }

    #[test]
    fn a_single_differing_value_changes_the_hash() {
        let mut a = StableHasher::new();
        a.write_u64(1);

        let mut b = StableHasher::new();
        b.write_u64(2);

        assert_ne!(a.finish(), b.finish());
    }
}
