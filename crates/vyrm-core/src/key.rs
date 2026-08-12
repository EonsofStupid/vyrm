//! Claim key encoding.
//!
//! ```text
//! c/{subject}\x00{predicate}\x00{inv_valid_from:020}\x00{inv_tx_time:020}
//!
//! inv(t) = u64::MAX - t
//! ```
//!
//! Both timestamps are encoded inverted, so the newest version sorts first under
//! a byte-lexicographic iterator and resolution requires one seek.
//!
//! Transaction time is part of the key by necessity, not for ordering. An
//! encoding over valid time alone gives two claims about the same valid-time
//! point the same key, so a later correction silently destroys the claim it
//! corrects while the sequence watermark still counts both. That contradicts the
//! retirement guarantee in `SPEC.md` §6 and is covered by
//! `vyrm-store/tests/bitemporal.rs`.
//!
//! Verified against Fjall 3.1.8 on 2026-08-09 (`SPEC.md` §6.1): prefix scan
//! returned newest-first ordering, all valid-time boundary cases resolved
//! correctly, and the adversarial neighbours `wp3x/status`, `wp3/statusx`, and
//! `wp/status` produced no leakage.

use crate::error::{Error, Result};
use crate::ident::{Predicate, Reader, Subject, SEP};

/// Namespace byte prefix for claim keys.
pub const CLAIM_NS: &[u8] = b"c/";

/// Width of the zero-padded inverted timestamp. `u64::MAX` is 20 digits, so a
/// narrower field would break lexicographic ordering.
const INV_WIDTH: usize = 20;

/// Invert a timestamp so that larger values sort earlier.
#[inline]
pub fn invert(valid_from: u64) -> u64 {
    u64::MAX - valid_from
}

/// Full key for one claim version.
///
/// Both timelines participate. Encoding valid time alone would cause a later
/// correction at the same `valid_from` to overwrite the claim it corrects, which
/// would contradict the retirement guarantee in `SPEC.md` §6. Within one
/// `valid_from`, inverted transaction time orders the most recently recorded
/// version first, so resolution still returns current knowledge by taking the
/// first candidate.
pub fn claim_key(
    subject: &Subject,
    predicate: &Predicate,
    valid_from: u64,
    tx_time: u64,
) -> Vec<u8> {
    let mut key = valid_from_bound(subject, predicate, valid_from);
    key.extend_from_slice(format!("{:0width$}", invert(tx_time), width = INV_WIDTH).as_bytes());
    key
}

/// Lower bound of every version sharing one `valid_from`.
///
/// Sorts before any full claim key with that `valid_from`, because a prefix
/// orders before any extension of itself.
fn valid_from_bound(subject: &Subject, predicate: &Predicate, valid_from: u64) -> Vec<u8> {
    let mut key = version_prefix(subject, predicate);
    key.extend_from_slice(format!("{:0width$}", invert(valid_from), width = INV_WIDTH).as_bytes());
    key.push(SEP);
    key
}

/// Prefix covering every version of one subject+predicate.
pub fn version_prefix(subject: &Subject, predicate: &Predicate) -> Vec<u8> {
    let mut key = Vec::with_capacity(CLAIM_NS.len() + subject.as_str().len() + predicate.as_str().len() + 2);
    key.extend_from_slice(CLAIM_NS);
    key.extend_from_slice(subject.as_str().as_bytes());
    key.push(SEP);
    key.extend_from_slice(predicate.as_str().as_bytes());
    key.push(SEP);
    key
}

/// Prefix covering every claim of one subject, across all predicates.
///
/// A scan over this prefix yields claims ordered by predicate, and within each
/// predicate newest first — the grouping recall relies on. One seek serves an
/// entire subject, which is what makes a subject-set recall a bounded number of
/// seeks rather than a scan of the store.
pub fn subject_prefix(subject: &Subject) -> Vec<u8> {
    let mut key = Vec::with_capacity(CLAIM_NS.len() + subject.as_str().len() + 1);
    key.extend_from_slice(CLAIM_NS);
    key.extend_from_slice(subject.as_str().as_bytes());
    key.push(SEP);
    key
}

