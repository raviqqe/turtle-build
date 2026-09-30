use core::hash::Hasher;
use rapidhash::v3::{DEFAULT_RAPID_SECRETS, RapidStreamHasherV3};

// Hashes stored in databases must not depend on the unspecified hash algorithm
// of the standard library.
pub struct StableHasher(RapidStreamHasherV3<'static>);

impl Default for StableHasher {
    fn default() -> Self {
        Self(RapidStreamHasherV3::new(&DEFAULT_RAPID_SECRETS))
    }
}

impl Hasher for StableHasher {
    fn write(&mut self, bytes: &[u8]) {
        self.0.write(bytes);
    }

    fn finish(&self) -> u64 {
        self.0.finish()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use pretty_assertions::{assert_eq, assert_ne};

    #[test]
    fn hash_nothing() {
        assert_eq!(StableHasher::default().finish(), 0x0338_dc4b_e2ce_cdae);
    }

    #[test]
    fn hash_bytes() {
        let mut hasher = StableHasher::default();

        hasher.write(b"foo");

        assert_eq!(hasher.finish(), 0x20c0_f952_e291_56a1);
    }

    #[test]
    fn hash_bytes_in_chunks() {
        let mut hasher = StableHasher::default();

        hasher.write(b"foo");

        let mut other = StableHasher::default();

        other.write(b"fo");
        other.write(b"o");

        assert_eq!(hasher.finish(), other.finish());
    }

    #[test]
    fn hash_different_bytes() {
        let mut hasher = StableHasher::default();

        hasher.write(b"foo");

        let mut other = StableHasher::default();

        other.write(b"bar");

        assert_ne!(hasher.finish(), other.finish());
    }
}
