use super::validation::{sha256, text};
use crate::{invalid, Result};
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};
use std::collections::BTreeSet;

pub const ARCHITECTURE_ASSESSMENT_FORMAT_VERSION: u16 = 1;
pub const GOLDEN_PATTERN_REGISTRY_REVISION: u64 = 1;
const MAX_ITEMS: usize = 128;

#[derive(
    Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize, JsonSchema,
)]
#[serde(rename_all = "snake_case")]
pub enum ArchitectureEvidenceKindV1 {
    Topology,
    Profile,
    SourceTree,
    File,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct ArchitectureEvidenceV1 {
    pub kind: ArchitectureEvidenceKindV1,
    pub source: String,
    pub sha256: String,
    pub summary: String,
}
impl ArchitectureEvidenceV1 {
    pub fn validate(&self) -> Result<()> {
        text("architecture evidence source", &self.source)?;
        sha256("architecture evidence", &self.sha256)?;
        text("architecture evidence summary", &self.summary)
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(tag = "status", rename_all = "snake_case", deny_unknown_fields)]
pub enum ArchitectureAnswerV1<T> {
    Known {
        answer: T,
        evidence: Vec<ArchitectureEvidenceV1>,
    },
    Unknown {
        missing_evidence: Vec<String>,
        discovery_action: String,
    },
}
impl<T> ArchitectureAnswerV1<T> {
    pub fn known(&self) -> Option<&T> {
        match self {
            Self::Known { answer, .. } => Some(answer),
            Self::Unknown { .. } => None,
        }
    }
    pub fn evidence(&self) -> &[ArchitectureEvidenceV1] {
        match self {
            Self::Known { evidence, .. } => evidence,
            Self::Unknown { .. } => &[],
        }
    }
    pub fn is_known(&self) -> bool {
        matches!(self, Self::Known { .. })
    }
    fn validate_with(&self, name: &str, validate: impl FnOnce(&T) -> Result<()>) -> Result<()> {
        match self {
            Self::Known { answer, evidence } => {
                if evidence.is_empty() || evidence.len() > 32 {
                    return invalid(format!("architecture {name} requires bounded evidence"));
                }
                let mut unique = BTreeSet::new();
                for item in evidence {
                    item.validate()?;
                    if !unique.insert((item.kind, item.source.as_str())) {
                        return invalid(format!("architecture {name} repeats evidence"));
                    }
                }
                validate(answer)
            }
            Self::Unknown {
                missing_evidence,
                discovery_action,
            } => {
                sorted_texts(
                    &format!("architecture {name} missing evidence"),
                    missing_evidence,
                    false,
                )?;
                text(
                    &format!("architecture {name} discovery action"),
                    discovery_action,
                )
            }
        }
    }
}

#[derive(
    Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize, JsonSchema,
)]
#[serde(rename_all = "snake_case")]
pub enum ArchitectureBoundaryKindV1 {
    Component,
    Application,
    Workspace,
}
impl ArchitectureBoundaryKindV1 {
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Component => "component",
            Self::Application => "application",
            Self::Workspace => "workspace",
        }
    }
}
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum ArchitectureBoundaryDecisionV1 {
    ExtendExisting,
    CreateJustified,
}
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum ArchitectureImpactDispositionV1 {
    None,
    Contained,
    CrossesBoundary,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct CapabilityOwnershipAnswerV1 {
    pub capability: String,
    pub owner_boundary: String,
}
impl CapabilityOwnershipAnswerV1 {
    fn validate(&self) -> Result<()> {
        text("owned capability", &self.capability)?;
        text("capability owner", &self.owner_boundary)
    }
}
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct BoundaryChoiceAnswerV1 {
    pub decision: ArchitectureBoundaryDecisionV1,
    pub kind: ArchitectureBoundaryKindV1,
    pub boundary: String,
    pub justification: String,
}
impl BoundaryChoiceAnswerV1 {
    fn validate(&self) -> Result<()> {
        text("chosen boundary", &self.boundary)?;
        text("boundary justification", &self.justification)
    }
}
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct PublicContractAnswerV1 {
    pub public_contract: String,
    pub dependency_direction: String,
}
impl PublicContractAnswerV1 {
    fn validate(&self) -> Result<()> {
        text("public contract", &self.public_contract)?;
        text("dependency direction", &self.dependency_direction)
    }
}
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct RejectedGoldenPatternV1 {
    pub pattern_id: String,
    pub rationale: String,
}
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct PatternChoiceAnswerV1 {
    pub candidate_pattern_ids: Vec<String>,
    pub rejected_candidates: Vec<RejectedGoldenPatternV1>,
}
impl PatternChoiceAnswerV1 {
    fn validate(&self) -> Result<()> {
        sorted_texts("pattern candidates", &self.candidate_pattern_ids, false)?;
        let ids = self
            .rejected_candidates
            .iter()
            .map(|v| v.pattern_id.clone())
            .collect::<Vec<_>>();
        sorted_texts("rejected patterns", &ids, true)?;
        for value in &self.rejected_candidates {
            text("rejected pattern rationale", &value.rationale)?;
        }
        Ok(())
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct ArchitectureImplicationV1 {
    pub disposition: ArchitectureImpactDispositionV1,
    pub detail: String,
}
impl ArchitectureImplicationV1 {
    fn validate(&self, name: &str) -> Result<()> {
        text(name, &self.detail)
    }
}
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct CrossBoundaryImplicationsAnswerV1 {
    pub persistence: ArchitectureImplicationV1,
    pub transaction: ArchitectureImplicationV1,
    pub security: ArchitectureImplicationV1,
    pub audit: ArchitectureImplicationV1,
    pub realtime: ArchitectureImplicationV1,
    pub recovery: ArchitectureImplicationV1,
    pub observability: ArchitectureImplicationV1,
}
impl CrossBoundaryImplicationsAnswerV1 {
    fn validate(&self) -> Result<()> {
        self.persistence.validate("persistence implication")?;
        self.transaction.validate("transaction implication")?;
        self.security.validate("security implication")?;
        self.audit.validate("audit implication")?;
        self.realtime.validate("realtime implication")?;
        self.recovery.validate("recovery implication")?;
        self.observability.validate("observability implication")
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct PathOwnershipAnswerV1 {
    pub existing_paths: Vec<String>,
    pub justified_new_paths: Vec<String>,
    pub oversized_boundary_prevention: String,
    pub cyclic_ownership_prevention: String,
}
impl PathOwnershipAnswerV1 {
    fn validate(&self) -> Result<()> {
        sorted_texts("existing paths", &self.existing_paths, true)?;
        sorted_texts("new paths", &self.justified_new_paths, true)?;
        if self.existing_paths.is_empty() && self.justified_new_paths.is_empty() {
            return invalid("path ownership requires a path");
        }
        text(
            "oversized boundary prevention",
            &self.oversized_boundary_prevention,
        )?;
        text(
            "cyclic ownership prevention",
            &self.cyclic_ownership_prevention,
        )
    }
}
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct VerificationMatrixAnswerV1 {
    pub exact_argv: Vec<Vec<String>>,
    pub platforms: Vec<String>,
}
impl VerificationMatrixAnswerV1 {
    fn validate(&self) -> Result<()> {
        if self.exact_argv.is_empty() || self.exact_argv.len() > MAX_ITEMS {
            return invalid("verification matrix requires exact argv");
        }
        for argv in &self.exact_argv {
            if argv.is_empty() || argv.len() > 256 {
                return invalid("invalid verification argv");
            }
            for arg in argv {
                text("verification argument", arg)?;
            }
        }
        sorted_texts("verification platforms", &self.platforms, false)
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct ArchitectureAssessmentV1 {
    pub format: u16,
    pub task_sha256: String,
    pub topology_sha256: String,
    pub profile_sha256: String,
    pub source_tree_sha256: String,
    pub capability_ownership: ArchitectureAnswerV1<CapabilityOwnershipAnswerV1>,
    pub boundary_choice: ArchitectureAnswerV1<BoundaryChoiceAnswerV1>,
    pub public_contract: ArchitectureAnswerV1<PublicContractAnswerV1>,
    pub pattern_choice: ArchitectureAnswerV1<PatternChoiceAnswerV1>,
    pub cross_boundary_implications: ArchitectureAnswerV1<CrossBoundaryImplicationsAnswerV1>,
    pub path_ownership: ArchitectureAnswerV1<PathOwnershipAnswerV1>,
    pub verification_matrix: ArchitectureAnswerV1<VerificationMatrixAnswerV1>,
    pub assessment_sha256: String,
}
impl ArchitectureAssessmentV1 {
    pub fn validate_answers(&self) -> Result<()> {
        if self.format != ARCHITECTURE_ASSESSMENT_FORMAT_VERSION {
            return invalid("unsupported architecture assessment format");
        }
        sha256("architecture task", &self.task_sha256)?;
        sha256("architecture topology", &self.topology_sha256)?;
        sha256("architecture profile", &self.profile_sha256)?;
        sha256("architecture source tree", &self.source_tree_sha256)?;
        self.capability_ownership.validate_with(
            "capability ownership",
            CapabilityOwnershipAnswerV1::validate,
        )?;
        self.boundary_choice
            .validate_with("boundary choice", BoundaryChoiceAnswerV1::validate)?;
        self.public_contract
            .validate_with("public contract", PublicContractAnswerV1::validate)?;
        self.pattern_choice
            .validate_with("pattern choice", PatternChoiceAnswerV1::validate)?;
        self.cross_boundary_implications.validate_with(
            "cross-boundary implications",
            CrossBoundaryImplicationsAnswerV1::validate,
        )?;
        self.path_ownership
            .validate_with("path ownership", PathOwnershipAnswerV1::validate)?;
        self.verification_matrix
            .validate_with("verification matrix", VerificationMatrixAnswerV1::validate)
    }
    pub fn validate(&self) -> Result<()> {
        self.validate_answers()?;
        sha256("architecture assessment", &self.assessment_sha256)
    }
    pub fn all_answers_known(&self) -> bool {
        self.capability_ownership.is_known()
            && self.boundary_choice.is_known()
            && self.public_contract.is_known()
            && self.pattern_choice.is_known()
            && self.cross_boundary_implications.is_known()
            && self.path_ownership.is_known()
            && self.verification_matrix.is_known()
    }
    pub fn evidence(&self) -> Vec<&ArchitectureEvidenceV1> {
        let mut all = Vec::new();
        all.extend(self.capability_ownership.evidence());
        all.extend(self.boundary_choice.evidence());
        all.extend(self.public_contract.evidence());
        all.extend(self.pattern_choice.evidence());
        all.extend(self.cross_boundary_implications.evidence());
        all.extend(self.path_ownership.evidence());
        all.extend(self.verification_matrix.evidence());
        all
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct SelectedGoldenPatternV1 {
    pub pattern_id: String,
    pub revision: u64,
    pub evidence_sources: Vec<String>,
    pub composed_facet: bool,
}
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct GoldenPatternSelectionV1 {
    pub registry_revision: u64,
    pub task_sha256: String,
    pub topology_sha256: String,
    pub profile_sha256: String,
    pub source_tree_sha256: String,
    pub assessment_sha256: String,
    pub selected: Vec<SelectedGoldenPatternV1>,
    pub selection_sha256: String,
}
impl GoldenPatternSelectionV1 {
    pub fn validate(&self) -> Result<()> {
        if self.registry_revision != GOLDEN_PATTERN_REGISTRY_REVISION {
            return invalid("unsupported pattern registry revision");
        }
        for (name, value) in [
            ("pattern task", &self.task_sha256),
            ("pattern topology", &self.topology_sha256),
            ("pattern profile", &self.profile_sha256),
            ("pattern source tree", &self.source_tree_sha256),
            ("pattern assessment", &self.assessment_sha256),
            ("pattern selection", &self.selection_sha256),
        ] {
            sha256(name, value)?;
        }
        if self.selected.is_empty() || self.selected.len() > MAX_ITEMS {
            return invalid("pattern selection requires reviewed patterns");
        }
        let ids = self
            .selected
            .iter()
            .map(|v| v.pattern_id.clone())
            .collect::<Vec<_>>();
        sorted_texts("selected patterns", &ids, false)?;
        for value in &self.selected {
            if value.revision == 0 {
                return invalid("pattern revision must be non-zero");
            }
            sorted_texts("pattern evidence sources", &value.evidence_sources, false)?;
        }
        Ok(())
    }
}

fn sorted_texts(name: &str, values: &[String], allow_empty: bool) -> Result<()> {
    if (!allow_empty && values.is_empty()) || values.len() > MAX_ITEMS {
        return invalid(format!("{name} has invalid item count"));
    }
    let mut previous = None;
    for value in values {
        text(name, value)?;
        if previous.is_some_and(|prior: &String| prior >= value) {
            return invalid(format!("{name} must be sorted and unique"));
        }
        previous = Some(value);
    }
    Ok(())
}
