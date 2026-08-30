//! Evidence-backed architecture review and reviewed golden-pattern selection.
use super::{ensure_routing_fresh, load_project_artifacts, require_fresh_attunement};
use rrd_core::digest;
use rrd_graph::{Ecosystem, ProjectProfile};
use rrd_store::Engine;
use serde::{Deserialize, Serialize};
use std::collections::{BTreeMap, BTreeSet};
use std::path::{Component, Path};

pub use rrd_contract::{
    ArchitectureAnswerV1, ArchitectureAssessmentV1, ArchitectureBoundaryDecisionV1,
    ArchitectureBoundaryKindV1, ArchitectureEvidenceKindV1, ArchitectureEvidenceV1,
    ArchitectureImpactDispositionV1, ArchitectureImplicationV1, BoundaryChoiceAnswerV1,
    CapabilityOwnershipAnswerV1, CrossBoundaryImplicationsAnswerV1, GoldenPatternSelectionV1,
    PathOwnershipAnswerV1, PatternChoiceAnswerV1, PublicContractAnswerV1, RejectedGoldenPatternV1,
    SelectedGoldenPatternV1, VerificationMatrixAnswerV1, ARCHITECTURE_ASSESSMENT_FORMAT_VERSION,
    GOLDEN_PATTERN_REGISTRY_REVISION,
};

