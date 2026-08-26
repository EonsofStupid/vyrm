//! Deterministic BM25 projection primitives.
//!
//! Canonical records remain authoritative in RRD. This artifact is a
//! content-addressable projection over one immutable source cursor. The score
//! follows Qdrant's BM25 sparse-vector reference: document-side term-frequency
//! saturation and query-side Robertson IDF.

use crate::{Error, Result};
use rrd_core::digest;
use serde::{Deserialize, Serialize};
use std::collections::{BTreeMap, BTreeSet};

pub const BM25_ARTIFACT_CONTRACT_VERSION: u16 = 1;
const MAX_TOKEN_CHARS: usize = 40;
const MAX_DOCUMENTS: usize = 1_000_000;
const MAX_TERMS: usize = 10_000_000;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Bm25Analyzer {
    /// Split on non-alphanumeric characters and apply Unicode lowercase.
    UnicodeLowercase,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Bm25Config {
    pub analyzer: Bm25Analyzer,
    /// BM25 term-frequency saturation parameter in millionths.
    pub k1_micros: u32,
    /// BM25 document-length normalization parameter in millionths.
    pub b_micros: u32,
}

impl Default for Bm25Config {
    fn default() -> Self {
        Self {
            analyzer: Bm25Analyzer::UnicodeLowercase,
            k1_micros: 1_200_000,
            b_micros: 750_000,
        }
    }
}

impl Bm25Config {
    pub fn validate(&self) -> Result<()> {
        if self.k1_micros == 0 || self.k1_micros > 10_000_000 {
            return Err(Error::Catalog("BM25 k1 must be in (0, 10]".into()));
        }
        if self.b_micros > 1_000_000 {
            return Err(Error::Catalog("BM25 b must be in [0, 1]".into()));
        }
        Ok(())
    }

    fn k1(&self) -> f64 {
        f64::from(self.k1_micros) / 1_000_000.0
    }

    fn b(&self) -> f64 {
        f64::from(self.b_micros) / 1_000_000.0
    }

    pub fn digest(&self) -> Result<String> {
        self.validate()?;
        Ok(digest::sha256_hex(&serde_json::to_vec(self)?))
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Bm25Document {
    pub identity: String,
    pub length: u32,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Bm25Posting {
    pub document_ordinal: u32,
    pub term_frequency: u32,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Bm25Artifact {
    pub contract_version: u16,
    pub config: Bm25Config,
    pub config_sha256: String,
    pub source_cursor: u64,
    pub schema_revision: u64,
    pub valid_at: u64,
    pub total_document_length: u64,
    pub average_document_length: f64,
    pub documents: Vec<Bm25Document>,
    pub postings: BTreeMap<String, Vec<Bm25Posting>>,
}

impl Bm25Artifact {
    pub fn build<I, S>(
        config: Bm25Config,
        source_cursor: u64,
        schema_revision: u64,
        valid_at: u64,
        documents: I,
    ) -> Result<Self>
    where
        I: IntoIterator<Item = (S, S)>,
        S: Into<String>,
    {
        config.validate()?;
        if source_cursor == 0 || schema_revision == 0 {
            return Err(Error::Catalog(
                "BM25 source cursor and schema revision must be greater than zero".into(),
            ));
        }
        let mut source = documents
            .into_iter()
            .map(|(identity, text)| (identity.into(), text.into()))
            .collect::<Vec<_>>();
        if source.len() > MAX_DOCUMENTS {
            return Err(Error::Budget("BM25 document limit exceeded".into()));
        }
        source.sort_by(|left, right| left.0.cmp(&right.0));
        if source.windows(2).any(|pair| pair[0].0 == pair[1].0) {
            return Err(Error::Catalog(
                "BM25 document identities must be unique".into(),
            ));
        }

        let mut indexed_documents = Vec::with_capacity(source.len());
        let mut postings = BTreeMap::<String, Vec<Bm25Posting>>::new();
        let mut total_document_length = 0_u64;
        for (ordinal, (identity, text)) in source.into_iter().enumerate() {
            let tokens = analyze(config.analyzer, &text);
            let length = u32::try_from(tokens.len())
                .map_err(|_| Error::Budget("BM25 document token count exceeds u32".into()))?;
            total_document_length = total_document_length
                .checked_add(u64::from(length))
                .ok_or_else(|| Error::Budget("BM25 corpus token count overflow".into()))?;
            let mut frequencies = BTreeMap::<String, u32>::new();
            for token in tokens {
                let frequency = frequencies.entry(token).or_default();
                *frequency = frequency
                    .checked_add(1)
                    .ok_or_else(|| Error::Budget("BM25 term frequency overflow".into()))?;
            }
            let document_ordinal = u32::try_from(ordinal)
                .map_err(|_| Error::Budget("BM25 document ordinal exceeds u32".into()))?;
            for (term, term_frequency) in frequencies {
                postings.entry(term).or_default().push(Bm25Posting {
                    document_ordinal,
                    term_frequency,
                });
            }
            indexed_documents.push(Bm25Document { identity, length });
        }
        if postings.len() > MAX_TERMS {
            return Err(Error::Budget("BM25 term limit exceeded".into()));
        }
        let average_document_length = if indexed_documents.is_empty() {
            0.0
        } else {
            total_document_length as f64 / indexed_documents.len() as f64
        };
        let artifact = Self {
            contract_version: BM25_ARTIFACT_CONTRACT_VERSION,
            config_sha256: config.digest()?,
            config,
            source_cursor,
            schema_revision,
            valid_at,
            total_document_length,
            average_document_length,
            documents: indexed_documents,
            postings,
        };
        artifact.validate()?;
        Ok(artifact)
    }

    pub fn validate(&self) -> Result<()> {
        if self.contract_version != BM25_ARTIFACT_CONTRACT_VERSION {
            return Err(Error::Integrity(format!(
                "unsupported BM25 artifact contract version {}",
                self.contract_version
            )));
        }
        self.config.validate()?;
        if self.config_sha256 != self.config.digest()? {
            return Err(Error::Integrity(
                "BM25 configuration digest does not match".into(),
            ));
        }
        if self.source_cursor == 0 || self.schema_revision == 0 {
            return Err(Error::Integrity(
                "BM25 source cursor and schema revision must be greater than zero".into(),
            ));
        }
        if self.documents.len() > MAX_DOCUMENTS || self.postings.len() > MAX_TERMS {
            return Err(Error::Integrity("BM25 artifact bounds exceeded".into()));
        }
        if self
            .documents
            .windows(2)
            .any(|pair| pair[0].identity >= pair[1].identity)
        {
            return Err(Error::Integrity(
                "BM25 document identities must be unique and sorted".into(),
            ));
        }
        let total = self.documents.iter().try_fold(0_u64, |total, document| {
            total
                .checked_add(u64::from(document.length))
                .ok_or_else(|| Error::Integrity("BM25 corpus length overflow".into()))
        })?;
        if total != self.total_document_length {
            return Err(Error::Integrity(
                "BM25 total document length does not match documents".into(),
            ));
        }
        let expected_average = if self.documents.is_empty() {
            0.0
        } else {
            total as f64 / self.documents.len() as f64
        };
        if !self.average_document_length.is_finite()
            || self.average_document_length.to_bits() != expected_average.to_bits()
        {
            return Err(Error::Integrity(
                "BM25 average document length does not match documents".into(),
            ));
        }
        for (term, postings) in &self.postings {
            if term.is_empty()
                || term.chars().count() > MAX_TOKEN_CHARS
                || postings.is_empty()
                || postings
                    .windows(2)
                    .any(|pair| pair[0].document_ordinal >= pair[1].document_ordinal)
            {
                return Err(Error::Integrity(
                    "BM25 term or posting order is invalid".into(),
                ));
            }
            if postings.iter().any(|posting| {
                posting.term_frequency == 0
                    || usize::try_from(posting.document_ordinal)
                        .ok()
                        .is_none_or(|ordinal| ordinal >= self.documents.len())
            }) {
                return Err(Error::Integrity(
                    "BM25 posting references an invalid document or frequency".into(),
                ));
            }
        }
        Ok(())
    }

    pub fn encode(&self) -> Result<Vec<u8>> {
        self.validate()?;
        Ok(serde_json::to_vec(self)?)
    }

    pub fn decode(bytes: &[u8]) -> Result<Self> {
        let artifact: Self = serde_json::from_slice(bytes)?;
        artifact.validate()?;
        if artifact.encode()? != bytes {
            return Err(Error::Integrity(
                "BM25 artifact bytes are not canonical".into(),
            ));
        }
        Ok(artifact)
    }

    pub fn digest(&self) -> Result<String> {
        Ok(digest::sha256_hex(&self.encode()?))
    }

    pub fn search(&self, query: &str, top_k: usize) -> Result<Vec<Bm25Hit>> {
        self.validate()?;
        if top_k == 0 {
            return Err(Error::Budget("BM25 top_k must be greater than zero".into()));
        }
        let query_terms = analyze(self.config.analyzer, query)
            .into_iter()
            .collect::<BTreeSet<_>>();
        if query_terms.is_empty() || self.documents.is_empty() {
            return Ok(Vec::new());
        }
        let document_count = self.documents.len() as f64;
        let average_length = self.average_document_length;
        let k1 = self.config.k1();
        let b = self.config.b();
        let mut scores = BTreeMap::<u32, (f64, Vec<String>)>::new();
        for term in query_terms {
            let Some(postings) = self.postings.get(&term) else {
                continue;
            };
            let document_frequency = postings.len() as f64;
            let idf = ((document_count - document_frequency + 0.5) / (document_frequency + 0.5)
                + 1.0)
                .ln();
            for posting in postings {
                let ordinal = usize::try_from(posting.document_ordinal)
                    .map_err(|_| Error::Integrity("BM25 document ordinal exceeds usize".into()))?;
                let document_length = f64::from(self.documents[ordinal].length);
                let frequency = f64::from(posting.term_frequency);
                let normalization = if average_length == 0.0 {
                    1.0
                } else {
                    1.0 - b + b * document_length / average_length
                };
                let contribution =
                    idf * (frequency * (k1 + 1.0)) / (frequency + k1 * normalization);
                let score = scores.entry(posting.document_ordinal).or_default();
                score.0 += contribution;
                score.1.push(term.clone());
            }
        }
        let mut hits = scores
            .into_iter()
            .map(|(ordinal, (score, matched_terms))| {
                let ordinal = usize::try_from(ordinal)
                    .map_err(|_| Error::Integrity("BM25 document ordinal exceeds usize".into()))?;
                Ok(Bm25Hit {
                    identity: self.documents[ordinal].identity.clone(),
                    score,
                    matched_terms,
                })
            })
            .collect::<Result<Vec<_>>>()?;
        hits.sort_by(|left, right| {
            right
                .score
                .total_cmp(&left.score)
                .then_with(|| left.identity.cmp(&right.identity))
        });
        hits.truncate(top_k);
        Ok(hits)
    }
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Bm25Hit {
    pub identity: String,
    pub score: f64,
    pub matched_terms: Vec<String>,
}

fn analyze(analyzer: Bm25Analyzer, text: &str) -> Vec<String> {
    match analyzer {
        Bm25Analyzer::UnicodeLowercase => text
            .split(|character: char| !character.is_alphanumeric())
            .filter(|token| !token.is_empty())
            .map(|token| {
                token
                    .chars()
                    .flat_map(char::to_lowercase)
                    .collect::<String>()
            })
            .filter(|token| token.chars().count() <= MAX_TOKEN_CHARS)
            .collect(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn artifact() -> Bm25Artifact {
        Bm25Artifact::build(
            Bm25Config::default(),
            7,
            2,
            100,
            [
                ("doc:a", "alpha beta beta"),
                ("doc:b", "alpha gamma"),
                ("doc:c", "delta"),
            ],
        )
        .unwrap()
    }

    #[test]
    fn qdrant_reference_equation_is_applied() {
        let artifact = artifact();
        let hit = artifact.search("beta", 10).unwrap().remove(0);
        let n = 3.0_f64;
        let df = 1.0_f64;
        let idf = ((n - df + 0.5) / (df + 0.5) + 1.0).ln();
        let tf = 2.0_f64;
        let k1 = 1.2_f64;
        let b = 0.75_f64;
        let dl = 3.0_f64;
        let avg = 2.0_f64;
        let expected = idf * (tf * (k1 + 1.0)) / (tf + k1 * (1.0 - b + b * dl / avg));
        assert_eq!(hit.identity, "doc:a");
        assert!((hit.score - expected).abs() < 1e-12);
        assert_eq!(hit.matched_terms, ["beta"]);
    }

    #[test]
    fn build_and_bytes_are_deterministic() {
        let left = artifact();
        let right = Bm25Artifact::build(
            Bm25Config::default(),
            7,
            2,
            100,
            [
                ("doc:c", "delta"),
                ("doc:a", "alpha beta beta"),
                ("doc:b", "alpha gamma"),
            ],
        )
        .unwrap();
        assert_eq!(left, right);
        assert_eq!(left.encode().unwrap(), right.encode().unwrap());
        assert_eq!(left.digest().unwrap(), right.digest().unwrap());
    }

    #[test]
    fn canonical_round_trip_and_corruption_rejection() {
        let artifact = artifact();
        let bytes = artifact.encode().unwrap();
        assert_eq!(Bm25Artifact::decode(&bytes).unwrap(), artifact);

        let mut corrupt: serde_json::Value = serde_json::from_slice(&bytes).unwrap();
        corrupt["total_document_length"] = serde_json::json!(999);
        assert!(Bm25Artifact::decode(&serde_json::to_vec(&corrupt).unwrap()).is_err());
    }

    #[test]
    fn unicode_lowercase_and_deterministic_ties() {
        let artifact = Bm25Artifact::build(
            Bm25Config::default(),
            1,
            1,
            1,
            [("doc:b", "CAFÉ"), ("doc:a", "café")],
        )
        .unwrap();
        let hits = artifact.search("Café", 10).unwrap();
        assert_eq!(hits.len(), 2);
        assert_eq!(hits[0].identity, "doc:a");
        assert_eq!(hits[1].identity, "doc:b");
    }

    #[test]
    fn empty_and_invalid_requests_fail_closed() {
        let artifact = artifact();
        assert!(artifact.search("beta", 0).is_err());
        assert!(Bm25Artifact::build(
            Bm25Config {
                analyzer: Bm25Analyzer::UnicodeLowercase,
                k1_micros: 0,
                b_micros: 750_000,
            },
            1,
            1,
            1,
            [("doc:a", "alpha")],
        )
        .is_err());
    }
}
