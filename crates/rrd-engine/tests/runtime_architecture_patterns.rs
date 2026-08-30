use rrd_engine::*;
use rrd_store::PersistentEngine;
use std::path::Path;

fn write(root: &Path, relative: &str, body: &str) {
    let path = root.join(relative);
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent).unwrap();
    }
    std::fs::write(path, body).unwrap();
}
fn project(polyglot: bool) -> tempfile::TempDir {
    let root = tempfile::tempdir().unwrap();
    InstanceManifest::ensure_dedicated_as(root.path(), "fixture").unwrap();
    write(
        root.path(),
        "Cargo.toml",
        "[package]\nname=\"fixture\"\nversion=\"0.1.0\"\n",
    );
    write(root.path(), "src/lib.rs", "pub fn payments() {}\n");
    if polyglot {
        write(
            root.path(),
            "package.json",
            r#"{"name":"web","scripts":{"test":"vitest"}}"#,
        );
        write(root.path(), "web/app.ts", "export const payments = true;\n");
    }
    root
}
fn known<T>(answer: T, evidence: &ArchitectureEvidenceV1) -> ArchitectureAnswerV1<T> {
    ArchitectureAnswerV1::Known {
        answer,
        evidence: vec![evidence.clone()],
    }
}
fn assessment(
    receipt: &TaskPreflightReceipt,
    decision: ArchitectureBoundaryDecisionV1,
    kind: ArchitectureBoundaryKindV1,
    candidates: Vec<&str>,
) -> ArchitectureAssessmentV1 {
    let evidence = ArchitectureEvidenceV1 {
        kind: ArchitectureEvidenceKindV1::Topology,
        source: "project-topology".into(),
        sha256: receipt.topology_sha256.clone(),
        summary: "current topology proves owner and stack".into(),
    };
    let impact = || ArchitectureImplicationV1 {
        disposition: ArchitectureImpactDispositionV1::Contained,
        detail: "contained by the engine".into(),
    };
    ArchitectureAssessmentV1 {
        format: ARCHITECTURE_ASSESSMENT_FORMAT_VERSION,
        task_sha256: receipt.task_sha256.clone(),
        topology_sha256: receipt.topology_sha256.clone(),
        profile_sha256: receipt.profile_sha256.clone(),
        source_tree_sha256: receipt.source_tree_sha256.clone(),
        capability_ownership: known(
            CapabilityOwnershipAnswerV1 {
                capability: "payments".into(),
                owner_boundary: "fixture".into(),
            },
            &evidence,
        ),
        boundary_choice: known(
            BoundaryChoiceAnswerV1 {
                decision,
                kind,
                boundary: if decision == ArchitectureBoundaryDecisionV1::ExtendExisting {
                    "fixture".into()
                } else {
                    "payment-service".into()
                },
                justification: "current topology and public contract require this boundary".into(),
            },
            &evidence,
        ),
        public_contract: known(
            PublicContractAnswerV1 {
                public_contract: "payments command".into(),
                dependency_direction: "contract -> engine -> adapter".into(),
            },
            &evidence,
        ),
        pattern_choice: known(
            PatternChoiceAnswerV1 {
                candidate_pattern_ids: candidates.into_iter().map(str::to_owned).collect(),
                rejected_candidates: Vec::new(),
            },
            &evidence,
        ),
        cross_boundary_implications: known(
            CrossBoundaryImplicationsAnswerV1 {
                persistence: impact(),
                transaction: impact(),
                security: impact(),
                audit: impact(),
                realtime: impact(),
                recovery: impact(),
                observability: impact(),
            },
            &evidence,
        ),
        path_ownership: known(
            PathOwnershipAnswerV1 {
                existing_paths: vec!["src/lib.rs".into()],
                justified_new_paths: if decision == ArchitectureBoundaryDecisionV1::CreateJustified
                {
                    vec!["crates/payment-service/src/lib.rs".into()]
                } else {
                    Vec::new()
                },
                oversized_boundary_prevention: "one owner per capability".into(),
                cyclic_ownership_prevention: "dependencies point toward contracts".into(),
            },
            &evidence,
        ),
        verification_matrix: known(
            VerificationMatrixAnswerV1 {
                exact_argv: vec![vec!["cargo".into(), "test".into()]],
                platforms: vec!["linux-x86_64".into()],
            },
            &evidence,
        ),
        assessment_sha256: "0".repeat(64),
    }
}
fn preflight(root: &Path, store: &PersistentEngine, task: &str) -> TaskPreflightReceipt {
    preflight_task(
        store,
        root,
        None,
        &Reader::new("test:architecture").unwrap(),
        10,
        4096,
        Some(task),
    )
    .unwrap()
    .context_receipt
    .unwrap()
}

#[test]
fn existing_boundary_selects_a_reviewed_component_and_reopens() {
    let root = project(false);
    let database = root.path().join(".rrflow/rrd");
    let store = PersistentEngine::open(&database).unwrap();
    let task = "implement payments in the existing fixture boundary";
    let receipt = preflight(root.path(), &store, task);
    let review = review_architecture(
        &store,
        root.path(),
        task,
        assessment(
            &receipt,
            ArchitectureBoundaryDecisionV1::ExtendExisting,
            ArchitectureBoundaryKindV1::Component,
            vec!["rust/component"],
        ),
        11,
    )
    .unwrap();
    assert!(review.verify());
    assert_eq!(review.selection.selected[0].pattern_id, "rust/component");
    drop(store);
    let reopened = PersistentEngine::open(&database).unwrap();
    assert_eq!(load_architecture_review(&reopened).unwrap(), Some(review));
    assert_eq!(
        preflight(root.path(), &reopened, task).pattern_disposition,
        EvidenceDisposition::Current
    );
}

#[test]
fn justified_new_boundary_selects_the_reviewed_application_pattern() {
    let root = project(false);
    let store = PersistentEngine::open(&root.path().join(".rrflow/rrd")).unwrap();
    let task = "build a justified payments application boundary";
    let receipt = preflight(root.path(), &store, task);
    let review = review_architecture(
        &store,
        root.path(),
        task,
        assessment(
            &receipt,
            ArchitectureBoundaryDecisionV1::CreateJustified,
            ArchitectureBoundaryKindV1::Application,
            vec!["rust/application"],
        ),
        11,
    )
    .unwrap();
    assert_eq!(review.selection.selected[0].pattern_id, "rust/application");
}

#[test]
fn polyglot_checkouts_compose_identical_reviewed_facets() {
    let task = "implement payments across native and web boundaries";
    let mut selections = Vec::new();
    for _ in 0..2 {
        let root = project(true);
        let store = PersistentEngine::open(&root.path().join(".rrflow/rrd")).unwrap();
        let receipt = preflight(root.path(), &store, task);
        selections.push(
            review_architecture(
                &store,
                root.path(),
                task,
                assessment(
                    &receipt,
                    ArchitectureBoundaryDecisionV1::ExtendExisting,
                    ArchitectureBoundaryKindV1::Component,
                    vec![
                        "polyglot/protocol",
                        "rust/component",
                        "typescript/component",
                    ],
                ),
                11,
            )
            .unwrap()
            .selection,
        );
    }
    assert_eq!(selections[0], selections[1]);
    assert_eq!(
        selections[0]
            .selected
            .iter()
            .map(|v| v.pattern_id.as_str())
            .collect::<Vec<_>>(),
        vec![
            "polyglot/protocol",
            "rust/component",
            "typescript/component"
        ]
    );
}
