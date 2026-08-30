use crate::{
    invalid, AuditPage, CanonicalId, ChangefeedPage, DataObjectReceipt, DataProperties,
    DataSchemaRegistry, EstateSnapshot, ProductCapabilityCatalogue, QueryIndexCatalogueSnapshot,
    Readiness, ResourceId, RuntimeToolCatalogue, VectorCollectionCatalogueSnapshot,
    MAX_CHANGEFEED_PAGE, MAX_MESSAGE_BYTES, MAX_QUERY_SCANNED_CHANGES,
};
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};

pub const DIAGNOSTIC_SNAPSHOT_FORMAT_VERSION: u16 = 1;
pub const MAX_DIAGNOSTIC_AUDIT_RECORDS: u16 = 1_024;

/// Coordinates a bounded diagnostic read. Cursors are explicit so a client can
/// resume without asking RRD to infer history from UI state.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct ReadDiagnosticSnapshot {
    pub scope: String,
    pub graph_valid_at_unix_ms: u64,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub graph_known_at_cursor: Option<u64>,
    pub graph_compare_cursor: u64,
    pub runtime_max_scanned_changes: u64,
    pub changes_after_cursor: u64,
    pub change_limit: u64,
    pub audit_after_sequence: u64,
    pub audit_limit: u16,
}

impl ReadDiagnosticSnapshot {
    pub fn validate(&self) -> crate::Result<()> {
        if self.scope.is_empty()
            || self.scope.len() > crate::MAX_ID_BYTES
            || self.scope.as_bytes().contains(&0)
        {
            return invalid("diagnostic scope is invalid");
        }
        if self.change_limit == 0 || self.change_limit > MAX_CHANGEFEED_PAGE {
            return invalid(format!(
                "diagnostic change_limit must be in 1..={MAX_CHANGEFEED_PAGE}"
            ));
        }
        if self.runtime_max_scanned_changes == 0
            || self.runtime_max_scanned_changes > MAX_QUERY_SCANNED_CHANGES
        {
            return invalid(format!(
                "diagnostic runtime_max_scanned_changes must be in 1..={MAX_QUERY_SCANNED_CHANGES}"
            ));
        }
        if self
            .graph_known_at_cursor
            .is_some_and(|cursor| self.graph_compare_cursor > cursor)
        {
            return invalid("diagnostic graph_compare_cursor exceeds graph_known_at_cursor");
        }
        if self.audit_limit == 0 || self.audit_limit > MAX_DIAGNOSTIC_AUDIT_RECORDS {
            return invalid(format!(
                "diagnostic audit_limit must be in 1..={MAX_DIAGNOSTIC_AUDIT_RECORDS}"
            ));
        }
        Ok(())
    }
}

/// The authoritative coordinates that remained stable while every diagnostic
/// section was assembled. RRD retries instead of returning a mixed-time view.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct DiagnosticReadStamp {
    pub claim_sequence: u64,
    pub control_sequence: u64,
    pub runtime_cursor: u64,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub schema_revision: Option<u64>,
    pub catalogue_revision: u64,
    pub runtime_manifest_sha256: String,
    pub retention_sha256: String,
    pub assembly_attempts: u8,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum DiagnosticAuthority {
    Authoritative,
    Projection,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum DiagnosticCoverage {
    Complete,
    Bounded,
}

/// Machine-readable disclosure for every section in the response. Connectome
/// renders these facts; it does not invent authority or completeness labels.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct DiagnosticSectionSnapshot {
    pub id: CanonicalId,
    pub authority: DiagnosticAuthority,
    pub coverage: DiagnosticCoverage,
    pub row_count: u64,
    pub known_at_cursor: u64,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub requested_after: Option<u64>,
}

#[derive(
    Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize, JsonSchema,
)]
#[serde(rename_all = "snake_case")]
pub enum DiagnosticModelKind {
    Record,
    Relation,
    Event,
}

