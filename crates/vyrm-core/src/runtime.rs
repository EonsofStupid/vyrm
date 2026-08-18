//! Typed, append-only contract for the persisted reasoning runtime graph.
//!
//! A runtime commit is the unit of causality and durability. It may contain
//! claims, typed records, typed relations, and lifecycle events. Storage
//! adapters allocate a single global cursor for every mutation, hash-chain the
//! resulting changes, and reject a commit whose expected cursor is stale.

use crate::{digest, Claim, Error, Millis, Result};
use serde::{Deserialize, Serialize};
use std::collections::{BTreeMap, BTreeSet, VecDeque};
use std::fmt;

macro_rules! runtime_ident {
    ($name:ident, $label:literal) => {
        #[derive(Clone, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
        #[serde(try_from = "String", into = "String")]
        pub struct $name(String);

        impl $name {
            pub fn new(value: impl Into<String>) -> Result<Self> {
                let value = value.into();
                validate_identifier($label, &value)?;
                Ok(Self(value))
            }

            pub fn as_str(&self) -> &str {
                &self.0
            }
        }

        impl TryFrom<String> for $name {
            type Error = Error;

            fn try_from(value: String) -> Result<Self> {
                Self::new(value)
            }
        }

        impl From<$name> for String {
            fn from(value: $name) -> Self {
                value.0
            }
        }

        impl fmt::Debug for $name {
            fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
                write!(f, "{}({:?})", $label, self.0)
            }
        }

        impl fmt::Display for $name {
            fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
                f.write_str(&self.0)
            }
        }
    };
}

runtime_ident!(ScopeId, "runtime scope");
runtime_ident!(RuntimeType, "runtime type");
runtime_ident!(RuntimeId, "runtime id");

fn validate_identifier(kind: &'static str, value: &str) -> Result<()> {
    if value.is_empty() {
        return Err(Error::EmptyIdentifier { kind });
    }
    if value.as_bytes().contains(&0) {
        return Err(Error::SeparatorInIdentifier {
            kind,
            value: value.to_owned(),
        });
    }
    Ok(())
}

/// A stable, typed reference within one runtime scope.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
pub struct RuntimeRef {
    pub kind: RuntimeType,
    pub id: RuntimeId,
}

impl RuntimeRef {
    pub fn new(kind: impl Into<String>, id: impl Into<String>) -> Result<Self> {
        Ok(Self {
            kind: RuntimeType::new(kind)?,
            id: RuntimeId::new(id)?,
        })
    }
}

/// Dependency-free property values. Decimal values are strings deliberately:
/// their canonical identity must not depend on platform floating-point rules.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "type", content = "value", rename_all = "snake_case")]
pub enum RuntimeValue {
    Null,
    Bool(bool),
    Integer(i64),
    Unsigned(u64),
    Decimal(String),
    String(String),
    Digest(String),
    List(Vec<RuntimeValue>),
    Map(BTreeMap<String, RuntimeValue>),
}

pub type RuntimeProperties = BTreeMap<String, RuntimeValue>;

/// One immutable version of a graph node.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct RuntimeRecord {
    #[serde(flatten)]
    pub reference: RuntimeRef,
    pub valid_from: Millis,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub valid_to: Option<Millis>,
    #[serde(default)]
    pub properties: RuntimeProperties,
}

/// One immutable version of a directed, typed graph edge.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct RuntimeRelation {
    #[serde(flatten)]
    pub reference: RuntimeRef,
    pub from: RuntimeRef,
    pub to: RuntimeRef,
    pub valid_from: Millis,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub valid_to: Option<Millis>,
    #[serde(default)]
    pub properties: RuntimeProperties,
}

/// A lifecycle fact that need not itself be a graph node or edge.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct RuntimeEvent {
    pub kind: RuntimeType,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub subject: Option<RuntimeRef>,
    #[serde(default)]
    pub properties: RuntimeProperties,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(tag = "mutation", rename_all = "snake_case")]
pub enum RuntimeMutation {
    Claim { claim: Claim },
    Record { record: RuntimeRecord },
    Relation { relation: RuntimeRelation },
    Event { event: RuntimeEvent },
}

/// The caller-observed head is mandatory. This is the compare-and-swap that
/// turns concurrent writers into explicit conflicts rather than lost updates.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct RuntimeCommit {
    pub scope: ScopeId,
    pub at: Millis,
    pub actor: String,
    pub expected_cursor: u64,
    pub mutations: Vec<RuntimeMutation>,
}

