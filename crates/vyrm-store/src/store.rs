//! The substrate-backed claim store.

use crate::error::{Error, Result};
use crate::gc::{build_report, RemovalReport, Tally};
use crate::invocation::{self, Invocation, InvocationInput};
use crate::keyspaces::{self, Durability};
use fjall::{KeyspaceCreateOptions, Readable, SingleWriterTxDatabase, SingleWriterTxKeyspace};
use std::collections::BTreeMap;
use std::path::Path;
use vyrm_core::{key, Claim, ClaimSource, Millis, Predicate, Reader, Subject};

/// Sequences assigned by an append.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct AppendOutcome {
    pub first_sequence: u64,
    pub last_sequence: u64,
    pub count: usize,
}

pub struct Store {
    db: SingleWriterTxDatabase,
    claims: SingleWriterTxKeyspace,
    /// Append sequence to claim key. Written in the same transaction as the
    /// claim, so the index cannot diverge from the watermark.
    sequence_index: SingleWriterTxKeyspace,
    access: SingleWriterTxKeyspace,
    meta: SingleWriterTxKeyspace,
    /// Recorded operator invocations (`SPEC.md` §13).
    invocations: SingleWriterTxKeyspace,
    /// Derived projections, stored whole under a caller-chosen name.
    projections: SingleWriterTxKeyspace,
}

impl Store {
    pub fn open(path: &Path) -> Result<Self> {
        std::fs::create_dir_all(path).map_err(|e| Error::Substrate(e.to_string()))?;
        let db = SingleWriterTxDatabase::builder(path)
            .manual_journal_persist(true)
            .open()?;
        let claims = db.keyspace(keyspaces::CLAIMS, KeyspaceCreateOptions::default)?;
        let sequence_index =
            db.keyspace(keyspaces::SEQUENCE_INDEX, KeyspaceCreateOptions::default)?;
        let access = db.keyspace(keyspaces::ACCESS, KeyspaceCreateOptions::default)?;
        let meta = db.keyspace(keyspaces::META, KeyspaceCreateOptions::default)?;
        let invocations =
            db.keyspace(keyspaces::INVOCATIONS, KeyspaceCreateOptions::default)?;
        let projections =
            db.keyspace(keyspaces::PROJECTIONS, KeyspaceCreateOptions::default)?;
        Ok(Self {
            db,
            claims,
            sequence_index,
            access,
            meta,
            invocations,
            projections,
        })
    }

    /// Stores a derived projection under a name, replacing any prior value.
    ///
    /// Buffered durability: a projection is derivable from its sources, so a
    /// crash-lost write costs the next process a rebuild, never truth. An
    /// authoritative fsync here would charge every projection refresh the
    /// 0.431 ms the durability classes exist to avoid (`SPEC.md` §7.1).
    pub fn put_projection(&self, name: &str, bytes: &[u8]) -> Result<()> {
        let mut tx = self
            .db
            .write_tx()
            .durability(Durability::Buffered.persist_mode());
        tx.insert(&self.projections, name.as_bytes(), bytes);
        tx.commit()?;
        Ok(())
    }

    /// Loads a projection by name. `None` means the caller rebuilds from
    /// sources — absence is a recovery path, not an error.
    pub fn get_projection(&self, name: &str) -> Result<Option<Vec<u8>>> {
        let snapshot = self.db.read_tx();
        Ok(snapshot
            .get(&self.projections, name.as_bytes())?
            .map(|value| value.to_vec()))
    }

    /// Current claim sequence watermark.
    pub fn sequence(&self) -> Result<u64> {
        let snapshot = self.db.read_tx();
        match snapshot.get(&self.meta, keyspaces::SEQUENCE_WATERMARK)? {
            Some(value) => decode_sequence(&value),
            None => Ok(0),
        }
    }

