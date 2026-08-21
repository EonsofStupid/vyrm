//! Portable authenticated append-only log primitives.
//!
//! The tree shape and domain separation follow RFC 9162 section 2.1: empty
//! roots hash the empty string, leaves hash `0x00 || input`, and parents hash
//! `0x01 || left || right`. Vyrm persists complete subtree nodes and a compact
//! frontier, so an append writes at most one node per tree level and a point
//! read can be authenticated with a logarithmic inclusion path.

use crate::{digest, Error, Result, RuntimeChange};
use serde::{Deserialize, Serialize};

pub const RUNTIME_LOG_ACCUMULATOR_VERSION: u16 = 1;

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct RuntimeMerkleNode {
    pub level: u8,
    pub index: u64,
    pub digest: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct RuntimeInclusionProof {
    pub version: u16,
    pub tree_size: u64,
    pub leaf_index: u64,
    pub path: Vec<String>,
}

impl RuntimeInclusionProof {
    pub fn verify(&self, leaf_input: &[u8], expected_root: &str) -> Result<()> {
        if self.version != RUNTIME_LOG_ACCUMULATOR_VERSION {
            return invalid(format!(
                "unsupported runtime inclusion-proof version {}",
                self.version
            ));
        }
        if self.tree_size == 0 || self.leaf_index >= self.tree_size {
            return invalid("runtime inclusion proof names a leaf outside its tree");
        }
        let expected = parse_digest("runtime accumulator root", expected_root)?;
        let mut computed = leaf_hash(leaf_input);
        let mut leaf = self.leaf_index;
        let mut last = self.tree_size - 1;
        for sibling in &self.path {
            if last == 0 {
                return invalid("runtime inclusion proof contains excess path nodes");
            }
            let sibling = parse_digest("runtime inclusion-proof node", sibling)?;
            if leaf & 1 == 1 || leaf == last {
                computed = node_hash(sibling, computed);
                if leaf & 1 == 0 {
                    while leaf != 0 && leaf & 1 == 0 {
                        leaf >>= 1;
                        last >>= 1;
                    }
                }
            } else {
                computed = node_hash(computed, sibling);
            }
            leaf >>= 1;
            last >>= 1;
        }
        if last != 0 || computed != expected {
            return invalid("runtime inclusion proof does not match its accumulator root");
        }
        Ok(())
    }

    pub fn verify_change(&self, change: &RuntimeChange, expected_root: &str) -> Result<()> {
        if !change.verify_digest() || change.cursor.checked_sub(1) != Some(self.leaf_index) {
            return invalid("runtime change does not match the inclusion-proof leaf");
        }
        self.verify(&change.authenticated_log_bytes(), expected_root)
    }
}

/// Compact range for the current append-only log prefix.
///
/// `frontier[level]` is populated exactly when bit `level` of `tree_size` is
/// set. Each populated entry is the root of that complete subtree.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct RuntimeLogAccumulator {
    pub version: u16,
    pub tree_size: u64,
    pub frontier: Vec<Option<String>>,
    pub root: String,
}

impl Default for RuntimeLogAccumulator {
    fn default() -> Self {
        Self::new()
    }
}

impl RuntimeLogAccumulator {
    pub fn new() -> Self {
        Self {
            version: RUNTIME_LOG_ACCUMULATOR_VERSION,
            tree_size: 0,
            frontier: Vec::new(),
            root: encode_digest(empty_root()),
        }
    }

    /// Reconstructs the compact frontier for a retained prefix from complete
    /// subtree nodes. This makes historical snapshot proofs independent of the
    /// latest persisted frontier while still checking the stamped root.
    pub fn from_nodes<F>(tree_size: u64, expected_root: &str, mut node_at: F) -> Result<Self>
    where
        F: FnMut(u8, u64) -> Result<Option<String>>,
    {
        parse_digest("runtime accumulator root", expected_root)?;
        if tree_size == 0 {
            let accumulator = Self::new();
            if accumulator.root != expected_root {
                return invalid("empty runtime accumulator root does not match");
            }
            return Ok(accumulator);
        }
        let levels = 64 - tree_size.leading_zeros() as usize;
        let mut frontier = vec![None; levels];
        let mut start = 0_u64;
        for level in (0..levels).rev() {
            if tree_size & (1_u64 << level) == 0 {
                continue;
            }
            let index = start >> level;
            let value = node_at(level as u8, index)?.ok_or_else(|| Error::InvalidRuntime {
                reason: format!(
                    "runtime accumulator is missing frontier node at level {level}, index {index}"
                ),
            })?;
            parse_digest("runtime accumulator frontier node", &value)?;
            frontier[level] = Some(value);
            start += 1_u64 << level;
        }
        let accumulator = Self {
            version: RUNTIME_LOG_ACCUMULATOR_VERSION,
            tree_size,
            frontier,
            root: expected_root.to_owned(),
        };
        accumulator.validate()?;
        Ok(accumulator)
    }

