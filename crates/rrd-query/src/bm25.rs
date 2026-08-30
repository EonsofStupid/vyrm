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
use unicode_normalization::{char::is_combining_mark, UnicodeNormalization};

pub const BM25_ARTIFACT_CONTRACT_VERSION: u16 = 2;
const MAX_TOKEN_CHARS: usize = 40;
const MAX_STOP_WORDS: usize = 10_000;
const MAX_DOCUMENTS: usize = 1_000_000;
const MAX_TERMS: usize = 10_000_000;
const MAX_POSITIONS: usize = 100_000_000;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Bm25Analyzer {
    /// Split on non-alphanumeric characters and apply Unicode lowercase.
    UnicodeLowercase,
    /// Preserve the source case after tokenization.
    UnicodeCaseSensitive,
}

#[derive(Debug, Default, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Bm25Tokenizer {
    /// Split at every non-alphanumeric Unicode scalar.
    #[default]
    UnicodeAlphanumeric,
    /// Split only at Unicode whitespace.
    Whitespace,
}

#[derive(Debug, Default, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Bm25Stemmer {
    #[default]
    None,
    /// A deterministic, bounded English suffix stemmer.
    English,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Bm25Config {
    pub analyzer: Bm25Analyzer,
    #[serde(default, skip_serializing_if = "is_default_tokenizer")]
    pub tokenizer: Bm25Tokenizer,
    #[serde(default, skip_serializing_if = "is_false")]
    pub ascii_folding: bool,
    #[serde(default, skip_serializing_if = "BTreeSet::is_empty")]
    pub stop_words: BTreeSet<String>,
    #[serde(
        default = "default_min_token_chars",
        skip_serializing_if = "is_default_min_token_chars"
    )]
    pub min_token_chars: u16,
    #[serde(
        default = "default_max_token_chars",
        skip_serializing_if = "is_default_max_token_chars"
    )]
    pub max_token_chars: u16,
    #[serde(default, skip_serializing_if = "is_default_stemmer")]
    pub stemmer: Bm25Stemmer,
    /// BM25 term-frequency saturation parameter in millionths.
    pub k1_micros: u32,
    /// BM25 document-length normalization parameter in millionths.
    pub b_micros: u32,
}