    /// Appends claims in one transaction with one fsync.
    ///
    /// `SPEC.md` §11 corrections 1, 2, 3 and 5. The sequence watermark is read
    /// and advanced **inside** the transaction, so allocation does not depend on
    /// an external lock; increment uses `checked_add`; and the commit carries the
    /// single fsync.
    pub fn append_batch(&self, claims: &[Claim]) -> Result<AppendOutcome> {
        if claims.is_empty() {
            let at = self.sequence()?;
            return Ok(AppendOutcome {
                first_sequence: at,
                last_sequence: at,
                count: 0,
            });
        }

        let mut tx = self
            .db
            .write_tx()
            .durability(Durability::Authoritative.persist_mode());

        // Correction 1: allocation is inside the transaction.
        let start = match tx.get(&self.meta, keyspaces::SEQUENCE_WATERMARK)? {
            Some(value) => decode_sequence(&value)?,
            None => 0,
        };

        let mut sequence = start;
        for claim in claims {
            claim.validate()?;
            // Correction 2: overflow is reported, never saturated.
            sequence = sequence.checked_add(1).ok_or(Error::SequenceOverflow)?;
            let encoded = serde_json::to_vec(claim)?;
            let claim_key =
                key::claim_key(&claim.subject, &claim.predicate, claim.valid_from, claim.tx_time);
            // The index entry is written in this same transaction, so it cannot
            // diverge from the watermark advanced below.
            tx.insert(
                &self.sequence_index,
                key::sequence_key(sequence),
                claim_key.clone(),
            );
            tx.insert(&self.claims, claim_key, encoded);
        }

        tx.insert(
            &self.meta,
            keyspaces::SEQUENCE_WATERMARK,
            sequence.to_string().as_bytes(),
        );

        // Correction 3: the commit carries durability. No persist call follows.
        tx.commit()?;

        Ok(AppendOutcome {
            first_sequence: start + 1,
            last_sequence: sequence,
            count: claims.len(),
        })
    }

    /// Appends a single claim. Equivalent to a batch of one; provided for call
    /// sites that genuinely have one claim, not as the preferred write path.
    pub fn assert(&self, claim: &Claim) -> Result<AppendOutcome> {
        self.append_batch(std::slice::from_ref(claim))
    }

    /// Records a read against a claim. Buffered: telemetry must not pay for
    /// durability (`SPEC.md` §7.1).
    ///
    /// The record lives entirely in the key, so no value is stored. Every field
    /// is recoverable via [`key::parse_access_key`].
    pub fn observe(
        &self,
        reader: &Reader,
        subject: &Subject,
        predicate: &Predicate,
        at: Millis,
    ) -> Result<()> {
        let mut tx = self
            .db
            .write_tx()
            .durability(Durability::Buffered.persist_mode());
        tx.insert(&self.access, key::access_key(at, reader, subject, predicate), []);
        tx.commit()?;
        Ok(())
    }

    /// Derives removal candidates over the interval `[since, evaluated_at]`.
    ///
    /// `SPEC.md` §7: a pair with no access record in the interval is a
    /// candidate; a pair with any access is retained. Every verdict carries its
    /// evidence.
    ///
    /// Identifiers are read from keys rather than from claim values, so the scan
    /// does not deserialize claims.
    pub fn removal_report(&self, since: Millis, evaluated_at: Millis) -> Result<RemovalReport> {
        let snapshot = self.db.read_tx();
        let mut tallies: BTreeMap<(String, String), Tally> = BTreeMap::new();

        for guard in snapshot.range(&self.claims, Vec::new()..) {
            let (claim_key, _) = guard.into_inner()?;
            let (subject, predicate) = key::parse_claim_key(&claim_key)?;
            tallies
                .entry((subject.as_str().to_owned(), predicate.as_str().to_owned()))
                .or_default()
                .claim_count += 1;
        }

        // Access keys lead with time, so the interval is a forward range scan.
        for guard in snapshot.range(&self.access, key::access_bound(since)..) {
            let (access_key, _) = guard.into_inner()?;
            let (at, reader, subject, predicate) = key::parse_access_key(&access_key)?;
            if at > evaluated_at {
                break;
            }
            let tally = tallies
                .entry((subject.as_str().to_owned(), predicate.as_str().to_owned()))
                .or_default();
            tally.access_count += 1;
            if tally.last_access.is_none_or(|previous| at >= previous) {
                tally.last_access = Some(at);
                tally.last_reader = Some(reader);
            }
        }

        Ok(build_report(tallies, since, evaluated_at)?)
    }