impl RuntimeCommit {
    pub fn validate(&self) -> Result<()> {
        validate_text("runtime actor", &self.actor)?;
        if self.mutations.is_empty() {
            return Err(Error::InvalidRuntime {
                reason: "runtime commit must contain at least one mutation".into(),
            });
        }

        let mut identities = BTreeSet::new();
        for mutation in &self.mutations {
            match mutation {
                RuntimeMutation::Claim { claim } => claim.validate()?,
                RuntimeMutation::Record { record } => {
                    validate_window(record.valid_from, record.valid_to)?;
                    validate_properties(&record.properties)?;
                    if !identities.insert(("record", record.reference.clone())) {
                        return duplicate_identity("record", &record.reference);
                    }
                }
                RuntimeMutation::Relation { relation } => {
                    validate_window(relation.valid_from, relation.valid_to)?;
                    validate_properties(&relation.properties)?;
                    if !identities.insert(("relation", relation.reference.clone())) {
                        return duplicate_identity("relation", &relation.reference);
                    }
                }
                RuntimeMutation::Event { event } => validate_properties(&event.properties)?,
            }
        }
        Ok(())
    }

    /// Content identity for idempotency and cross-adapter conformance.
    pub fn digest(&self) -> String {
        digest::sha256_hex(&self.canonical_bytes())
    }

    pub fn canonical_bytes(&self) -> Vec<u8> {
        let mut out = b"vyrm-runtime-commit-v1\0".to_vec();
        text(&mut out, self.scope.as_str());
        out.extend_from_slice(&self.at.to_be_bytes());
        text(&mut out, &self.actor);
        out.extend_from_slice(&self.expected_cursor.to_be_bytes());
        out.extend_from_slice(&(self.mutations.len() as u64).to_be_bytes());
        for mutation in &self.mutations {
            encode_mutation(&mut out, mutation);
        }
        out
    }
}

/// A mutation after the store has assigned its global position.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct RuntimeChange {
    pub cursor: u64,
    pub commit_id: String,
    pub commit_ordinal: u64,
    pub scope: ScopeId,
    pub at: Millis,
    pub actor: String,
    pub mutation: RuntimeMutation,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub previous_digest: Option<String>,
    pub digest: String,
}

impl RuntimeChange {
    pub fn committed(
        cursor: u64,
        commit: &RuntimeCommit,
        commit_id: &str,
        commit_ordinal: u64,
        mutation: RuntimeMutation,
        previous_digest: Option<String>,
    ) -> Self {
        let mut change = Self {
            cursor,
            commit_id: commit_id.to_owned(),
            commit_ordinal,
            scope: commit.scope.clone(),
            at: commit.at,
            actor: commit.actor.clone(),
            mutation,
            previous_digest,
            digest: String::new(),
        };
        change.digest = digest::sha256_hex(&change.canonical_bytes());
        change
    }

    pub fn verify_digest(&self) -> bool {
        digest::sha256_hex(&self.canonical_bytes()) == self.digest
    }