impl Default for Bm25Config {
    fn default() -> Self {
        Self {
            analyzer: Bm25Analyzer::UnicodeLowercase,
            tokenizer: Bm25Tokenizer::UnicodeAlphanumeric,
            ascii_folding: false,
            stop_words: BTreeSet::new(),
            min_token_chars: 1,
            max_token_chars: MAX_TOKEN_CHARS as u16,
            stemmer: Bm25Stemmer::None,
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
        if self.min_token_chars == 0
            || self.min_token_chars > self.max_token_chars
            || usize::from(self.max_token_chars) > MAX_TOKEN_CHARS
        {
            return Err(Error::Catalog(format!(
                "BM25 token length must satisfy 1 <= min <= max <= {MAX_TOKEN_CHARS}"
            )));
        }
        if self.stop_words.len() > MAX_STOP_WORDS
            || self.stop_words.iter().any(|word| word.is_empty())
        {
            return Err(Error::Catalog(
                "BM25 stop-word configuration is invalid".into(),
            ));
        }
        let mut normalization_config = self.clone();
        normalization_config.stop_words.clear();
        if self
            .stop_words
            .iter()
            .any(|word| analyze(&normalization_config, word).as_slice() != [word.as_str()])
        {
            return Err(Error::Catalog(
                "BM25 stop words must already be unique normalized single tokens".into(),
            ));
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
    pub positions: Vec<Bm25Offset>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Bm25Offset {
    pub token_ordinal: u32,
    pub byte_start: u32,
    pub byte_end: u32,
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
            let tokens = analyze_with_offsets(&config, &text)?;
            let length = u32::try_from(tokens.len())
                .map_err(|_| Error::Budget("BM25 document token count exceeds u32".into()))?;
            total_document_length = total_document_length
                .checked_add(u64::from(length))
                .ok_or_else(|| Error::Budget("BM25 corpus token count overflow".into()))?;
            let mut frequencies = BTreeMap::<String, Vec<Bm25Offset>>::new();
            for token in tokens {
                frequencies
                    .entry(token.term)
                    .or_default()
                    .push(token.offset);
            }
            let document_ordinal = u32::try_from(ordinal)
                .map_err(|_| Error::Budget("BM25 document ordinal exceeds u32".into()))?;
            for (term, positions) in frequencies {
                let term_frequency = u32::try_from(positions.len())
                    .map_err(|_| Error::Budget("BM25 term frequency overflow".into()))?;
                postings.entry(term).or_default().push(Bm25Posting {
                    document_ordinal,
                    term_frequency,
                    positions,
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
        let mut total_positions = 0_usize;
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
            for posting in postings {
                let Some(document) = usize::try_from(posting.document_ordinal)
                    .ok()
                    .and_then(|ordinal| self.documents.get(ordinal))
                else {
                    return Err(Error::Integrity(
                        "BM25 posting references an invalid document".into(),
                    ));
                };
                if posting.term_frequency == 0
                    || usize::try_from(posting.term_frequency).ok() != Some(posting.positions.len())
                    || posting.positions.windows(2).any(|pair| pair[0] >= pair[1])
                    || posting.positions.iter().any(|position| {
                        position.byte_start >= position.byte_end
                            || position.token_ordinal >= document.length
                    })
                {
                    return Err(Error::Integrity(
                        "BM25 posting position or frequency is invalid".into(),
                    ));
                }
            }
            total_positions = total_positions.saturating_add(
                postings
                    .iter()
                    .map(|posting| posting.positions.len())
                    .sum::<usize>(),
            );
            if total_positions > MAX_POSITIONS {
                return Err(Error::Integrity(
                    "BM25 posting position limit exceeded".into(),
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
        let query_terms = analyze(&self.config, query)
            .into_iter()
            .collect::<BTreeSet<_>>();
        if query_terms.is_empty() || self.documents.is_empty() {
            return Ok(Vec::new());
        }
        let document_count = self.documents.len() as f64;
        let average_length = self.average_document_length;
        let k1 = self.config.k1();
        let b = self.config.b();
        let mut scores = BTreeMap::<u32, (f64, Vec<String>, Vec<Bm25Offset>)>::new();
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
                score.2.extend_from_slice(&posting.positions);
            }
        }
        let mut hits = scores
            .into_iter()
            .map(|(ordinal, (score, matched_terms, mut offsets))| {
                let ordinal = usize::try_from(ordinal)
                    .map_err(|_| Error::Integrity("BM25 document ordinal exceeds usize".into()))?;
                offsets.sort_unstable();
                offsets.dedup();
                Ok(Bm25Hit {
                    identity: self.documents[ordinal].identity.clone(),
                    score,
                    matched_terms,
                    offsets,
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
    pub offsets: Vec<Bm25Offset>,
}

#[derive(Debug)]
struct AnalyzedToken {
    term: String,
    offset: Bm25Offset,
}

fn analyze(config: &Bm25Config, text: &str) -> Vec<String> {
    analyze_with_offsets(config, text)
        .unwrap_or_default()
        .into_iter()
        .map(|token| token.term)
        .collect()
}

fn analyze_with_offsets(config: &Bm25Config, text: &str) -> Result<Vec<AnalyzedToken>> {
    let spans = token_spans(config.tokenizer, text);
    let mut tokens = Vec::new();
    for (byte_start, byte_end) in spans {
        let raw = &text[byte_start..byte_end];
        let mut term = match config.analyzer {
            Bm25Analyzer::UnicodeLowercase => raw.chars().flat_map(char::to_lowercase).collect(),
            Bm25Analyzer::UnicodeCaseSensitive => raw.to_owned(),
        };
        if config.ascii_folding {
            term = term
                .nfkd()
                .filter(|character| !is_combining_mark(*character))
                .collect();
        }
        if config.stemmer == Bm25Stemmer::English {
            term = stem_english(&term);
        }
        let chars = term.chars().count();
        if chars < usize::from(config.min_token_chars)
            || chars > usize::from(config.max_token_chars)
            || config.stop_words.contains(&term)
        {
            continue;
        }
        let token_ordinal = u32::try_from(tokens.len())
            .map_err(|_| Error::Budget("BM25 token ordinal exceeds u32".into()))?;
        tokens.push(AnalyzedToken {
            term,
            offset: Bm25Offset {
                token_ordinal,
                byte_start: u32::try_from(byte_start)
                    .map_err(|_| Error::Budget("BM25 text byte offset exceeds u32".into()))?,
                byte_end: u32::try_from(byte_end)
                    .map_err(|_| Error::Budget("BM25 text byte offset exceeds u32".into()))?,
            },
        });
    }
    Ok(tokens)
}

fn token_spans(tokenizer: Bm25Tokenizer, text: &str) -> Vec<(usize, usize)> {
    let accepted = |character: char| match tokenizer {
        Bm25Tokenizer::UnicodeAlphanumeric => character.is_alphanumeric(),
        Bm25Tokenizer::Whitespace => !character.is_whitespace(),
    };
    let mut spans = Vec::new();
    let mut start = None;
    for (offset, character) in text.char_indices() {
        if accepted(character) {
            start.get_or_insert(offset);
        } else if let Some(begin) = start.take() {
            spans.push((begin, offset));
        }
    }
    if let Some(begin) = start {
        spans.push((begin, text.len()));
    }
    spans
}

fn stem_english(term: &str) -> String {
    if term.len() > 4 && term.ends_with("ies") {
        return format!("{}y", &term[..term.len() - 3]);
    }
    for suffix in ["ingly", "edly", "ing", "ed"] {
        if term.len() > suffix.len() + 2 && term.ends_with(suffix) {
            return term[..term.len() - suffix.len()].to_owned();
        }
    }
    if term.len() > 4
        && ["sses", "ches", "shes", "xes", "zes", "ses"]
            .iter()
            .any(|suffix| term.ends_with(suffix))
    {
        return term[..term.len() - 2].to_owned();
    }
    if term.len() > 3 && term.ends_with('s') {
        return term[..term.len() - 1].to_owned();
    }
    term.to_owned()
}

pub fn highlight_offsets(
    text: &str,
    offsets: &[Bm25Offset],
    prefix: &str,
    suffix: &str,
) -> Result<String> {
    let mut ordered = offsets.to_vec();
    ordered.sort_unstable_by_key(|offset| (offset.byte_start, offset.byte_end));
    let mut output = String::new();
    let mut cursor = 0_usize;
    for offset in ordered {
        let start = usize::try_from(offset.byte_start)
            .map_err(|_| Error::Integrity("BM25 highlight offset exceeds usize".into()))?;
        let end = usize::try_from(offset.byte_end)
            .map_err(|_| Error::Integrity("BM25 highlight offset exceeds usize".into()))?;
        if start < cursor
            || end > text.len()
            || !text.is_char_boundary(start)
            || !text.is_char_boundary(end)
        {
            return Err(Error::Integrity(
                "BM25 highlight offsets are invalid for the source text".into(),
            ));
        }
        output.push_str(&text[cursor..start]);
        output.push_str(prefix);
        output.push_str(&text[start..end]);
        output.push_str(suffix);
        cursor = end;
    }
    output.push_str(&text[cursor..]);
    Ok(output)
}

fn default_min_token_chars() -> u16 {
    1
}
fn default_max_token_chars() -> u16 {
    MAX_TOKEN_CHARS as u16
}
fn is_default_min_token_chars(value: &u16) -> bool {
    *value == default_min_token_chars()
}
fn is_default_max_token_chars(value: &u16) -> bool {
    *value == default_max_token_chars()
}
fn is_default_tokenizer(value: &Bm25Tokenizer) -> bool {
    *value == Bm25Tokenizer::default()
}
fn is_default_stemmer(value: &Bm25Stemmer) -> bool {
    *value == Bm25Stemmer::default()
}
fn is_false(value: &bool) -> bool {
    !*value
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
                ..Bm25Config::default()
            },
            1,
            1,
            1,
            [("doc:a", "alpha")],
        )
        .is_err());
    }

    #[test]
    fn configurable_pipeline_scores_and_highlights_fixed_corpus() {
        let config = Bm25Config {
            ascii_folding: true,
            stop_words: BTreeSet::from(["the".into()]),
            min_token_chars: 2,
            stemmer: Bm25Stemmer::English,
            ..Bm25Config::default()
        };
        let artifact = Bm25Artifact::build(
            config,
            9,
            3,
            200,
            [
                ("doc:a", "The cafés are running quickly"),
                ("doc:b", "A cafe runner"),
            ],
        )
        .unwrap();
        let hits = artifact.search("CAFÉ running", 10).unwrap();
        assert_eq!(hits[0].identity, "doc:a");
        let source_hit = hits.iter().find(|hit| hit.identity == "doc:a").unwrap();
        assert_eq!(source_hit.matched_terms, ["cafe", "runn"]);
        assert_eq!(
            highlight_offsets(
                "The cafés are running quickly",
                &source_hit.offsets,
                "<b>",
                "</b>"
            )
            .unwrap(),
            "The <b>cafés</b> are <b>running</b> quickly"
        );
    }

    #[test]
    fn whitespace_and_case_sensitive_analyzers_are_distinct() {
        let artifact = Bm25Artifact::build(
            Bm25Config {
                analyzer: Bm25Analyzer::UnicodeCaseSensitive,
                tokenizer: Bm25Tokenizer::Whitespace,
                ..Bm25Config::default()
            },
            3,
            1,
            1,
            [("doc:a", "Alpha,beta Alpha,beta")],
        )
        .unwrap();
        assert!(artifact.search("alpha,beta", 10).unwrap().is_empty());
        assert_eq!(artifact.search("Alpha,beta", 10).unwrap().len(), 1);
    }
}