    pub fn validate(&self) -> Result<()> {
        if self.version != RUNTIME_LOG_ACCUMULATOR_VERSION {
            return invalid(format!(
                "unsupported runtime accumulator version {}",
                self.version
            ));
        }
        if self.frontier.len() > 64 {
            return invalid("runtime accumulator frontier exceeds u64 tree depth");
        }
        for (level, value) in self.frontier.iter().enumerate() {
            let expected = self.tree_size & (1_u64 << level) != 0;
            if value.is_some() != expected {
                return invalid(format!(
                    "runtime accumulator frontier level {level} disagrees with tree size {}",
                    self.tree_size
                ));
            }
            if let Some(value) = value {
                parse_digest("runtime accumulator frontier node", value)?;
            }
        }
        let required = if self.tree_size == 0 {
            0
        } else {
            64 - self.tree_size.leading_zeros() as usize
        };
        if self.frontier.len() != required {
            return invalid(format!(
                "runtime accumulator frontier has {} levels, expected {required}",
                self.frontier.len()
            ));
        }
        let root = parse_digest("runtime accumulator root", &self.root)?;
        if root != self.frontier_root()? {
            return invalid("runtime accumulator root disagrees with its frontier");
        }
        Ok(())
    }

    /// Appends one leaf and returns every newly materialized complete subtree
    /// node, ordered from leaf to the highest parent produced by binary carry.
    pub fn append(&mut self, leaf_input: &[u8]) -> Result<Vec<RuntimeMerkleNode>> {
        self.validate()?;
        if self.tree_size == u64::MAX {
            return invalid("runtime accumulator tree size overflow");
        }
        let leaf_index = self.tree_size;
        let mut node = leaf_hash(leaf_input);
        let mut node_index = leaf_index;
        let mut level = 0usize;
        let mut created = vec![RuntimeMerkleNode {
            level: 0,
            index: leaf_index,
            digest: encode_digest(node),
        }];

        while node_index & 1 == 1 {
            let left = self
                .frontier
                .get_mut(level)
                .and_then(Option::take)
                .ok_or_else(|| Error::InvalidRuntime {
                    reason: format!("runtime accumulator is missing frontier level {level}"),
                })?;
            node = node_hash(
                parse_digest("runtime accumulator frontier node", &left)?,
                node,
            );
            node_index >>= 1;
            level += 1;
            created.push(RuntimeMerkleNode {
                level: u8::try_from(level).expect("u64 tree depth fits u8"),
                index: node_index,
                digest: encode_digest(node),
            });
        }
        if self.frontier.len() <= level {
            self.frontier.resize(level + 1, None);
        }
        self.frontier[level] = Some(encode_digest(node));
        self.tree_size += 1;
        self.root = encode_digest(self.frontier_root()?);
        self.validate()?;
        Ok(created)
    }

    pub fn append_change(&mut self, change: &RuntimeChange) -> Result<Vec<RuntimeMerkleNode>> {
        if !change.verify_digest() || change.cursor.checked_sub(1) != Some(self.tree_size) {
            return invalid("runtime change is not the next valid accumulator leaf");
        }
        self.append(&change.authenticated_log_bytes())
    }

    /// Builds the RFC 9162 audit path using stored complete-subtree nodes.
    pub fn inclusion_proof<F>(
        &self,
        leaf_index: u64,
        mut node_at: F,
    ) -> Result<RuntimeInclusionProof>
    where
        F: FnMut(u8, u64) -> Result<Option<String>>,
    {
        self.validate()?;
        if leaf_index >= self.tree_size {
            return invalid("runtime inclusion proof names a leaf outside its tree");
        }
        let mut path = Vec::new();
        build_path(0, self.tree_size, leaf_index, &mut node_at, &mut path)?;
        Ok(RuntimeInclusionProof {
            version: RUNTIME_LOG_ACCUMULATOR_VERSION,
            tree_size: self.tree_size,
            leaf_index,
            path,
        })
    }