pub const ARCHITECTURE_REVIEW_FORMAT: u16 = 1;
pub const GOLDEN_PATTERN_SELECTION_PROJECTION: &str = "golden-pattern-selection-v1";
const REVIEW_SOURCE: &str = "docs/rrd-engine-foundation-cheat-sheet.md#initial-stack-packs";

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ReviewedGoldenPattern {
    pub pattern_id: String,
    pub revision: u64,
    pub ecosystem: Option<String>,
    pub boundary_kind: ArchitectureBoundaryKindV1,
    pub composed_facet: bool,
    pub evidence_sources: Vec<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ArchitectureReview {
    pub format: u16,
    pub assessment: ArchitectureAssessmentV1,
    pub selection: GoldenPatternSelectionV1,
    pub reviewed_at: u64,
    pub review_sha256: String,
}

impl ArchitectureReview {
    fn seal(&mut self) {
        self.review_sha256.clear();
        self.review_sha256 = sealed_digest(b"rrflow-architecture-review-v1\0", self);
    }

    pub fn verify(&self) -> bool {
        if self.format != ARCHITECTURE_REVIEW_FORMAT
            || self.assessment.validate().is_err()
            || self.selection.validate().is_err()
            || self.assessment.assessment_sha256 != self.selection.assessment_sha256
            || self.assessment.task_sha256 != self.selection.task_sha256
            || self.assessment.topology_sha256 != self.selection.topology_sha256
            || self.assessment.profile_sha256 != self.selection.profile_sha256
            || self.assessment.source_tree_sha256 != self.selection.source_tree_sha256
        {
            return false;
        }
        let mut assessment = self.assessment.clone();
        let expected_assessment = assessment.assessment_sha256.clone();
        seal_assessment(&mut assessment);
        let mut selection = self.selection.clone();
        let expected_selection = selection.selection_sha256.clone();
        seal_selection(&mut selection);
        let mut review = self.clone();
        let expected_review = review.review_sha256.clone();
        review.seal();
        expected_assessment == assessment.assessment_sha256
            && expected_selection == selection.selection_sha256
            && expected_review == review.review_sha256
    }
}

pub fn reviewed_golden_patterns() -> Vec<ReviewedGoldenPattern> {
    let families = [
        ("rust", "cargo"),
        ("typescript", "javascript"),
        ("python", "python"),
        ("go", "go"),
        ("jvm", "jvm"),
        ("dotnet", "dotnet"),
        ("cpp", "cmake"),
    ];
    let kinds = [
        ArchitectureBoundaryKindV1::Component,
        ArchitectureBoundaryKindV1::Application,
        ArchitectureBoundaryKindV1::Workspace,
    ];
    let mut patterns = Vec::new();
    for (family, ecosystem) in families {
        for kind in kinds {
            patterns.push(ReviewedGoldenPattern {
                pattern_id: format!("{family}/{}", kind.as_str()),
                revision: GOLDEN_PATTERN_REGISTRY_REVISION,
                ecosystem: Some(ecosystem.into()),
                boundary_kind: kind,
                composed_facet: false,
                evidence_sources: vec![REVIEW_SOURCE.into()],
            });
        }
    }
    patterns.push(ReviewedGoldenPattern {
        pattern_id: "polyglot/protocol".into(),
        revision: GOLDEN_PATTERN_REGISTRY_REVISION,
        ecosystem: None,
        boundary_kind: ArchitectureBoundaryKindV1::Component,
        composed_facet: true,
        evidence_sources: vec![REVIEW_SOURCE.into()],
    });
    patterns.sort_by(|left, right| left.pattern_id.cmp(&right.pattern_id));
    patterns
}

pub fn review_architecture<E: Engine>(
    store: &E,
    root: &Path,
    task: &str,
    mut assessment: ArchitectureAssessmentV1,
    now: u64,
) -> Result<ArchitectureReview, Box<dyn std::error::Error>> {
    let root = std::fs::canonicalize(root)?;
    let ready = ensure_routing_fresh(store, &root)?;
    let attunement = require_fresh_attunement(store, &root, &ready)?;
    let (topology, profile) =
        load_project_artifacts(store, &root)?.ok_or("project topology/profile is absent")?;
    assessment.validate_answers()?;
    if !assessment.all_answers_known() {
        return Err("architecture assessment requires bounded discovery before selection".into());
    }
    if assessment.task_sha256 != task_sha256(task)
        || assessment.topology_sha256 != topology.digest
        || assessment.profile_sha256 != profile.digest
        || assessment.source_tree_sha256 != attunement.source_tree_sha256
    {
        return Err("architecture assessment is stale against task or project attunement".into());
    }
    validate_evidence(
        &root,
        &assessment,
        &topology.digest,
        &profile.digest,
        &attunement.source_tree_sha256,
    )?;
    seal_assessment(&mut assessment);
    let selected = select_patterns(&profile, &assessment)?;
    let mut selection = GoldenPatternSelectionV1 {
        registry_revision: GOLDEN_PATTERN_REGISTRY_REVISION,
        task_sha256: assessment.task_sha256.clone(),
        topology_sha256: assessment.topology_sha256.clone(),
        profile_sha256: assessment.profile_sha256.clone(),
        source_tree_sha256: assessment.source_tree_sha256.clone(),
        assessment_sha256: assessment.assessment_sha256.clone(),
        selected,
        selection_sha256: String::new(),
    };
    seal_selection(&mut selection);
    selection.validate()?;
    let mut review = ArchitectureReview {
        format: ARCHITECTURE_REVIEW_FORMAT,
        assessment,
        selection,
        reviewed_at: now,
        review_sha256: String::new(),
    };
    review.seal();
    if !review.verify() {
        return Err("architecture review failed self-verification".into());
    }
    store.put_projection(
        GOLDEN_PATTERN_SELECTION_PROJECTION,
        &serde_json::to_vec(&review)?,
    )?;
    Ok(review)
}

pub fn load_architecture_review<E: Engine>(
    store: &E,
) -> Result<Option<ArchitectureReview>, Box<dyn std::error::Error>> {
    store
        .get_projection(GOLDEN_PATTERN_SELECTION_PROJECTION)?
        .map(|bytes| {
            let review: ArchitectureReview = serde_json::from_slice(&bytes)
                .map_err(|error| format!("architecture review is unreadable: {error}"))?;
            if !review.verify() {
                return Err("architecture review failed digest verification".into());
            }
            Ok(review)
        })
        .transpose()
}

fn select_patterns(
    profile: &ProjectProfile,
    assessment: &ArchitectureAssessmentV1,
) -> Result<Vec<SelectedGoldenPatternV1>, Box<dyn std::error::Error>> {
    let boundary = assessment
        .boundary_choice
        .known()
        .ok_or("boundary answer unresolved")?;
    let choices = assessment
        .pattern_choice
        .known()
        .ok_or("pattern answer unresolved")?;
    let families = profile
        .ecosystems
        .iter()
        .filter_map(|value| match value {
            Ecosystem::Cargo => Some("rust"),
            Ecosystem::JavaScript => Some("typescript"),
            Ecosystem::Python => Some("python"),
            Ecosystem::Go => Some("go"),
            Ecosystem::Jvm => Some("jvm"),
            Ecosystem::Dotnet => Some("dotnet"),
            Ecosystem::Cmake => Some("cpp"),
            Ecosystem::Ci => None,
        })
        .collect::<BTreeSet<_>>();
    if families.is_empty() {
        return Err("project profile has no reviewed stack family".into());
    }
    let mut expected = families
        .iter()
        .map(|family| format!("{family}/{}", boundary.kind.as_str()))
        .collect::<BTreeSet<_>>();
    if families.len() > 1 {
        expected.insert("polyglot/protocol".into());
    }
    let candidates = choices
        .candidate_pattern_ids
        .iter()
        .cloned()
        .collect::<BTreeSet<_>>();
    let rejected = choices
        .rejected_candidates
        .iter()
        .map(|value| value.pattern_id.clone())
        .collect::<BTreeSet<_>>();
    if !expected.is_subset(&candidates)
        || candidates != expected.union(&rejected).cloned().collect()
    {
        return Err(
            "pattern candidates do not exactly explain selected and rejected patterns".into(),
        );
    }
    let registry = reviewed_golden_patterns()
        .into_iter()
        .map(|value| (value.pattern_id.clone(), value))
        .collect::<BTreeMap<_, _>>();
    for id in &rejected {
        if !registry.contains_key(id) {
            return Err(format!("rejected pattern {id:?} is not reviewed").into());
        }
    }
    expected
        .into_iter()
        .map(|id| {
            let value = registry
                .get(&id)
                .ok_or_else(|| format!("pattern {id:?} is not reviewed"))?;
            Ok(SelectedGoldenPatternV1 {
                pattern_id: value.pattern_id.clone(),
                revision: value.revision,
                evidence_sources: value.evidence_sources.clone(),
                composed_facet: value.composed_facet,
            })
        })
        .collect()
}

fn validate_evidence(
    root: &Path,
    assessment: &ArchitectureAssessmentV1,
    topology: &str,
    profile: &str,
    tree: &str,
) -> Result<(), Box<dyn std::error::Error>> {
    for evidence in assessment.evidence() {
        match evidence.kind {
            ArchitectureEvidenceKindV1::Topology
                if evidence.source == "project-topology" && evidence.sha256 == topology => {}
            ArchitectureEvidenceKindV1::Profile
                if evidence.source == "project-profile" && evidence.sha256 == profile => {}
            ArchitectureEvidenceKindV1::SourceTree
                if evidence.source == "source-tree" && evidence.sha256 == tree => {}
            ArchitectureEvidenceKindV1::File => {
                let relative = Path::new(&evidence.source);
                if relative.is_absolute()
                    || relative
                        .components()
                        .any(|value| !matches!(value, Component::Normal(_)))
                {
                    return Err("architecture file evidence is not project-relative".into());
                }
                let bytes = std::fs::read(root.join(relative))?;
                if digest::sha256_hex(&bytes) != evidence.sha256 {
                    return Err(format!("architecture evidence {} changed", evidence.source).into());
                }
            }
            _ => {
                return Err(format!(
                    "architecture evidence {} is stale or mismatched",
                    evidence.source
                )
                .into())
            }
        }
    }
    Ok(())
}

fn task_sha256(task: &str) -> String {
    sealed_digest(b"rrflow-task-v1\0", &task)
}
fn seal_assessment(value: &mut ArchitectureAssessmentV1) {
    value.assessment_sha256.clear();
    value.assessment_sha256 = sealed_digest(b"rrflow-architecture-assessment-v1\0", value);
}
fn seal_selection(value: &mut GoldenPatternSelectionV1) {
    value.selection_sha256.clear();
    value.selection_sha256 = sealed_digest(b"rrflow-golden-pattern-selection-v1\0", value);
}
fn sealed_digest(prefix: &[u8], value: &impl Serialize) -> String {
    let mut bytes = prefix.to_vec();
    bytes.extend_from_slice(&serde_json::to_vec(value).expect("architecture fields serialize"));
    digest::sha256_hex(&bytes)
}