/// Seek key for an as-of query. Range `seek_key(..as_of) .. prefix_end` and take
/// the first entry: that is the newest version with `valid_from <= as_of`.
pub fn seek_key(subject: &Subject, predicate: &Predicate, as_of: u64) -> Vec<u8> {
    valid_from_bound(subject, predicate, as_of)
}

/// Exclusive upper bound for a prefix scan.
///
/// Increments the last byte below `0xFF`, dropping trailing `0xFF` bytes.
/// Returns `None` when every byte is `0xFF`, indicating a scan unbounded above.
///
/// Claim prefixes always terminate in `SEP` (`0x00`), so the single-increment
/// path always applies at present. The general case is implemented regardless,
/// because a future key namespace relying on the simplified form would fail
/// silently.
pub fn prefix_end(prefix: &[u8]) -> Option<Vec<u8>> {
    let mut end = prefix.to_vec();
    while let Some(last) = end.last_mut() {
        if *last < 0xFF {
            *last += 1;
            return Some(end);
        }
        end.pop();
    }
    None
}

/// Key for one access record.
///
/// ```text
/// {at:020}\x00{reader}\x00{subject}\x00{predicate}
/// ```
///
/// Time leads so that an interval is a range scan, which is the access pattern
/// removal candidacy requires (`SPEC.md` §7).
///
/// Fields are separated by `SEP`, the one byte identifiers reject. A printable
/// separator such as `/` would be ambiguous, because identifiers may contain it:
/// subject `a/b` with predicate `c` would encode identically to subject `a` with
/// predicate `b/c`.
pub fn access_key(at: u64, reader: &Reader, subject: &Subject, predicate: &Predicate) -> Vec<u8> {
    let mut key = format!("{at:0width$}", width = INV_WIDTH).into_bytes();
    for field in [reader.as_str(), subject.as_str(), predicate.as_str()] {
        key.push(SEP);
        key.extend_from_slice(field.as_bytes());
    }
    key
}

/// Lower bound for an access-record scan starting at `at`.
pub fn access_bound(at: u64) -> Vec<u8> {
    format!("{at:0width$}", width = INV_WIDTH).into_bytes()
}

/// Decode an access record key into its fields.
pub fn parse_access_key(key: &[u8]) -> Result<(u64, Reader, Subject, Predicate)> {
    let mut parts = key.splitn(4, |b| *b == SEP);
    let at = parts
        .next()
        .ok_or(Error::MalformedKey { reason: "access key is empty" })?;
    let at = std::str::from_utf8(at)
        .map_err(|_| Error::MalformedKey { reason: "access timestamp is not UTF-8" })?
        .parse::<u64>()
        .map_err(|_| Error::MalformedKey { reason: "access timestamp is not numeric" })?;
    let mut field = |what: &'static str| -> Result<String> {
        let raw = parts.next().ok_or(Error::MalformedKey { reason: what })?;
        String::from_utf8(raw.to_vec())
            .map_err(|_| Error::MalformedKey { reason: "access key field is not UTF-8" })
    };
    let reader = Reader::new(field("access key is missing the reader")?)?;
    let subject = Subject::new(field("access key is missing the subject")?)?;
    let predicate = Predicate::new(field("access key is missing the predicate")?)?;
    Ok((at, reader, subject, predicate))
}

/// Decode the subject and predicate from a claim key.
///
/// Reads identifiers from the key directly, so enumerating the stored
/// subject-predicate pairs does not require deserializing every claim.
pub fn parse_claim_key(key: &[u8]) -> Result<(Subject, Predicate)> {
    let body = key
        .strip_prefix(CLAIM_NS)
        .ok_or(Error::MalformedKey { reason: "claim key lacks its namespace" })?;
    let mut parts = body.splitn(3, |b| *b == SEP);
    let subject = parts
        .next()
        .ok_or(Error::MalformedKey { reason: "claim key is missing the subject" })?;
    let predicate = parts
        .next()
        .ok_or(Error::MalformedKey { reason: "claim key is missing the predicate" })?;
    let subject = String::from_utf8(subject.to_vec())
        .map_err(|_| Error::MalformedKey { reason: "subject is not UTF-8" })?;
    let predicate = String::from_utf8(predicate.to_vec())
        .map_err(|_| Error::MalformedKey { reason: "predicate is not UTF-8" })?;
    Ok((Subject::new(subject)?, Predicate::new(predicate)?))
}