    fn frontier_root(&self) -> Result<[u8; 32]> {
        let mut root = None;
        for value in self.frontier.iter().flatten() {
            let next = parse_digest("runtime accumulator frontier node", value)?;
            root = Some(match root {
                None => next,
                Some(right) => node_hash(next, right),
            });
        }
        Ok(root.unwrap_or_else(empty_root))
    }
}

fn build_path<F>(
    start: u64,
    size: u64,
    leaf_index: u64,
    node_at: &mut F,
    path: &mut Vec<String>,
) -> Result<()>
where
    F: FnMut(u8, u64) -> Result<Option<String>>,
{
    if size == 1 {
        return Ok(());
    }
    let split = largest_power_of_two_less_than(size);
    if leaf_index < start + split {
        build_path(start, split, leaf_index, node_at, path)?;
        path.push(range_hash(start + split, size - split, node_at)?);
    } else {
        build_path(start + split, size - split, leaf_index, node_at, path)?;
        path.push(range_hash(start, split, node_at)?);
    }
    Ok(())
}

fn range_hash<F>(start: u64, size: u64, node_at: &mut F) -> Result<String>
where
    F: FnMut(u8, u64) -> Result<Option<String>>,
{
    if size.is_power_of_two() {
        let level = size.trailing_zeros() as u8;
        let index = start / size;
        let value = node_at(level, index)?.ok_or_else(|| Error::InvalidRuntime {
            reason: format!("runtime accumulator is missing node at level {level}, index {index}"),
        })?;
        parse_digest("runtime accumulator node", &value)?;
        return Ok(value);
    }
    let split = largest_power_of_two_less_than(size);
    let left = parse_digest(
        "runtime accumulator node",
        &range_hash(start, split, node_at)?,
    )?;
    let right = parse_digest(
        "runtime accumulator node",
        &range_hash(start + split, size - split, node_at)?,
    )?;
    Ok(encode_digest(node_hash(left, right)))
}

fn largest_power_of_two_less_than(value: u64) -> u64 {
    debug_assert!(value > 1);
    1_u64 << (63 - (value - 1).leading_zeros())
}

fn empty_root() -> [u8; 32] {
    digest::sha256(&[])
}

fn leaf_hash(input: &[u8]) -> [u8; 32] {
    let mut bytes = Vec::with_capacity(input.len() + 1);
    bytes.push(0);
    bytes.extend_from_slice(input);
    digest::sha256(&bytes)
}

fn node_hash(left: [u8; 32], right: [u8; 32]) -> [u8; 32] {
    let mut bytes = [0_u8; 65];
    bytes[0] = 1;
    bytes[1..33].copy_from_slice(&left);
    bytes[33..].copy_from_slice(&right);
    digest::sha256(&bytes)
}

fn encode_digest(value: [u8; 32]) -> String {
    let mut output = String::with_capacity(64);
    for byte in value {
        use std::fmt::Write as _;
        write!(&mut output, "{byte:02x}").expect("writing to String cannot fail");
    }
    output
}

fn parse_digest(label: &str, value: &str) -> Result<[u8; 32]> {
    if value.len() != 64 {
        return invalid(format!("{label} must be a 64-character SHA-256 digest"));
    }
    let mut bytes = [0_u8; 32];
    for (index, slot) in bytes.iter_mut().enumerate() {
        *slot = u8::from_str_radix(&value[index * 2..index * 2 + 2], 16).map_err(|_| {
            Error::InvalidRuntime {
                reason: format!("{label} must contain lowercase hexadecimal"),
            }
        })?;
    }
    if encode_digest(bytes) != value {
        return invalid(format!("{label} must contain lowercase hexadecimal"));
    }
    Ok(bytes)
}

