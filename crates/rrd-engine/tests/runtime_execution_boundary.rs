use rrd_engine::{
    digest, load_exact_execution_observation, record_exact_execution_observation,
    ExactExecutionObservationV1, ExactExecutionRepositoryV1, ExactExecutionRequestV1,
    ExactExecutionStreamV1, EXACT_EXECUTION_CONTRACT,
};
use rrd_store::PersistentEngine;

fn repository(byte: char, paths: &[&str]) -> ExactExecutionRepositoryV1 {
    ExactExecutionRepositoryV1 {
        revision: "git:fixture".into(),
        worktree_sha256: byte.to_string().repeat(64),
        changed_paths: paths.iter().map(|path| (*path).into()).collect(),
    }
}

fn observation() -> ExactExecutionObservationV1 {
    let exact_argv = vec!["npm".into(), "test".into(), "--".into(), "one spec".into()];
    let mut request = ExactExecutionRequestV1 {
        contract: EXACT_EXECUTION_CONTRACT.into(),
        project_root_sha256: "1".repeat(64),
        cwd: "/fixture".into(),
        invoked_as: "npm".into(),
        executable: "/tools/npm".into(),
        executable_sha256: "2".repeat(64),
        exact_argv_sha256: digest::sha256_hex(&serde_json::to_vec(&exact_argv).unwrap()),
        exact_argv,
        environment_sha256: "3".repeat(64),
        environment_entries: 4,
        repository_before: repository('4', &["package.json"]),
        timeout_ms: 1_000,
        max_output_bytes: 16,
        verification_policy: "checked_in_work_plan".into(),
        request_sha256: String::new(),
    };
    request.seal().unwrap();
    let mut observation = ExactExecutionObservationV1 {
        request,
        authorization_sha256: "5".repeat(64),
        repository_after: repository('6', &["generated/client.ts", "package.json"]),
        exit_code: Some(0),
        signal: None,
        success: true,
        timed_out: false,
        duration_ms: 10,
        spawn_error: None,
        stdout: ExactExecutionStreamV1 {
            sha256: digest::sha256_hex(b"0123456789abcdef-more"),
            bytes: 21,
            retained_bytes: 16,
            truncated: true,
            retained_utf8: Some("0123456789abcdef".into()),
        },
        stderr: ExactExecutionStreamV1 {
            sha256: digest::sha256_hex(b""),
            bytes: 0,
            retained_bytes: 0,
            truncated: false,
            retained_utf8: Some(String::new()),
        },
        observation_sha256: String::new(),
    };
    observation.seal().unwrap();
    observation
}

#[test]
fn exact_observation_is_idempotent_reopen_safe_and_substitution_proof() {
    let root = tempfile::tempdir().unwrap();
    let database = root.path().join("rrd");
    let expected = observation();
    let request_sha256 = expected.request.request_sha256.clone();
    let store = PersistentEngine::open(&database).unwrap();
    record_exact_execution_observation(&store, &expected).unwrap();
    record_exact_execution_observation(&store, &expected).unwrap();

    let mut substituted = expected.clone();
    substituted.repository_after.worktree_sha256 = "7".repeat(64);
    substituted.seal().unwrap();
    assert!(record_exact_execution_observation(&store, &substituted).is_err());
    drop(store);

    let reopened = PersistentEngine::open(&database).unwrap();
    assert_eq!(
        load_exact_execution_observation(&reopened, &request_sha256).unwrap(),
        Some(expected)
    );
    assert!(load_exact_execution_observation(&reopened, &"z".repeat(64)).is_err());
}