/// Key for one entry in the sequence index.
///
/// The index maps an append sequence to the claim key written at that sequence,
/// which is what makes a sequence range answerable (`SPEC.md` §8.2, §8.4).
/// Ordering is ascending, not inverted: a sequence range is scanned forward.
///
/// The index occupies its own keyspace, so no namespace prefix is required.
pub fn sequence_key(sequence: u64) -> Vec<u8> {
    format!("{sequence:0width$}", width = INV_WIDTH).into_bytes()
}

/// Decode a sequence index key.
pub fn sequence_of(key: &[u8]) -> Result<u64> {
    if key.len() != INV_WIDTH {
        return Err(Error::MalformedKey { reason: "sequence key has the wrong width" });
    }
    std::str::from_utf8(key)
        .map_err(|_| Error::MalformedKey { reason: "sequence key is not UTF-8" })?
        .parse::<u64>()
        .map_err(|_| Error::MalformedKey { reason: "sequence key is not numeric" })
}

/// Decode the `valid_from` and `tx_time` embedded in a claim key.
pub fn timestamps_of(key: &[u8]) -> Result<(u64, u64)> {
    // Trailing layout: {inv_valid_from:020}{SEP}{inv_tx_time:020}
    const TAIL: usize = INV_WIDTH * 2 + 1;
    if key.len() < TAIL {
        return Err(Error::MalformedKey { reason: "shorter than the timestamp fields" });
    }
    let tail = &key[key.len() - TAIL..];
    if tail[INV_WIDTH] != SEP {
        return Err(Error::MalformedKey { reason: "timestamp fields are not separated" });
    }
    let valid_from = parse_inverted(&tail[..INV_WIDTH])?;
    let tx_time = parse_inverted(&tail[INV_WIDTH + 1..])?;
    Ok((valid_from, tx_time))
}