/// One stable logical model summary derived from the authoritative schema.
/// Connectome may render the full schema, but it does not classify model kinds
/// or recompute constraint counts itself.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct DiagnosticModelSnapshot {
    pub id: CanonicalId,
    pub kind: DiagnosticModelKind,
    pub property_count: u64,
    pub required_property_count: u64,
    pub constraint_count: u64,
    pub allow_additional_properties: bool,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct DiagnosticModelCatalogueSnapshot {
    pub scope: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub schema_revision: Option<u64>,
    pub models: Vec<DiagnosticModelSnapshot>,
}

/// Lossless runtime identity used by diagnostics. Runtime IDs intentionally
/// admit causal/generation separators such as `:` and `@`; they are not the
/// URL-safe public resource IDs represented by `CanonicalId`.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct DiagnosticRuntimeReference {
    pub kind: String,
    pub id: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct DiagnosticGraphRecordSnapshot {
    pub reference: DiagnosticRuntimeReference,
    pub valid_from_unix_ms: u64,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub valid_to_unix_ms: Option<u64>,
    pub properties: DataProperties,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct DiagnosticGraphRelationSnapshot {
    pub reference: DiagnosticRuntimeReference,
    pub from: DiagnosticRuntimeReference,
    pub to: DiagnosticRuntimeReference,
    pub valid_from_unix_ms: u64,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub valid_to_unix_ms: Option<u64>,
    pub properties: DataProperties,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct DiagnosticGraphSnapshot {
    pub scope: String,
    pub valid_at_unix_ms: u64,
    pub known_at_cursor: u64,
    pub records: Vec<DiagnosticGraphRecordSnapshot>,
    pub relations: Vec<DiagnosticGraphRelationSnapshot>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct DiagnosticGraphRecordChange {
    pub before: DiagnosticGraphRecordSnapshot,
    pub after: DiagnosticGraphRecordSnapshot,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct DiagnosticGraphRelationChange {
    pub before: DiagnosticGraphRelationSnapshot,
    pub after: DiagnosticGraphRelationSnapshot,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct DiagnosticGraphDifference {
    pub valid_at_unix_ms: u64,
    pub from_cursor: u64,
    pub to_cursor: u64,
    pub added_records: Vec<DiagnosticGraphRecordSnapshot>,
    pub removed_records: Vec<DiagnosticGraphRecordSnapshot>,
    pub changed_records: Vec<DiagnosticGraphRecordChange>,
    pub added_relations: Vec<DiagnosticGraphRelationSnapshot>,
    pub removed_relations: Vec<DiagnosticGraphRelationSnapshot>,
    pub changed_relations: Vec<DiagnosticGraphRelationChange>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct DiagnosticSnapshotLease {
    pub id_sha256: String,
    pub scope: String,
    pub owner: String,
    pub created_at_unix_ms: u64,
    pub expires_at_unix_ms: u64,
    pub runtime_cursor: u64,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub schema_revision: Option<u64>,
    pub catalogue_revision: u64,
    pub runtime_manifest_sha256: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct DiagnosticRetentionPin {
    pub id_sha256: String,
    pub snapshot_id_sha256: String,
    pub scope: String,
    pub runtime_manifest_sha256: String,
    pub minimum_cursor: u64,
    pub expires_at_unix_ms: u64,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct DiagnosticRetentionSnapshot {
    pub observed_at_unix_ms: u64,
    pub leases: Vec<DiagnosticSnapshotLease>,
    pub pins: Vec<DiagnosticRetentionPin>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub oldest_retained_cursor: Option<u64>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum DiagnosticVectorArtifactKind {
    ExactSegment,
    CompactDense,
    Hnsw,
    TurboQuant,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct DiagnosticVectorArtifactSnapshot {
    pub catalogue_revision: u64,
    pub kind: DiagnosticVectorArtifactKind,
    pub scope: String,
    pub projection_id: String,
    pub generation: u64,
    pub source_cursor: u64,
    pub config_sha256: String,
    pub artifact_sha256: String,
    pub object: DiagnosticRuntimeReference,
    pub object_sha256: String,
    pub object_length: u64,
    pub media_type: String,
    pub receipt: DataObjectReceipt,
    pub published_at_unix_ms: u64,
    pub entry_sha256: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct DiagnosticVectorArtifactCatalogueSnapshot {
    pub scope: String,
    pub revision: u64,
    pub artifacts: Vec<DiagnosticVectorArtifactSnapshot>,
}

/// First cohesive RRD diagnostic projection. All fields are assembled by the
/// engine under one verified read stamp and are identical through embedded and
/// daemon faces. Additional graph/reasoning/trace lenses extend this contract;
/// they must not be reconstructed by Connectome from physical crates.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct DiagnosticSnapshot {
    pub format_version: u16,
    pub observed_at_unix_ms: u64,
    pub instance: ResourceId,
    pub scope: String,
    pub read: DiagnosticReadStamp,
    pub readiness: Readiness,
    pub product_capabilities: ProductCapabilityCatalogue,
    pub runtime_tools: RuntimeToolCatalogue,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub schema: Option<DataSchemaRegistry>,
    pub models: DiagnosticModelCatalogueSnapshot,
    pub graph: DiagnosticGraphSnapshot,
    pub graph_difference: DiagnosticGraphDifference,
    pub retention: DiagnosticRetentionSnapshot,
    pub vector_artifacts: DiagnosticVectorArtifactCatalogueSnapshot,
    pub query_indexes: QueryIndexCatalogueSnapshot,
    pub vector_collections: VectorCollectionCatalogueSnapshot,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub estate: Option<EstateSnapshot>,
    pub changes: ChangefeedPage,
    pub audit: AuditPage,
    pub sections: Vec<DiagnosticSectionSnapshot>,
}

impl DiagnosticSnapshot {
    pub fn validate(&self) -> crate::Result<()> {
        if self.format_version != DIAGNOSTIC_SNAPSHOT_FORMAT_VERSION {
            return invalid(format!(
                "unsupported diagnostic snapshot format {}; expected {}",
                self.format_version, DIAGNOSTIC_SNAPSHOT_FORMAT_VERSION
            ));
        }
        if self.observed_at_unix_ms == 0 {
            return invalid("diagnostic observation time must be greater than zero");
        }
        if self.scope.is_empty()
            || self.scope.len() > crate::MAX_ID_BYTES
            || self.scope.as_bytes().contains(&0)
        {
            return invalid("diagnostic scope is invalid");
        }
        if self.read.assembly_attempts == 0 {
            return invalid("diagnostic assembly attempts must be greater than zero");
        }
        crate::validate_sha256(
            &self.read.runtime_manifest_sha256,
            "runtime_manifest_sha256",
        )?;
        crate::validate_sha256(&self.read.retention_sha256, "retention_sha256")?;
        if self.read.claim_sequence != self.readiness.claim_sequence
            || self.read.runtime_cursor != self.readiness.runtime_cursor
        {
            return invalid("diagnostic read stamp differs from readiness coordinates");
        }
        if self.changes.head_cursor != self.read.runtime_cursor
            || self.changes.through_cursor > self.read.runtime_cursor
        {
            return invalid("diagnostic changefeed differs from the verified runtime cursor");
        }
        self.readiness.validate()?;
        self.product_capabilities.validate()?;
        self.runtime_tools.validate()?;
        if self.models.scope != self.scope
            || self.models.schema_revision != self.read.schema_revision
            || self.schema.as_ref().map(|schema| schema.revision) != self.read.schema_revision
        {
            return invalid("diagnostic schema/model coordinates differ from the read stamp");
        }
        if self.graph.scope != self.scope
            || self.graph.known_at_cursor > self.read.runtime_cursor
            || self.graph_difference.valid_at_unix_ms != self.graph.valid_at_unix_ms
            || self.graph_difference.to_cursor != self.graph.known_at_cursor
            || self.graph_difference.from_cursor > self.graph_difference.to_cursor
        {
            return invalid("diagnostic graph coordinates differ from the read stamp");
        }
        validate_graph_records(&self.graph.records, "diagnostic graph records")?;
        validate_graph_relations(&self.graph.relations, "diagnostic graph relations")?;
        validate_graph_difference(&self.graph_difference)?;
        if self.retention.observed_at_unix_ms != self.observed_at_unix_ms
            || self
                .retention
                .leases
                .iter()
                .any(|lease| lease.runtime_cursor > self.read.runtime_cursor)
        {
            return invalid("diagnostic retention coordinates differ from the read stamp");
        }
        validate_retention(&self.retention, &self.read.retention_sha256)?;
        validate_vector_artifacts(
            &self.vector_artifacts,
            &self.scope,
            self.read.runtime_cursor,
        )?;
        let mut model_ids = std::collections::BTreeSet::new();
        for model in &self.models.models {
            if model.required_property_count > model.property_count
                || !model_ids.insert((model.kind, model.id.as_str()))
            {
                return invalid("diagnostic model summary is invalid or duplicated");
            }
        }
        if self
            .models
            .models
            .windows(2)
            .any(|pair| (pair[0].kind, &pair[0].id) >= (pair[1].kind, &pair[1].id))
        {
            return invalid("diagnostic models must be sorted by kind and id");
        }
        let mut section_ids = std::collections::BTreeSet::new();
        for section in &self.sections {
            if !section_ids.insert(section.id.as_str()) {
                return invalid("diagnostic section identities must be unique");
            }
            if section.known_at_cursor > self.read.runtime_cursor {
                return invalid("diagnostic section exceeds the verified runtime cursor");
            }
        }
        Ok(())
    }
}

fn validate_retention(
    retention: &DiagnosticRetentionSnapshot,
    expected_sha256: &str,
) -> crate::Result<()> {
    let encoded =
        serde_json::to_vec(retention).map_err(|error| crate::ContractError(error.to_string()))?;
    let computed = sha256_hex(&encoded);
    if computed != expected_sha256 {
        return invalid("diagnostic retention digest differs from the read stamp");
    }
    let mut leases_by_id = std::collections::BTreeMap::new();
    for lease in &retention.leases {
        crate::validate_sha256(&lease.id_sha256, "retention lease id")?;
        crate::validate_sha256(
            &lease.runtime_manifest_sha256,
            "retention lease runtime manifest",
        )?;
        if lease.scope.is_empty()
            || lease.owner.is_empty()
            || lease.expires_at_unix_ms <= lease.created_at_unix_ms
            || lease.expires_at_unix_ms <= retention.observed_at_unix_ms
            || leases_by_id
                .insert(lease.id_sha256.as_str(), lease)
                .is_some()
        {
            return invalid("diagnostic retention lease is invalid or duplicated");
        }
    }
    if retention
        .leases
        .windows(2)
        .any(|pair| pair[0].id_sha256 >= pair[1].id_sha256)
    {
        return invalid("diagnostic retention leases must be sorted by identity");
    }

    let mut pin_ids = std::collections::BTreeSet::new();
    for pin in &retention.pins {
        crate::validate_sha256(&pin.id_sha256, "retention pin id")?;
        crate::validate_sha256(&pin.snapshot_id_sha256, "retention pin snapshot id")?;
        crate::validate_sha256(
            &pin.runtime_manifest_sha256,
            "retention pin runtime manifest",
        )?;
        if pin.scope.is_empty()
            || pin.expires_at_unix_ms <= retention.observed_at_unix_ms
            || !pin_ids.insert(pin.id_sha256.as_str())
        {
            return invalid("diagnostic retention pin is invalid or duplicated");
        }
        let Some(lease) = leases_by_id.get(pin.snapshot_id_sha256.as_str()) else {
            return invalid("diagnostic retention pin has no matching lease");
        };
        if pin.scope != lease.scope
            || pin.runtime_manifest_sha256 != lease.runtime_manifest_sha256
            || pin.minimum_cursor != lease.runtime_cursor
            || pin.expires_at_unix_ms != lease.expires_at_unix_ms
        {
            return invalid("diagnostic retention pin differs from its lease");
        }
    }
    if retention
        .pins
        .windows(2)
        .any(|pair| pair[0].id_sha256 >= pair[1].id_sha256)
    {
        return invalid("diagnostic retention pins must be sorted by identity");
    }
    if retention.pins.len() != retention.leases.len() {
        return invalid("diagnostic retention requires one pin per live lease");
    }
    if retention.oldest_retained_cursor != retention.pins.iter().map(|pin| pin.minimum_cursor).min()
    {
        return invalid("diagnostic oldest retained cursor differs from its pins");
    }
    Ok(())
}

fn sha256_hex(bytes: &[u8]) -> String {
    let digest = Sha256::digest(bytes);
    let mut output = String::with_capacity(64);
    const HEX: &[u8; 16] = b"0123456789abcdef";
    for byte in digest {
        output.push(HEX[usize::from(byte >> 4)] as char);
        output.push(HEX[usize::from(byte & 0x0f)] as char);
    }
    output
}

fn validate_vector_artifacts(
    catalogue: &DiagnosticVectorArtifactCatalogueSnapshot,
    scope: &str,
    runtime_cursor: u64,
) -> crate::Result<()> {
    if catalogue.scope != scope
        || catalogue.revision
            != u64::try_from(catalogue.artifacts.len())
                .map_err(|_| crate::ContractError("vector artifact count exceeds u64".into()))?
    {
        return invalid("diagnostic vector artifact catalogue coordinates are invalid");
    }
    for (index, artifact) in catalogue.artifacts.iter().enumerate() {
        crate::validate_sha256(&artifact.config_sha256, "vector artifact configuration")?;
        crate::validate_sha256(&artifact.artifact_sha256, "vector artifact content")?;
        crate::validate_sha256(&artifact.object_sha256, "vector artifact object")?;
        crate::validate_sha256(&artifact.entry_sha256, "vector artifact entry")?;
        validate_runtime_reference(&artifact.object)?;
        let expected_revision = u64::try_from(index)
            .map_err(|_| crate::ContractError("vector artifact ordinal exceeds u64".into()))?
            .saturating_add(1);
        if artifact.scope != scope
            || artifact.projection_id.is_empty()
            || artifact.projection_id.len() > MAX_MESSAGE_BYTES
            || artifact.projection_id.as_bytes().contains(&0)
            || artifact.catalogue_revision != expected_revision
            || artifact.generation == 0
            || artifact.source_cursor > runtime_cursor
            || artifact.object_length == 0
            || artifact.media_type.is_empty()
            || artifact.receipt.backend.is_empty()
            || artifact.receipt.key.is_empty()
        {
            return invalid("diagnostic vector artifact entry is invalid or out of order");
        }
    }
    Ok(())
}

fn validate_graph_records(
    records: &[DiagnosticGraphRecordSnapshot],
    label: &str,
) -> crate::Result<()> {
    let mut identities = std::collections::BTreeSet::new();
    for record in records {
        validate_runtime_reference(&record.reference)?;
        if record
            .valid_to_unix_ms
            .is_some_and(|valid_to| valid_to <= record.valid_from_unix_ms)
            || !identities.insert(&record.reference)
        {
            return invalid(format!("{label} contain an invalid or duplicate record"));
        }
    }
    if records
        .windows(2)
        .any(|pair| pair[0].reference >= pair[1].reference)
    {
        return invalid(format!("{label} must be sorted by identity"));
    }
    Ok(())
}

fn validate_graph_relations(
    relations: &[DiagnosticGraphRelationSnapshot],
    label: &str,
) -> crate::Result<()> {
    let mut identities = std::collections::BTreeSet::new();
    for relation in relations {
        validate_runtime_reference(&relation.reference)?;
        validate_runtime_reference(&relation.from)?;
        validate_runtime_reference(&relation.to)?;
        if relation
            .valid_to_unix_ms
            .is_some_and(|valid_to| valid_to <= relation.valid_from_unix_ms)
            || !identities.insert(&relation.reference)
        {
            return invalid(format!("{label} contain an invalid or duplicate relation"));
        }
    }
    if relations
        .windows(2)
        .any(|pair| pair[0].reference >= pair[1].reference)
    {
        return invalid(format!("{label} must be sorted by identity"));
    }
    Ok(())
}

fn validate_runtime_reference(reference: &DiagnosticRuntimeReference) -> crate::Result<()> {
    if reference.kind.is_empty()
        || reference.kind.len() > MAX_MESSAGE_BYTES
        || reference.kind.as_bytes().contains(&0)
        || reference.id.is_empty()
        || reference.id.len() > MAX_MESSAGE_BYTES
        || reference.id.as_bytes().contains(&0)
    {
        return invalid("diagnostic runtime reference is empty, oversized, or contains NUL");
    }
    Ok(())
}

fn validate_graph_difference(difference: &DiagnosticGraphDifference) -> crate::Result<()> {
    validate_graph_records(
        &difference.added_records,
        "diagnostic graph difference added records",
    )?;
    validate_graph_records(
        &difference.removed_records,
        "diagnostic graph difference removed records",
    )?;
    validate_graph_relations(
        &difference.added_relations,
        "diagnostic graph difference added relations",
    )?;
    validate_graph_relations(
        &difference.removed_relations,
        "diagnostic graph difference removed relations",
    )?;

    let mut record_identities = difference
        .added_records
        .iter()
        .chain(&difference.removed_records)
        .map(|record| &record.reference)
        .collect::<std::collections::BTreeSet<_>>();
    if record_identities.len()
        != difference
            .added_records
            .len()
            .saturating_add(difference.removed_records.len())
    {
        return invalid("diagnostic graph record difference categories overlap");
    }
    for change in &difference.changed_records {
        validate_graph_records(
            std::slice::from_ref(&change.before),
            "diagnostic graph difference changed record before",
        )?;
        validate_graph_records(
            std::slice::from_ref(&change.after),
            "diagnostic graph difference changed record after",
        )?;
        if change.before.reference != change.after.reference
            || change.before == change.after
            || !record_identities.insert(&change.after.reference)
        {
            return invalid("diagnostic graph changed record identity is invalid or overlaps");
        }
    }
    if difference
        .changed_records
        .windows(2)
        .any(|pair| pair[0].after.reference >= pair[1].after.reference)
    {
        return invalid("diagnostic graph changed records must be sorted by identity");
    }

    let mut relation_identities = difference
        .added_relations
        .iter()
        .chain(&difference.removed_relations)
        .map(|relation| &relation.reference)
        .collect::<std::collections::BTreeSet<_>>();
    if relation_identities.len()
        != difference
            .added_relations
            .len()
            .saturating_add(difference.removed_relations.len())
    {
        return invalid("diagnostic graph relation difference categories overlap");
    }
    for change in &difference.changed_relations {
        validate_graph_relations(
            std::slice::from_ref(&change.before),
            "diagnostic graph difference changed relation before",
        )?;
        validate_graph_relations(
            std::slice::from_ref(&change.after),
            "diagnostic graph difference changed relation after",
        )?;
        if change.before.reference != change.after.reference
            || change.before == change.after
            || !relation_identities.insert(&change.after.reference)
        {
            return invalid("diagnostic graph changed relation identity is invalid or overlaps");
        }
    }
    if difference
        .changed_relations
        .windows(2)
        .any(|pair| pair[0].after.reference >= pair[1].after.reference)
    {
        return invalid("diagnostic graph changed relations must be sorted by identity");
    }
    Ok(())
}
