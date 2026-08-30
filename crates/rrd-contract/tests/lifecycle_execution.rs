use rrd_contract::{
    ExactExecutionObservationV1, ExactExecutionRepositoryV1, ExactExecutionRequestV1,
    ExactExecutionStreamV1, EXACT_EXECUTION_CONTRACT, MAX_EXACT_EXECUTION_OUTPUT_BYTES,
    MAX_EXACT_EXECUTION_TIMEOUT_MS,
};
use sha2::{Digest, Sha256};

fn sha(bytes: &[u8]) -> String {
    let digest = Sha256::digest(bytes);
    const HEX: &[u8; 16] = b"0123456789abcdef";
    let mut encoded = String::with_capacity(digest.len() * 2);
    for byte in digest {
        encoded.push(HEX[(byte >> 4) as usize] as char);
        encoded.push(HEX[(byte & 0x0f) as usize] as char);
    }
    encoded
}

fn repository(worktree: u8, paths: &[&str]) -> ExactExecutionRepositoryV1 {
    ExactExecutionRepositoryV1 {
        revision: "git:0123456789abcdef".into(),
        worktree_sha256: format!("{worktree:02x}").repeat(32),
        changed_paths: paths.iter().map(|path| (*path).into()).collect(),
    }
}

fn request() -> ExactExecutionRequestV1 {
    let exact_argv = vec!["cargo".into(), "test".into(), "one argument".into()];
    let mut request = ExactExecutionRequestV1 {
        contract: EXACT_EXECUTION_CONTRACT.into(),
        project_root_sha256: "11".repeat(32),
        cwd: "/project".into(),
        invoked_as: "cargo".into(),
        executable: "/toolchain/bin/cargo".into(),
        executable_sha256: "22".repeat(32),
        exact_argv_sha256: sha(&serde_json::to_vec(&exact_argv).unwrap()),
        exact_argv,
        environment_sha256: "33".repeat(32),
        environment_entries: 12,
        repository_before: repository(0x44, &["src/lib.rs"]),
        timeout_ms: 30_000,
        max_output_bytes: 4_096,
        verification_policy: "checked_in_work_plan".into(),
        request_sha256: String::new(),
    };
    request.seal().unwrap();
    request
}

fn observation() -> ExactExecutionObservationV1 {
    let mut observation = ExactExecutionObservationV1 {
        request: request(),
        authorization_sha256: "55".repeat(32),
        repository_after: repository(0x66, &["src/lib.rs", "src/new.rs"]),
        exit_code: Some(0),
        signal: None,
        success: true,
        timed_out: false,
        duration_ms: 25,
        spawn_error: None,
        stdout: ExactExecutionStreamV1 {
            sha256: sha(b"complete stdout"),
            bytes: 15,
            retained_bytes: 4,
            truncated: true,
            retained_utf8: Some("comp".into()),
        },
        stderr: ExactExecutionStreamV1 {
            sha256: sha(b""),
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
fn exact_execution_contract_seals_every_authorized_and_observed_identity() {
    let value = observation();
    value.verify().unwrap();
    assert_ne!(value.request.request_sha256, value.observation_sha256);

    let mut substituted = value.clone();
    substituted.request.exact_argv[2] = "two arguments".into();
    assert!(substituted.verify().is_err());

    let mut contradictory = value;
    contradictory.stdout.truncated = false;
    contradictory.seal().unwrap_err();
}

#[test]
fn execution_bounds_and_unknown_fields_fail_closed() {
    let mut timeout = request();
    timeout.timeout_ms = MAX_EXACT_EXECUTION_TIMEOUT_MS + 1;
    timeout.seal().unwrap_err();

    let mut output = request();
    output.max_output_bytes = MAX_EXACT_EXECUTION_OUTPUT_BYTES + 1;
    output.seal().unwrap_err();

    let mut encoded = serde_json::to_value(request()).unwrap();
    encoded["shell_command"] = serde_json::json!("cargo test && deploy");
    assert!(serde_json::from_value::<ExactExecutionRequestV1>(encoded).is_err());
}
