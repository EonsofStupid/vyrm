use rrd_contract::CanonicalId;
use rrd_estate::{
    DesiredPhase, DesiredTarget, EstateRepository, MutationContext, OperationState, SetDesired,
};

fn id(value: &str) -> CanonicalId {
    CanonicalId::new(value).unwrap()
}

fn context(at: u64, operation: &str) -> MutationContext {
    MutationContext {
        at,
        actor: "rrd-estate".into(),
        request_id: format!("request-{operation}"),
        operation_id: id(operation),
    }
}

#[test]
fn connectome_projects_persisted_estate_authority_without_mutating_it() {
    let temporary = tempfile::tempdir().unwrap();
    let root = temporary.path().join("estate-a");
    std::fs::create_dir(&root).unwrap();
    vyrm_node::InstanceManifest::ensure_dedicated(&root).unwrap();
    let store = vyrm_store::PersistentEngine::open(&root.join(vyrm_node::STORE_DIR)).unwrap();
    let repository = EstateRepository::new(&store, id("estate-a"));
    repository.create(&context(10, "create-estate")).unwrap();
    repository
        .set_desired(&SetDesired {
            context: context(20, "deploy-project-a"),
            instance_id: id("project-a"),
            idempotency_key: "deploy-project-a".into(),
            target: DesiredTarget {
                phase: DesiredPhase::Running,
                deployment_ref: id("local-rrd"),
                version: "0.1.0".into(),
                configuration_sha256: "a".repeat(64),
            },
        })
        .unwrap();
    let revision_before = repository.load().unwrap().unwrap().revision;
    let binding = vyrm_node::InstanceBinding::discover(&root).unwrap();

    let snapshot = connectome_ui::snapshot(&store, &binding, 30).unwrap();
    let estate = &snapshot.estates[0];
    assert_eq!(estate.control_plane, "rrd-estate");
    assert_eq!(estate.authority, "authoritative");
    assert_eq!(estate.state, "reconciling");
    let authority = estate.snapshot.as_ref().unwrap();
    assert_eq!(authority.revision, 2);
    assert_eq!(authority.instances[0].id, id("project-a"));
    assert_eq!(
        authority.operations[0].state,
        rrd_contract::EstateOperationState::Pending
    );
    assert_eq!(
        repository.load().unwrap().unwrap().revision,
        revision_before,
        "Connectome reads must not advance estate authority"
    );
    assert_eq!(
        repository
            .load()
            .unwrap()
            .unwrap()
            .operation(&id("deploy-project-a"))
            .unwrap()
            .state,
        OperationState::Pending
    );
}