    /// Approximate number of recorded access records, as reported by the
    /// substrate. Not exact under deletion, and therefore unsuitable as an
    /// authoritative count.
    pub fn access_count(&self) -> usize {
        self.access.approximate_len()
    }

    /// Records one invocation. `SPEC.md` §13 stage 1.
    ///
    /// Authoritative durability: these records are the evidence from which
    /// automation policy is later derived, so they are not telemetry. The
    /// ordinal is allocated inside the transaction, for the same reason the
    /// claim sequence is (§11 correction 1).
    ///
    /// Returns the recorded invocation, including its allocated ordinal.
    pub fn record_invocation(&self, input: InvocationInput<'_>) -> Result<Invocation> {
        let mut tx = self
            .db
            .write_tx()
            .durability(Durability::Authoritative.persist_mode());
        let previous = match tx.get(&self.meta, keyspaces::INVOCATION_WATERMARK)? {
            Some(value) => decode_sequence(&value)?,
            None => 0,
        };
        let ordinal = previous.checked_add(1).ok_or(Error::SequenceOverflow)?;

        let record = Invocation {
            ordinal,
            at: input.at,
            trigger: input.trigger,
            command: input.command.to_owned(),
            arguments: input.arguments.to_vec(),
            outcome: input.outcome,
            duration_ms: input.duration_ms,
            detail: input.detail,
            effectiveness: input.effectiveness,
        };
        tx.insert(
            &self.invocations,
            invocation::invocation_key(input.at, ordinal),
            serde_json::to_vec(&record)?,
        );
        tx.insert(
            &self.meta,
            keyspaces::INVOCATION_WATERMARK,
            ordinal.to_string().as_bytes(),
        );
        tx.commit()?;
        Ok(record)
    }

    /// Judges a recall after the fact. `SPEC.md` §13.1: `outcome` is the
    /// signal trigger policy is derived from, and it arrives later than the
    /// recall it judges — so the record is rewritten in place, keyed as it was
    /// written.
    ///
    /// The lookup scans the log for the ordinal, which is linear in the number
    /// of invocations. Acceptable at stage 1 by construction: every invocation
    /// is manual, so the log grows at operator speed. An ordinal index earns
    /// its place when a measurement shows this scan mattering.
    ///
    /// Errors when the ordinal does not exist or names a non-recall record —
    /// judging a flush as `accepted` would poison the evidence base silently.
    pub fn set_recall_outcome(
        &self,
        ordinal: u64,
        outcome: crate::invocation::RecallOutcome,
    ) -> Result<Invocation> {
        let mut tx = self
            .db
            .write_tx()
            .durability(Durability::Authoritative.persist_mode());

        let mut found: Option<(Vec<u8>, Invocation)> = None;
        for guard in tx.range(&self.invocations, invocation::invocation_bound(0)..) {
            let (key, value) = guard.into_inner()?;
            let record: Invocation = serde_json::from_slice(&value)?;
            if record.ordinal == ordinal {
                found = Some((key.to_vec(), record));
                break;
            }
        }
        let Some((key, mut record)) = found else {
            return Err(Error::Substrate(format!("no invocation with ordinal {ordinal}")));
        };
        let Some(effectiveness) = record.effectiveness.as_mut() else {
            return Err(Error::Substrate(format!(
                "invocation {ordinal} is `{}`, not a recall — refusing to judge it",
                record.command
            )));
        };
        effectiveness.outcome = outcome;

        tx.insert(&self.invocations, key, serde_json::to_vec(&record)?);
        tx.commit()?;
        Ok(record)
    }

    /// Invocations recorded at or after `since`, in chronological order.
    pub fn invocations_since(&self, since: Millis) -> Result<Vec<Invocation>> {
        let snapshot = self.db.read_tx();
        let mut out = Vec::new();
        for guard in snapshot.range(&self.invocations, invocation::invocation_bound(since)..) {
            let (_, value) = guard.into_inner()?;
            out.push(serde_json::from_slice(&value)?);
        }
        Ok(out)
    }

    /// Count of recorded invocations, from the watermark rather than an
    /// approximate keyspace length.
    pub fn invocation_count(&self) -> Result<u64> {
        let snapshot = self.db.read_tx();
        match snapshot.get(&self.meta, keyspaces::INVOCATION_WATERMARK)? {
            Some(value) => decode_sequence(&value),
            None => Ok(0),
        }
    }