fn invalid<T>(reason: impl Into<String>) -> Result<T> {
    Err(Error::InvalidRuntime {
        reason: reason.into(),
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde::Deserialize;
    use std::collections::BTreeMap;

    #[derive(Deserialize)]
    struct Golden {
        version: u16,
        leaves: Vec<String>,
        root: String,
        proofs: Vec<GoldenProof>,
    }

    #[derive(Deserialize)]
    struct GoldenProof {
        leaf_index: u64,
        path: Vec<String>,
    }

    #[test]
    fn matches_checked_in_cross_language_vector() {
        let golden: Golden =
            serde_json::from_str(include_str!("../fixtures/authenticated-log-v1.json")).unwrap();
        assert_eq!(golden.version, RUNTIME_LOG_ACCUMULATOR_VERSION);
        let mut accumulator = RuntimeLogAccumulator::new();
        let mut nodes = BTreeMap::new();
        for leaf in &golden.leaves {
            for node in accumulator.append(leaf.as_bytes()).unwrap() {
                nodes.insert((node.level, node.index), node.digest);
            }
        }
        assert_eq!(accumulator.root, golden.root);
        for expected in golden.proofs {
            let proof = accumulator
                .inclusion_proof(expected.leaf_index, |level, index| {
                    Ok(nodes.get(&(level, index)).cloned())
                })
                .unwrap();
            assert_eq!(proof.path, expected.path);
            proof
                .verify(
                    golden.leaves[expected.leaf_index as usize].as_bytes(),
                    &golden.root,
                )
                .unwrap();
        }
    }

    #[test]
    fn matches_rfc_domain_separation_and_authenticates_every_shape() {
        let mut accumulator = RuntimeLogAccumulator::new();
        assert_eq!(
            accumulator.root,
            "e3b0c44298fc1c149afbf4c8996fb92427ae41e4649b934ca495991b7852b855"
        );
        let leaves = (0_u64..257)
            .map(|index| format!("runtime-change-{index}"))
            .collect::<Vec<_>>();
        let mut nodes = BTreeMap::new();
        for (index, leaf) in leaves.iter().enumerate() {
            for node in accumulator.append(leaf.as_bytes()).unwrap() {
                nodes.insert((node.level, node.index), node.digest);
            }
            for target in [0, index / 2, index] {
                let proof = accumulator
                    .inclusion_proof(target as u64, |level, node_index| {
                        Ok(nodes.get(&(level, node_index)).cloned())
                    })
                    .unwrap();
                proof
                    .verify(leaves[target].as_bytes(), &accumulator.root)
                    .unwrap();
                assert!(proof.path.len() <= 9);
            }
        }
    }

    #[test]
    fn corrupt_and_malformed_proofs_fail_closed() {
        let mut accumulator = RuntimeLogAccumulator::new();
        let mut nodes = BTreeMap::new();
        for leaf in [b"zero".as_slice(), b"one", b"two", b"three", b"four"] {
            for node in accumulator.append(leaf).unwrap() {
                nodes.insert((node.level, node.index), node.digest);
            }
        }
        let proof = accumulator
            .inclusion_proof(3, |level, index| Ok(nodes.get(&(level, index)).cloned()))
            .unwrap();
        proof.verify(b"three", &accumulator.root).unwrap();
        assert!(proof.verify(b"tampered", &accumulator.root).is_err());

        let mut corrupt = proof.clone();
        corrupt.path[0].replace_range(0..2, "ff");
        assert!(corrupt.verify(b"three", &accumulator.root).is_err());

        let mut excess = proof.clone();
        excess.path.push(accumulator.root.clone());
        assert!(excess.verify(b"three", &accumulator.root).is_err());

        let mut malformed = accumulator.clone();
        malformed.frontier[0] = None;
        assert!(malformed.validate().is_err());
    }

    #[test]
    fn retained_prefix_frontier_reconstructs_after_later_appends() {
        let mut accumulator = RuntimeLogAccumulator::new();
        let mut nodes = BTreeMap::new();
        let leaves = (0_u64..19)
            .map(|index| format!("leaf-{index}"))
            .collect::<Vec<_>>();
        let mut retained = None;
        for (index, leaf) in leaves.iter().enumerate() {
            for node in accumulator.append(leaf.as_bytes()).unwrap() {
                nodes.insert((node.level, node.index), node.digest);
            }
            if index == 10 {
                retained = Some(accumulator.clone());
            }
        }
        let retained = retained.unwrap();
        let reconstructed = RuntimeLogAccumulator::from_nodes(
            retained.tree_size,
            &retained.root,
            |level, index| Ok(nodes.get(&(level, index)).cloned()),
        )
        .unwrap();
        assert_eq!(reconstructed, retained);
        let proof = reconstructed
            .inclusion_proof(7, |level, index| Ok(nodes.get(&(level, index)).cloned()))
            .unwrap();
        proof.verify(leaves[7].as_bytes(), &retained.root).unwrap();
    }
}
