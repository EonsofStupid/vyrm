use rrd_contract::CanonicalId;
use rrd_estate::{
    public_snapshot, ApplyAuthority, AuthorityDesiredState, AuthorityObservedState,
    AuthorityReceipt, AuthorityReceiptBoundary, AuthorityResource, AuthorityResourceKind,
    AuthorityStatus, DesiredPhase, DesiredTarget, Error, EstateRepository, MutationContext,
    SetDesired,
};
use rrd_store::{Engine, NativeEngine};

fn id(value: &str) -> CanonicalId {
    CanonicalId::new(value).unwrap()
}

fn context(at: u64, request: &str, operation: &str) -> MutationContext {
    MutationContext {
        at,
        actor: "estate-controller".into(),
        request_id: request.into(),
        operation_id: id(operation),
    }
}

fn desired(generation: u64, at: u64, byte: char) -> AuthorityDesiredState {
    AuthorityDesiredState {
        generation,
        spec_sha256: byte.to_string().repeat(64),
        updated_at: at,
    }
}

fn observed(generation: u64, at: u64, byte: char) -> AuthorityObservedState {
    AuthorityObservedState {
        generation,
        status: AuthorityStatus::Ready,
        observed_at: at,
        evidence_sha256: byte.to_string().repeat(64),
        error: None,
    }
}

#[allow(clippy::too_many_arguments)]
fn resource(
    name: &str,
    kind: AuthorityResourceKind,
    parent: Option<&str>,
    secondary_parent: Option<&str>,
    state: bool,
    secrets: &[&str],
    at: u64,
) -> AuthorityResource {
    AuthorityResource {
        id: id(name),
        kind,
        parent_id: parent.map(id),
        secondary_parent_id: secondary_parent.map(id),
        name: name.replace('-', " "),
        spec_sha256: "a".repeat(64),
        desired: state.then(|| desired(1, at, 'b')),
        observed: state.then(|| observed(1, at, 'c')),
        secret_reference_ids: secrets.iter().map(|value| id(value)).collect(),
        created_at: at,
        updated_at: at,
    }
}

fn complete_resources(at: u64) -> Vec<AuthorityResource> {
    vec![
        resource(
            "org-a",
            AuthorityResourceKind::Organisation,
            None,
            None,
            false,
            &[],
            at,
        ),
        resource(
            "account-a",
            AuthorityResourceKind::Account,
            Some("org-a"),
            None,
            false,
            &[],
            at,
        ),
        resource(
            "entitlement-a",
            AuthorityResourceKind::Entitlement,
            Some("account-a"),
            None,
            false,
            &[],
            at,
        ),
        resource(
            "project-a",
            AuthorityResourceKind::Project,
            Some("account-a"),
            None,
            false,
            &[],
            at,
        ),
        resource(
            "environment-a",
            AuthorityResourceKind::Environment,
            Some("project-a"),
            None,
            false,
            &[],
            at,
        ),
        resource(
            "instance-a",
            AuthorityResourceKind::Instance,
            Some("environment-a"),
            None,
            false,
            &[],
            at,
        ),
        resource(
            "secret-a",
            AuthorityResourceKind::SecretReference,
            Some("environment-a"),
            None,
            false,
            &[],
            at,
        ),
        resource(
            "node-a",
            AuthorityResourceKind::Node,
            Some("environment-a"),
            None,
            true,
            &["secret-a"],
            at,
        ),
        resource(
            "shard-a",
            AuthorityResourceKind::Shard,
            Some("instance-a"),
            None,
            true,
            &[],
            at,
        ),
        resource(
            "job-a",
            AuthorityResourceKind::Job,
            Some("environment-a"),
            None,
            true,
            &["secret-a"],
            at,
        ),
        resource(
            "assignment-a",
            AuthorityResourceKind::Assignment,
            Some("job-a"),
            Some("node-a"),
            true,
            &[],
            at,
        ),
        AuthorityResource {
            observed: Some(observed(1, at, 'd')),
            ..resource(
                "health-a",
                AuthorityResourceKind::Health,
                Some("node-a"),
                None,
                false,
                &[],
                at,
            )
        },
    ]
}