    /// Claims appended in the sequence range `(from, to]`.
    ///
    /// The bound is half-open below to match the rebuild interval in
    /// `SPEC.md` §8.2, so that a watermark can be passed directly as `from`
    /// without re-applying the claim at that position.
    ///
    /// Claims are returned in append order. A sequence whose index entry points
    /// at a claim key written more than once resolves to the single stored
    /// claim: asserting an identical claim twice advances the sequence but does
    /// not duplicate content, since the key covers every distinguishing field.
    pub fn claims_in_range(&self, from: u64, to: u64) -> Result<Vec<Claim>> {
        if from >= to {
            return Ok(Vec::new());
        }
        let snapshot = self.db.read_tx();
        let start = key::sequence_key(from.saturating_add(1));
        let end = key::sequence_key(to);
        let mut out = Vec::new();
        for guard in snapshot.range(&self.sequence_index, start..=end) {
            let (_, claim_key) = guard.into_inner()?;
            let Some(encoded) = snapshot.get(&self.claims, &claim_key)? else {
                // The index and the claims keyspace are written in one
                // transaction, so a dangling pointer indicates substrate
                // corruption rather than a recoverable condition.
                return Err(Error::Substrate(format!(
                    "sequence index references a claim key that is not stored: {}",
                    String::from_utf8_lossy(&claim_key)
                )));
            };
            out.push(serde_json::from_slice(&encoded)?);
        }
        Ok(out)
    }

    /// Every claim, in append order.
    pub fn all_claims(&self) -> Result<Vec<Claim>> {
        self.claims_in_range(0, self.sequence()?)
    }

    /// Scans one subject and predicate from `from`, bounded by the version
    /// prefix. Reads take a snapshot and acquire no write lock
    /// (`SPEC.md` §11 correction 4).
    fn scan(
        &self,
        subject: &Subject,
        predicate: &Predicate,
        from: Vec<u8>,
    ) -> Result<Vec<Claim>> {
        let prefix = key::version_prefix(subject, predicate);
        let snapshot = self.db.read_tx();
        let mut out = Vec::new();
        match key::prefix_end(&prefix) {
            Some(end) => {
                for guard in snapshot.range(&self.claims, from..end) {
                    let (_, value) = guard.into_inner()?;
                    out.push(serde_json::from_slice(&value)?);
                }
            }
            None => {
                for guard in snapshot.range(&self.claims, from..) {
                    let (_, value) = guard.into_inner()?;
                    out.push(serde_json::from_slice(&value)?);
                }
            }
        }
        Ok(out)
    }
}

impl ClaimSource for Store {
    type Error = Error;

    fn versions_at_or_before(
        &self,
        subject: &Subject,
        predicate: &Predicate,
        as_of: Millis,
    ) -> Result<Vec<Claim>> {
        self.scan(subject, predicate, key::seek_key(subject, predicate, as_of))
    }

    fn all_versions(&self, subject: &Subject, predicate: &Predicate) -> Result<Vec<Claim>> {
        self.scan(subject, predicate, key::version_prefix(subject, predicate))
    }

    fn subject_versions(&self, subject: &Subject) -> Result<Vec<Claim>> {
        let prefix = key::subject_prefix(subject);
        let snapshot = self.db.read_tx();
        let mut out = Vec::new();
        match key::prefix_end(&prefix) {
            Some(end) => {
                for guard in snapshot.range(&self.claims, prefix..end) {
                    let (_, value) = guard.into_inner()?;
                    out.push(serde_json::from_slice(&value)?);
                }
            }
            None => {
                for guard in snapshot.range(&self.claims, prefix..) {
                    let (_, value) = guard.into_inner()?;
                    out.push(serde_json::from_slice(&value)?);
                }
            }
        }
        Ok(out)
    }
}

fn decode_sequence(value: &[u8]) -> Result<u64> {
    std::str::from_utf8(value)
        .map_err(|e| Error::CorruptWatermark(e.to_string()))?
        .parse::<u64>()
        .map_err(|e| Error::CorruptWatermark(e.to_string()))
}
