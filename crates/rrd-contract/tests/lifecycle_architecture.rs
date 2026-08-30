use rrd_contract::*;

fn sha(byte: char) -> String {
    byte.to_string().repeat(64)
}
fn evidence() -> ArchitectureEvidenceV1 {
    ArchitectureEvidenceV1 {
        kind: ArchitectureEvidenceKindV1::Topology,
        source: "project-topology".into(),
        sha256: sha('a'),
        summary: "current topology".into(),
    }
}
fn known<T>(answer: T) -> ArchitectureAnswerV1<T> {
    ArchitectureAnswerV1::Known {
        answer,
        evidence: vec![evidence()],
    }
}
fn assessment() -> ArchitectureAssessmentV1 {
    let impact = || ArchitectureImplicationV1 {
        disposition: ArchitectureImpactDispositionV1::Contained,
        detail: "contained by the engine".into(),
    };
    ArchitectureAssessmentV1 {
        format: ARCHITECTURE_ASSESSMENT_FORMAT_VERSION,
        task_sha256: sha('1'),
        topology_sha256: sha('a'),
        profile_sha256: sha('b'),
        source_tree_sha256: sha('c'),
        capability_ownership: known(CapabilityOwnershipAnswerV1 {
            capability: "architecture".into(),
            owner_boundary: "rrd-engine/runtime".into(),
        }),
        boundary_choice: known(BoundaryChoiceAnswerV1 {
            decision: ArchitectureBoundaryDecisionV1::ExtendExisting,
            kind: ArchitectureBoundaryKindV1::Component,
            boundary: "rrd-engine/runtime".into(),
            justification: "the runtime owns planning state".into(),
        }),
        public_contract: known(PublicContractAnswerV1 {
            public_contract: "strict V1 assessment".into(),
            dependency_direction: "rrd-contract -> rrd-engine".into(),
        }),
        pattern_choice: known(PatternChoiceAnswerV1 {
            candidate_pattern_ids: vec!["rust/component".into()],
            rejected_candidates: Vec::new(),
        }),
        cross_boundary_implications: known(CrossBoundaryImplicationsAnswerV1 {
            persistence: impact(),
            transaction: impact(),
            security: impact(),
            audit: impact(),
            realtime: impact(),
            recovery: impact(),
            observability: impact(),
        }),
        path_ownership: known(PathOwnershipAnswerV1 {
            existing_paths: vec!["crates/rrd-engine/src/runtime".into()],
            justified_new_paths: Vec::new(),
            oversized_boundary_prevention: "separate contract from authority".into(),
            cyclic_ownership_prevention: "dependencies point toward contract".into(),
        }),
        verification_matrix: known(VerificationMatrixAnswerV1 {
            exact_argv: vec![vec!["cargo".into(), "test".into()]],
            platforms: vec!["linux-x86_64".into()],
        }),
        assessment_sha256: sha('d'),
    }
}

#[test]
fn all_seven_answers_are_typed_and_unknown_fields_fail_closed() {
    let assessment = assessment();
    assessment.validate().unwrap();
    assert!(assessment.all_answers_known());
    assert_eq!(assessment.evidence().len(), 7);
    let mut value = serde_json::to_value(assessment).unwrap();
    value["copied_boilerplate"] = serde_json::json!(true);
    assert!(serde_json::from_value::<ArchitectureAssessmentV1>(value).is_err());
}

#[test]
fn unknown_requires_missing_evidence_and_a_discovery_action() {
    let mut value = assessment();
    value.boundary_choice = ArchitectureAnswerV1::Unknown {
        missing_evidence: vec!["member ownership".into()],
        discovery_action: "inspect the current topology member edge".into(),
    };
    value.validate().unwrap();
    assert!(!value.all_answers_known());
    value.boundary_choice = ArchitectureAnswerV1::Unknown {
        missing_evidence: Vec::new(),
        discovery_action: "inspect topology".into(),
    };
    assert!(value.validate().is_err());
}
