//! The `<digest16>` of `docs/web/URLS.md` §2: a `u64`, sixteen lowercase hex
//! digits, zero-padded.
//!
//! Three of the numbers this service stores are `u64` digests — the physics
//! digest, a map's collision digest, and a run's rolling digest. Postgres has
//! no unsigned 64-bit integer, so each is stored in a `bigint` as the
//! two's-complement reinterpretation of the same bits. That is lossless and it
//! is round-tripped by [`to_sql`] and [`from_sql`]; it is never *rendered* as a
//! signed decimal anywhere, because the only spelling a person or a URL ever
//! sees is the hex one.
//!
//! Parsing is strict about case on purpose. URLS.md §2: "A uppercase digest is
//! a 404, not a redirect — two spellings of one record is how a cache ends up
//! holding two copies of the same page."

/// Format a digest the way a URL and every JSON field spell it.
#[must_use]
pub fn format(value: u64) -> String {
    format!("{value:016x}")
}

/// Parse a `<digest16>`: exactly sixteen lowercase hex digits, nothing else.
///
/// Returns `None` for the wrong length, for uppercase, for `0x` prefixes and
/// for anything with a sign — all of which are a 404 rather than a redirect.
#[must_use]
pub fn parse(text: &str) -> Option<u64> {
    if text.len() != 16 {
        return None;
    }
    if !text
        .bytes()
        .all(|b| b.is_ascii_digit() || (b'a'..=b'f').contains(&b))
    {
        return None;
    }
    u64::from_str_radix(text, 16).ok()
}

/// The `bigint` a digest is stored in.
#[must_use]
pub const fn to_sql(value: u64) -> i64 {
    value as i64
}

/// The digest a `bigint` holds.
#[must_use]
pub const fn from_sql(value: i64) -> u64 {
    value as u64
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn round_trips_through_the_bigint_column() {
        for value in [0, 1, u64::MAX, u64::MAX / 2, 0x8000_0000_0000_0000, 0x0123_4567_89ab_cdef] {
            assert_eq!(from_sql(to_sql(value)), value, "{value:#018x}");
        }
    }

    #[test]
    fn round_trips_through_the_url_spelling() {
        for value in [0, 1, u64::MAX, 0x0123_4567_89ab_cdef] {
            assert_eq!(parse(&format(value)), Some(value));
        }
        assert_eq!(format(0), "0000000000000000");
        assert_eq!(format(u64::MAX), "ffffffffffffffff");
    }

    #[test]
    fn an_uppercase_digest_is_not_a_digest() {
        // URLS.md §2. A 404, not a redirect: the site must never end up with
        // two spellings of one record.
        assert_eq!(parse("0123456789ABCDEF"), None);
        assert_eq!(parse("0123456789abcdeF"), None);
    }

    #[test]
    fn nothing_else_parses() {
        assert_eq!(parse(""), None);
        assert_eq!(parse("0123456789abcde"), None, "fifteen digits");
        assert_eq!(parse("0123456789abcdef0"), None, "seventeen digits");
        assert_eq!(parse("0x0123456789abcd"), None);
        assert_eq!(parse("-123456789abcdef"), None);
        assert_eq!(parse("  123456789abcdef"), None);
    }
}
