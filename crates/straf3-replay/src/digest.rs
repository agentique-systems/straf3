//! The two digests a `.s3d` carries, and the one fold both are built from.
//!
//! # Why FNV-1a, again, spelled out again
//!
//! This is the fourth copy of these eight lines in the tree —
//! [`SimState::checksum`] folds the simulation state with them,
//! `straf3_map::digest` folds compiled geometry with them, `xtask` re-derives
//! the determinism report's digests with them, and now this. Each copy is
//! deliberate and each says so, for the same reason `xtask`'s does: the value
//! of a check that re-computes a number is that it re-computes it *from the
//! other side*. A shared helper cannot disagree with itself, so it cannot
//! demonstrate agreement.
//!
//! The constants are pinned by a known-answer test at the bottom of this file
//! rather than by importing them, so a drift between this crate and
//! `straf3-det-runner` surfaces as a failing test here instead of as two
//! digests that quietly mean different things.
//!
//! FNV-1a is chosen for the same three properties everywhere it appears:
//! order-dependent, allocation-free, and identical on every platform for the
//! same input bits. It is an equality check, not a security primitive — a
//! `.s3d` is evidence against accident and against staleness, not against a
//! determined forger. See the crate docs.
//!
//! # The two digests
//!
//! - The **run digest** ([`fold`] over every command's
//!   [`SimState::checksum`], in order). This is the number that says two
//!   machines simulated the same run, and it is the same scheme, seeded the
//!   same way, that criterion 2's cross-target check folds. It is *sticky*:
//!   once two runs disagree on any command, the digest differs for every
//!   command after it and can never re-converge, which is why comparing one
//!   number is equivalent to comparing all of them.
//! - The **content digest** ([`Fnv1a`] over the file's bytes). This one has
//!   nothing to do with physics; it catches a truncated or corrupted file at
//!   load, before anything tries to read a command count out of noise.
//!
//! [`SimState::checksum`]: straf3_sim::SimState::checksum

/// FNV-1a's 64-bit offset basis. The seed of both digests.
pub const FNV_OFFSET: u64 = 0xcbf2_9ce4_8422_2325;

/// FNV-1a's 64-bit prime.
pub const FNV_PRIME: u64 = 0x0000_0100_0000_01b3;

/// Fold one 64-bit value into a rolling digest, little-endian.
///
/// Byte-for-byte the same function as `straf3_det_runner::fold`. A recording's
/// digest and a determinism report's digest are therefore the same kind of
/// number, computed the same way, and a run whose command stream matches the
/// reference stream would fold to the same value.
#[must_use]
pub const fn fold(mut digest: u64, value: u64) -> u64 {
    let bytes = value.to_le_bytes();
    let mut i = 0;
    while i < bytes.len() {
        digest ^= bytes[i] as u64;
        digest = digest.wrapping_mul(FNV_PRIME);
        i += 1;
    }
    digest
}

/// Fold a whole sequence of per-command checksums, seeded at [`FNV_OFFSET`].
///
/// This is the definition of a recording's run digest, and the function
/// [`crate::Recording::from_bytes`] re-runs over a stored checksum trace to
/// check that the digest in the header was actually derived from it —
/// criterion 2's anti-tamper rule, applied to a saved run rather than to a
/// report.
#[must_use]
pub fn fold_all(values: impl IntoIterator<Item = u64>) -> u64 {
    values.into_iter().fold(FNV_OFFSET, fold)
}

/// A byte-wise FNV-1a fold in progress — the content digest.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Fnv1a(u64);

impl Fnv1a {
    /// A fold that has seen nothing.
    #[must_use]
    pub const fn new() -> Self {
        Self(FNV_OFFSET)
    }

    /// Fold a byte string.
    pub const fn bytes(&mut self, bytes: &[u8]) {
        let mut i = 0;
        while i < bytes.len() {
            self.0 ^= bytes[i] as u64;
            self.0 = self.0.wrapping_mul(FNV_PRIME);
            i += 1;
        }
    }

    /// The digest so far.
    #[must_use]
    pub const fn finish(self) -> u64 {
        self.0
    }
}

impl Default for Fnv1a {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The constants, pinned by value rather than by import.
    ///
    /// If `straf3-det-runner`, `straf3-map` or `SimState::checksum` ever moves
    /// off these, this test still passes and theirs changes — which is the
    /// point: the drift becomes a visible disagreement between two recorded
    /// numbers instead of an invisible redefinition of one.
    #[test]
    fn the_fold_is_the_one_the_determinism_check_uses() {
        // Known answers, computed from the FNV-1a definition: eight
        // little-endian bytes of the value, each xored then multiplied.
        assert_eq!(fold(FNV_OFFSET, 0), 0xa8c7_f832_281a_39c5);
        assert_eq!(fold_all([]), FNV_OFFSET);
        assert_eq!(fold_all([0u64]), fold(FNV_OFFSET, 0));
        assert_eq!(
            fold_all([1u64, 2, 3]),
            fold(fold(fold(FNV_OFFSET, 1), 2), 3)
        );
    }

    #[test]
    fn a_digest_cannot_re_converge_after_a_disagreement() {
        // The property the whole format rests on. Two runs that differ on one
        // command and agree on every other one still have different digests,
        // so a recording cannot claim a run it did not perform by ending in
        // the right place.
        let a = [1u64, 2, 3, 4, 5];
        let b = [1u64, 2, 99, 4, 5];
        assert_ne!(fold_all(a), fold_all(b));
        assert_eq!(a.last(), b.last());
    }

    #[test]
    fn the_content_digest_sees_a_single_flipped_byte() {
        let mut a = Fnv1a::new();
        a.bytes(b"S3DR\x01\x00\x00\x00");
        let mut b = Fnv1a::new();
        b.bytes(b"S3DR\x02\x00\x00\x00");
        assert_ne!(a.finish(), b.finish());
        assert_ne!(a.finish(), Fnv1a::new().finish());
    }

    #[test]
    fn the_content_digest_is_length_sensitive() {
        // A truncation that happens to end on a zero byte is still a
        // truncation.
        let mut a = Fnv1a::new();
        a.bytes(&[0, 0, 0]);
        let mut b = Fnv1a::new();
        b.bytes(&[0, 0, 0, 0]);
        assert_ne!(a.finish(), b.finish());
    }
}