    fn canonical_bytes(&self) -> Vec<u8> {
        let mut out = b"vyrm-runtime-change-v1\0".to_vec();
        out.extend_from_slice(&self.cursor.to_be_bytes());
        text(&mut out, &self.commit_id);
        out.extend_from_slice(&self.commit_ordinal.to_be_bytes());
        text(&mut out, self.scope.as_str());
        out.extend_from_slice(&self.at.to_be_bytes());
        text(&mut out, &self.actor);
        encode_mutation(&mut out, &self.mutation);
        optional_text(&mut out, self.previous_digest.as_deref());
        out
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RuntimeCommitOutcome {
    pub commit_id: String,
    pub first_cursor: u64,
    pub last_cursor: u64,
    pub count: usize,
    pub first_claim_sequence: Option<u64>,
    pub last_claim_sequence: Option<u64>,
}

/// One bounded replay page. Consumers advance to `through_cursor`, even if a
/// scope filter produced no matching changes, so sparse feeds cannot stall.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct RuntimeChangePage {
    pub requested_after: u64,
    pub through_cursor: u64,
    pub head_cursor: u64,
    pub changes: Vec<RuntimeChange>,
}

impl RuntimeChangePage {
    pub fn has_more(&self) -> bool {
        self.through_cursor < self.head_cursor
    }
}

/// A transaction-consistent structural graph reconstructed at a global cursor
/// and valid-time instant.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct RuntimeGraphSnapshot {
    pub scope: ScopeId,
    pub valid_at: Millis,
    pub known_at_cursor: u64,
    pub records: Vec<RuntimeRecord>,
    pub relations: Vec<RuntimeRelation>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct RuntimeRecordChange {
    pub before: RuntimeRecord,
    pub after: RuntimeRecord,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct RuntimeRelationChange {
    pub before: RuntimeRelation,
    pub after: RuntimeRelation,
}

/// Exact structural differential between two graph snapshots.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct RuntimeGraphDiff {
    pub from_cursor: u64,
    pub to_cursor: u64,
    pub added_records: Vec<RuntimeRecord>,
    pub removed_records: Vec<RuntimeRecord>,
    pub changed_records: Vec<RuntimeRecordChange>,
    pub added_relations: Vec<RuntimeRelation>,
    pub removed_relations: Vec<RuntimeRelation>,
    pub changed_relations: Vec<RuntimeRelationChange>,
}

impl RuntimeGraphSnapshot {
    pub fn from_changes(
        changes: &[RuntimeChange],
        scope: ScopeId,
        valid_at: Millis,
        known_at_cursor: u64,
    ) -> Self {
        let mut records = BTreeMap::<RuntimeRef, RuntimeRecord>::new();
        let mut relations = BTreeMap::<RuntimeRef, RuntimeRelation>::new();
        for change in changes
            .iter()
            .filter(|change| change.cursor <= known_at_cursor && change.scope == scope)
        {
            match &change.mutation {
                RuntimeMutation::Record { record } => {
                    // Ignore versions whose modeled validity has not begun at
                    // this instant. For every eligible identity, the last
                    // transaction-visible version wins; its valid_to below can
                    // then explicitly retire the identity.
                    if record.valid_from <= valid_at {
                        records.insert(record.reference.clone(), record.clone());
                    }
                }
                RuntimeMutation::Relation { relation } => {
                    if relation.valid_from <= valid_at {
                        relations.insert(relation.reference.clone(), relation.clone());
                    }
                }
                RuntimeMutation::Event { event } => {
                    let event_ref = RuntimeRef {
                        kind: event.kind.clone(),
                        id: RuntimeId::new(format!("cursor:{}", change.cursor))
                            .expect("cursor event id is valid"),
                    };
                    records.insert(
                        event_ref.clone(),
                        RuntimeRecord {
                            reference: event_ref.clone(),
                            valid_from: change.at,
                            valid_to: None,
                            properties: event.properties.clone(),
                        },
                    );
                    if let Some(subject) = &event.subject {
                        let relation_ref = RuntimeRef {
                            kind: RuntimeType::new("emitted")
                                .expect("static runtime type is valid"),
                            id: RuntimeId::new(format!("cursor:{}", change.cursor))
                                .expect("cursor relation id is valid"),
                        };
                        relations.insert(
                            relation_ref.clone(),
                            RuntimeRelation {
                                reference: relation_ref,
                                from: subject.clone(),
                                to: event_ref,
                                valid_from: change.at,
                                valid_to: None,
                                properties: RuntimeProperties::new(),
                            },
                        );
                    }
                }
                RuntimeMutation::Claim { .. } => {}
            }
        }
        let records = records
            .into_values()
            .filter(|record| valid_at_window(record.valid_from, record.valid_to, valid_at))
            .collect();
        let relations = relations
            .into_values()
            .filter(|relation| valid_at_window(relation.valid_from, relation.valid_to, valid_at))
            .collect();
        Self {
            scope,
            valid_at,
            known_at_cursor,
            records,
            relations,
        }
    }