fn parse_inverted(field: &[u8]) -> Result<u64> {
    let text = std::str::from_utf8(field)
        .map_err(|_| Error::MalformedKey { reason: "inverted timestamp is not UTF-8" })?;
    let inverted = text
        .parse::<u64>()
        .map_err(|_| Error::MalformedKey { reason: "inverted timestamp is not numeric" })?;
    Ok(invert(inverted))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn sp(s: &str, p: &str) -> (Subject, Predicate) {
        (Subject::new(s).unwrap(), Predicate::new(p).unwrap())
    }

    #[test]
    fn newest_sorts_first() {
        let (s, p) = sp("wp3", "status");
        let older = claim_key(&s, &p, 100, 100);
        let newer = claim_key(&s, &p, 300, 300);
        assert!(newer < older, "newer valid_from must sort before older");
    }

    #[test]
    fn inv_field_is_fixed_width() {
        let (s, p) = sp("wp3", "status");
        // A narrow field would make "9" sort after "10". Fixed width prevents it.
        assert_eq!(claim_key(&s, &p, 0, 0).len(), claim_key(&s, &p, u64::MAX, u64::MAX).len());
    }

    #[test]
    fn prefix_isolates_adversarial_neighbours() {
        let (s, p) = sp("wp3", "status");
        let prefix = version_prefix(&s, &p);
        let end = prefix_end(&prefix).unwrap();
        let within = |k: &[u8]| k >= &prefix[..] && k < &end[..];

        assert!(within(&claim_key(&s, &p, 200, 200)));

        // These three must all fall outside the range.
        let (s2, p2) = sp("wp3x", "status");
        assert!(!within(&claim_key(&s2, &p2, 200, 200)));
        let (s3, p3) = sp("wp3", "statusx");
        assert!(!within(&claim_key(&s3, &p3, 200, 200)));
        let (s4, p4) = sp("wp", "status");
        assert!(!within(&claim_key(&s4, &p4, 200, 200)));
    }

    #[test]
    fn prefix_end_handles_trailing_ff() {
        assert_eq!(prefix_end(b"ab").unwrap(), b"ac".to_vec());
        assert_eq!(prefix_end(&[0x61, 0xFF]).unwrap(), vec![0x62]);
        assert_eq!(prefix_end(&[0xFF, 0xFF]), None);
        assert!(prefix_end(b"").is_none());
    }

    #[test]
    fn timestamps_round_trip() {
        let (s, p) = sp("wp3", "status");
        for vf in [0u64, 1, 1786000000000, u64::MAX] {
            for tx in [0u64, 7, 1786000000001, u64::MAX] {
                assert_eq!(timestamps_of(&claim_key(&s, &p, vf, tx)).unwrap(), (vf, tx));
            }
        }
    }

    #[test]
    fn later_knowledge_sorts_first_within_one_valid_from() {
        let (s, p) = sp("wp3", "status");
        let earlier = claim_key(&s, &p, 100, 100);
        let correction = claim_key(&s, &p, 100, 200);
        assert!(correction < earlier, "later tx_time must resolve first");
        assert_ne!(correction, earlier, "distinct knowledge must not share a key");
    }

    #[test]
    fn access_keys_round_trip_with_separator_bearing_identifiers() {
        // `/` is legal in an identifier. A printable separator would make these
        // two encode identically.
        let reader = Reader::new("agent:clyffy/worker").unwrap();
        let a = access_key(500, &reader, &Subject::new("a/b").unwrap(), &Predicate::new("c").unwrap());
        let b = access_key(500, &reader, &Subject::new("a").unwrap(), &Predicate::new("b/c").unwrap());
        assert_ne!(a, b, "distinct identifier splits collided");

        let (at, r, s, p) = parse_access_key(&a).unwrap();
        assert_eq!(at, 500);
        assert_eq!(r.as_str(), "agent:clyffy/worker");
        assert_eq!(s.as_str(), "a/b");
        assert_eq!(p.as_str(), "c");
    }

    #[test]
    fn access_keys_sort_by_time_so_an_interval_is_a_range_scan() {
        let r = Reader::new("r").unwrap();
        let (s, p) = sp("wp3", "status");
        assert!(access_key(9, &r, &s, &p) < access_key(10, &r, &s, &p));
        assert!(access_bound(100) <= access_key(100, &r, &s, &p));
        assert!(access_key(99, &r, &s, &p) < access_bound(100));
    }

    #[test]
    fn claim_keys_yield_their_identifiers_without_decoding_the_claim() {
        for (subject, predicate) in [("wp3", "status"), ("a/b", "c"), ("a", "b/c")] {
            let (s, p) = sp(subject, predicate);
            let (ds, dp) = parse_claim_key(&claim_key(&s, &p, 100, 200)).unwrap();
            assert_eq!(ds.as_str(), subject);
            assert_eq!(dp.as_str(), predicate);
        }
        assert!(parse_claim_key(b"nonsense").is_err());
    }

    #[test]
    fn sequence_keys_sort_ascending_and_round_trip() {
        // Fixed width is what keeps 9 before 10 under byte ordering.
        assert!(sequence_key(9) < sequence_key(10));
        assert!(sequence_key(0) < sequence_key(u64::MAX));
        assert_eq!(sequence_key(0).len(), sequence_key(u64::MAX).len());
        for s in [0u64, 1, 9, 10, 1_000_000, u64::MAX] {
            assert_eq!(sequence_of(&sequence_key(s)).unwrap(), s);
        }
    }

    #[test]
    fn sequence_of_rejects_malformed_keys() {
        assert!(sequence_of(b"123").is_err());
        assert!(sequence_of(&[0xFFu8; 20]).is_err());
    }

    #[test]
    fn seek_ordering_matches_as_of_semantics() {
        let (s, p) = sp("wp3", "status");
        // Seeking at T must land at or before the version that was valid at T.
        let v1 = claim_key(&s, &p, 100, 100);
        let v2 = claim_key(&s, &p, 200, 200);
        let seek150 = seek_key(&s, &p, 150);
        assert!(v2 < seek150, "v2 (later) sorts before the seek point");
        assert!(seek150 <= v1, "seek point sorts at or before v1, so v1 is the first hit");
    }
}