#[test]
fn every_estate_resource_and_receipt_survives_native_reopen() {
    let temporary = tempfile::tempdir().unwrap();
    let path = temporary.path().join("authority");
    let request = ApplyAuthority {
        context: context(30, "authority-request", "authority-operation"),
        idempotency_key: "authority-batch-a".into(),
        resources: complete_resources(30),
        receipts: vec![AuthorityReceipt {
            id: id("receipt-a"),
            resource_id: id("assignment-a"),
            operation_id: id("authority-operation"),
            lease_epoch: 1,
            boundary: AuthorityReceiptBoundary::Completed,
            at: 30,
            evidence_sha256: "e".repeat(64),
        }],
    };
    {
        let engine = NativeEngine::open(&path).unwrap();
        let repository = EstateRepository::new(&engine, id("estate-a"));
        repository
            .create(&context(10, "create-estate", "create-estate"))
            .unwrap();
        repository
            .set_desired(&SetDesired {
                context: context(20, "instance-request", "instance-operation"),
                instance_id: id("instance-a"),
                idempotency_key: "instance-desired-a".into(),
                target: DesiredTarget {
                    phase: DesiredPhase::Running,
                    deployment_ref: id("local-rrd"),
                    version: "0.1.0".into(),
                    configuration_sha256: "f".repeat(64),
                },
            })
            .unwrap();
        let accepted = repository.apply_authority(&request).unwrap();
        assert!(!accepted.idempotent_replay);
        assert_eq!(accepted.document.authority.resources.len(), 12);
        assert_eq!(accepted.document.authority.receipts.len(), 1);
        let replay = repository.apply_authority(&request).unwrap();
        assert!(replay.idempotent_replay);
        assert_eq!(replay.document.revision, accepted.document.revision);
    }

    let reopened = NativeEngine::open(&path).unwrap();
    let repository = EstateRepository::new(&reopened, id("estate-a"));
    let document = repository.load().unwrap().unwrap();
    document.validate().unwrap();
    for kind in [
        AuthorityResourceKind::Organisation,
        AuthorityResourceKind::Account,
        AuthorityResourceKind::Entitlement,
        AuthorityResourceKind::Project,
        AuthorityResourceKind::Environment,
        AuthorityResourceKind::Instance,
        AuthorityResourceKind::Node,
        AuthorityResourceKind::Shard,
        AuthorityResourceKind::Job,
        AuthorityResourceKind::Assignment,
        AuthorityResourceKind::Health,
        AuthorityResourceKind::SecretReference,
    ] {
        assert!(document
            .authority
            .resources
            .values()
            .any(|resource| resource.kind == kind));
    }
    let public = public_snapshot(&document);
    assert_eq!(public.authority.resources.len(), 12);
    assert_eq!(public.authority.receipts.len(), 1);
    let encoded = serde_json::to_string(&public).unwrap();
    assert!(!encoded.contains("vault://"));

    let journal = reopened.control_journal_since(0, 10).unwrap();
    assert_eq!(
        journal
            .iter()
            .map(|entry| entry.action.as_str())
            .collect::<Vec<_>>(),
        vec![
            "estate.create",
            "estate.desired.set",
            "estate.authority.apply"
        ]
    );
    assert!(journal.iter().all(|entry| entry.verify()));
}

#[test]
fn references_generations_and_idempotency_fail_closed() {
    let engine = rrd_store::MemoryEngine::new();
    let repository = EstateRepository::new(&engine, id("estate-a"));
    repository
        .create(&context(10, "create-estate", "create-estate"))
        .unwrap();

    let orphan = ApplyAuthority {
        context: context(20, "orphan", "orphan-operation"),
        idempotency_key: "orphan-key".into(),
        resources: vec![resource(
            "account-a",
            AuthorityResourceKind::Account,
            Some("missing-org"),
            None,
            false,
            &[],
            20,
        )],
        receipts: Vec::new(),
    };
    assert!(matches!(
        repository.apply_authority(&orphan),
        Err(Error::Invalid(_))
    ));
    assert_eq!(repository.load().unwrap().unwrap().revision, 1);

    let accepted = ApplyAuthority {
        context: context(30, "complete", "complete-operation"),
        idempotency_key: "complete-key".into(),
        resources: complete_resources(30),
        receipts: Vec::new(),
    };
    repository.apply_authority(&accepted).unwrap();

    let mut rebound = accepted.clone();
    rebound.resources.last_mut().unwrap().name = "changed health".into();
    assert!(matches!(
        repository.apply_authority(&rebound),
        Err(Error::IdempotencyConflict(_))
    ));

    let mut backwards = document_resource(
        &repository,
        "node-a",
        context(40, "backwards", "backwards-operation"),
        "backwards-key",
    );
    backwards.resources[0].desired.as_mut().unwrap().generation = 0;
    assert!(matches!(
        repository.apply_authority(&backwards),
        Err(Error::Invalid(_))
    ));

    let mut future_observation = document_resource(
        &repository,
        "node-a",
        context(50, "future", "future-operation"),
        "future-key",
    );
    future_observation.resources[0].updated_at = 50;
    future_observation.resources[0]
        .observed
        .as_mut()
        .unwrap()
        .generation = 2;
    assert!(matches!(
        repository.apply_authority(&future_observation),
        Err(Error::Invalid(_))
    ));
}

fn document_resource(
    repository: &EstateRepository<'_, rrd_store::MemoryEngine>,
    resource_id: &str,
    context: MutationContext,
    idempotency_key: &str,
) -> ApplyAuthority {
    let mut resource = repository
        .load()
        .unwrap()
        .unwrap()
        .authority
        .resource(&id(resource_id))
        .unwrap()
        .clone();
    resource.updated_at = context.at;
    ApplyAuthority {
        context,
        idempotency_key: idempotency_key.into(),
        resources: vec![resource],
        receipts: Vec::new(),
    }
}