    pub fn outgoing<'a>(
        &'a self,
        from: &'a RuntimeRef,
    ) -> impl Iterator<Item = &'a RuntimeRelation> {
        self.relations
            .iter()
            .filter(move |relation| &relation.from == from)
    }

    pub fn incoming<'a>(&'a self, to: &'a RuntimeRef) -> impl Iterator<Item = &'a RuntimeRelation> {
        self.relations
            .iter()
            .filter(move |relation| &relation.to == to)
    }

    /// Breadth-first structural traversal. `max_depth == 0` returns only the
    /// start node. An empty relation-kind set permits every edge type.
    pub fn traverse(
        &self,
        start: &RuntimeRef,
        max_depth: usize,
        relation_kinds: &BTreeSet<RuntimeType>,
    ) -> Vec<RuntimeRef> {
        let mut visited = BTreeSet::from([start.clone()]);
        let mut queue = VecDeque::from([(start.clone(), 0usize)]);
        while let Some((current, depth)) = queue.pop_front() {
            if depth >= max_depth {
                continue;
            }
            for relation in self.outgoing(&current) {
                if !relation_kinds.is_empty() && !relation_kinds.contains(&relation.reference.kind)
                {
                    continue;
                }
                if visited.insert(relation.to.clone()) {
                    queue.push_back((relation.to.clone(), depth + 1));
                }
            }
        }
        visited.into_iter().collect()
    }

    pub fn diff(&self, newer: &Self) -> RuntimeGraphDiff {
        let before_records = self
            .records
            .iter()
            .map(|record| (record.reference.clone(), record))
            .collect::<BTreeMap<_, _>>();
        let after_records = newer
            .records
            .iter()
            .map(|record| (record.reference.clone(), record))
            .collect::<BTreeMap<_, _>>();
        let before_relations = self
            .relations
            .iter()
            .map(|relation| (relation.reference.clone(), relation))
            .collect::<BTreeMap<_, _>>();
        let after_relations = newer
            .relations
            .iter()
            .map(|relation| (relation.reference.clone(), relation))
            .collect::<BTreeMap<_, _>>();

        RuntimeGraphDiff {
            from_cursor: self.known_at_cursor,
            to_cursor: newer.known_at_cursor,
            added_records: after_records
                .iter()
                .filter(|(id, _)| !before_records.contains_key(*id))
                .map(|(_, record)| (*record).clone())
                .collect(),
            removed_records: before_records
                .iter()
                .filter(|(id, _)| !after_records.contains_key(*id))
                .map(|(_, record)| (*record).clone())
                .collect(),
            changed_records: after_records
                .iter()
                .filter_map(|(id, after)| {
                    let before = before_records.get(id)?;
                    (*before != *after).then(|| RuntimeRecordChange {
                        before: (*before).clone(),
                        after: (*after).clone(),
                    })
                })
                .collect(),
            added_relations: after_relations
                .iter()
                .filter(|(id, _)| !before_relations.contains_key(*id))
                .map(|(_, relation)| (*relation).clone())
                .collect(),
            removed_relations: before_relations
                .iter()
                .filter(|(id, _)| !after_relations.contains_key(*id))
                .map(|(_, relation)| (*relation).clone())
                .collect(),
            changed_relations: after_relations
                .iter()
                .filter_map(|(id, after)| {
                    let before = before_relations.get(id)?;
                    (*before != *after).then(|| RuntimeRelationChange {
                        before: (*before).clone(),
                        after: (*after).clone(),
                    })
                })
                .collect(),
        }
    }
}

fn duplicate_identity<T>(kind: &str, reference: &RuntimeRef) -> Result<T> {
    Err(Error::InvalidRuntime {
        reason: format!(
            "runtime commit contains duplicate {kind} identity {}/{}",
            reference.kind, reference.id
        ),
    })
}

fn validate_text(kind: &'static str, value: &str) -> Result<()> {
    validate_identifier(kind, value)
}

fn validate_window(valid_from: Millis, valid_to: Option<Millis>) -> Result<()> {
    if let Some(valid_to) = valid_to {
        if valid_to <= valid_from {
            return Err(Error::InvalidValidityWindow {
                valid_from,
                valid_to,
            });
        }
    }
    Ok(())
}

fn valid_at_window(valid_from: Millis, valid_to: Option<Millis>, at: Millis) -> bool {
    valid_from <= at && valid_to.is_none_or(|valid_to| at < valid_to)
}

fn validate_properties(properties: &RuntimeProperties) -> Result<()> {
    for key in properties.keys() {
        validate_identifier("runtime property", key)?;
    }
    Ok(())
}

fn text(out: &mut Vec<u8>, value: &str) {
    out.extend_from_slice(&(value.len() as u64).to_be_bytes());
    out.extend_from_slice(value.as_bytes());
}

fn optional_text(out: &mut Vec<u8>, value: Option<&str>) {
    out.push(u8::from(value.is_some()));
    if let Some(value) = value {
        text(out, value);
    }
}

fn encode_ref(out: &mut Vec<u8>, reference: &RuntimeRef) {
    text(out, reference.kind.as_str());
    text(out, reference.id.as_str());
}

fn encode_window(out: &mut Vec<u8>, valid_from: Millis, valid_to: Option<Millis>) {
    out.extend_from_slice(&valid_from.to_be_bytes());
    out.push(u8::from(valid_to.is_some()));
    if let Some(valid_to) = valid_to {
        out.extend_from_slice(&valid_to.to_be_bytes());
    }
}

fn encode_properties(out: &mut Vec<u8>, properties: &RuntimeProperties) {
    out.extend_from_slice(&(properties.len() as u64).to_be_bytes());
    for (key, value) in properties {
        text(out, key);
        encode_value(out, value);
    }
}

fn encode_value(out: &mut Vec<u8>, value: &RuntimeValue) {
    match value {
        RuntimeValue::Null => out.push(0),
        RuntimeValue::Bool(value) => {
            out.push(1);
            out.push(u8::from(*value));
        }
        RuntimeValue::Integer(value) => {
            out.push(2);
            out.extend_from_slice(&value.to_be_bytes());
        }
        RuntimeValue::Unsigned(value) => {
            out.push(3);
            out.extend_from_slice(&value.to_be_bytes());
        }
        RuntimeValue::Decimal(value) => {
            out.push(4);
            text(out, value);
        }
        RuntimeValue::String(value) => {
            out.push(5);
            text(out, value);
        }
        RuntimeValue::Digest(value) => {
            out.push(6);
            text(out, value);
        }
        RuntimeValue::List(values) => {
            out.push(7);
            out.extend_from_slice(&(values.len() as u64).to_be_bytes());
            for value in values {
                encode_value(out, value);
            }
        }
        RuntimeValue::Map(values) => {
            out.push(8);
            encode_properties(out, values);
        }
    }
}

fn encode_mutation(out: &mut Vec<u8>, mutation: &RuntimeMutation) {
    match mutation {
        RuntimeMutation::Claim { claim } => {
            out.push(0);
            let bytes = claim.canonical_bytes();
            out.extend_from_slice(&(bytes.len() as u64).to_be_bytes());
            out.extend_from_slice(&bytes);
        }
        RuntimeMutation::Record { record } => {
            out.push(1);
            encode_ref(out, &record.reference);
            encode_window(out, record.valid_from, record.valid_to);
            encode_properties(out, &record.properties);
        }
        RuntimeMutation::Relation { relation } => {
            out.push(2);
            encode_ref(out, &relation.reference);
            encode_ref(out, &relation.from);
            encode_ref(out, &relation.to);
            encode_window(out, relation.valid_from, relation.valid_to);
            encode_properties(out, &relation.properties);
        }
        RuntimeMutation::Event { event } => {
            out.push(3);
            text(out, event.kind.as_str());
            out.push(u8::from(event.subject.is_some()));
            if let Some(subject) = &event.subject {
                encode_ref(out, subject);
            }
            encode_properties(out, &event.properties);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn record(kind: &str, id: &str, at: u64) -> RuntimeRecord {
        RuntimeRecord {
            reference: RuntimeRef::new(kind, id).unwrap(),
            valid_from: at,
            valid_to: None,
            properties: RuntimeProperties::new(),
        }
    }

    #[test]
    fn commit_identity_is_stable_and_sensitive_to_order() {
        let base = RuntimeCommit {
            scope: ScopeId::new("instance:test").unwrap(),
            at: 10,
            actor: "agent:test".into(),
            expected_cursor: 0,
            mutations: vec![RuntimeMutation::Record {
                record: record("prompt", "p1", 10),
            }],
        };
        assert_eq!(base.digest(), base.clone().digest());
        let mut changed = base.clone();
        changed.mutations.push(RuntimeMutation::Record {
            record: record("outcome", "o1", 10),
        });
        assert_ne!(base.digest(), changed.digest());
    }

    #[test]
    fn snapshot_is_bitemporal_and_traversable() {
        let scope = ScopeId::new("instance:test").unwrap();
        let prompt = record("prompt", "p1", 10);
        let outcome = record("outcome", "o1", 10);
        let relation = RuntimeRelation {
            reference: RuntimeRef::new("caused", "p1-o1").unwrap(),
            from: prompt.reference.clone(),
            to: outcome.reference.clone(),
            valid_from: 10,
            valid_to: None,
            properties: RuntimeProperties::new(),
        };
        let commit = RuntimeCommit {
            scope: scope.clone(),
            at: 10,
            actor: "agent:test".into(),
            expected_cursor: 0,
            mutations: vec![
                RuntimeMutation::Record {
                    record: prompt.clone(),
                },
                RuntimeMutation::Record {
                    record: outcome.clone(),
                },
                RuntimeMutation::Relation { relation },
            ],
        };
        let id = commit.digest();
        let mut previous = None;
        let changes = commit
            .mutations
            .clone()
            .into_iter()
            .enumerate()
            .map(|(index, mutation)| {
                let change = RuntimeChange::committed(
                    index as u64 + 1,
                    &commit,
                    &id,
                    index as u64,
                    mutation,
                    previous.clone(),
                );
                previous = Some(change.digest.clone());
                change
            })
            .collect::<Vec<_>>();
        let graph = RuntimeGraphSnapshot::from_changes(&changes, scope, 10, 3);
        assert_eq!(graph.records.len(), 2);
        assert_eq!(graph.relations.len(), 1);
        assert_eq!(
            graph.traverse(&prompt.reference, 1, &BTreeSet::new()).len(),
            2
        );
    }
}
