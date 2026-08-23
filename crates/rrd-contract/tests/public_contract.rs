use rrd_contract::{
    transaction_operation_sha256, BeginTransaction, CanonicalId, CapabilityDescriptor,
    CapabilityStatus, CloseSession, CommitReceipt, CommitTransaction, CorrelationId,
    DeploymentMode, ErrorBody, ErrorCode, IdempotencyBinding, Liveness, PreviewTransaction,
    Readiness, RenewSession, RequestContext, RequestEnvelope, ResourceId, ResourceKind,
    ResourcePath, ResponseEnvelope, ResponseOutcome, ServiceCapabilities, SessionEndState,
    SessionLease, SessionLimits, SessionTermination, TransactionMutation, TransactionPreview,
    TransactionState, PROTOCOL, PROTOCOL_VERSION,
};
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct FixturePayload {
    action: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct ContractFixture {
    service: ServiceCapabilities,
    request: RequestEnvelope<FixturePayload>,
    success: ResponseEnvelope<FixturePayload>,
    failure: ResponseEnvelope<FixturePayload>,
    idempotency: IdempotencyBinding,
}

fn contract_fixture() -> ContractFixture {
    let request_id = CorrelationId::new("req-01j5y0h4k7").unwrap();
    let operation_id = CorrelationId::new("op-01j5y0h4k8").unwrap();
    let idempotency_key = CorrelationId::new("idem-01j5y0h4k9").unwrap();
    ContractFixture {
        service: ServiceCapabilities {
            protocol: PROTOCOL.into(),
            protocol_version: PROTOCOL_VERSION,
            implementation: CanonicalId::new("vyrm").unwrap(),
            implementation_version: "0.1.0-alpha.1".into(),
            deployment_mode: DeploymentMode::Embedded,
            instance: ResourceId::new(ResourceKind::Instance, "project-alpha").unwrap(),
            capabilities: vec![
                CapabilityDescriptor {
                    name: CanonicalId::new("logical-archive").unwrap(),
                    contract_version: 1,
                    status: CapabilityStatus::Unavailable,
                    limits: BTreeMap::new(),
                    limitation: Some("F1 implementation gate is not complete".into()),
                },
                CapabilityDescriptor {
                    name: CanonicalId::new("native-persistence").unwrap(),
                    contract_version: 1,
                    status: CapabilityStatus::Experimental,
                    limits: BTreeMap::from([(
                        CanonicalId::new("max-snapshot-bytes").unwrap(),
                        1_073_741_824,
                    )]),
                    limitation: Some(
                        "local alpha; independent-host qualification remains open".into(),
                    ),
                },
            ],
        },
        request: RequestEnvelope {
            protocol: PROTOCOL.into(),
            protocol_version: PROTOCOL_VERSION,
            context: RequestContext {
                request_id: request_id.clone(),
                operation_id: operation_id.clone(),
                idempotency_key: Some(idempotency_key.clone()),
                deadline_unix_ms: Some(1_800_000_000_000),
            },
            resource: ResourcePath {
                segments: vec![
                    ResourceId::new(ResourceKind::Organization, "local").unwrap(),
                    ResourceId::new(ResourceKind::Estate, "developer").unwrap(),
                    ResourceId::new(ResourceKind::Project, "alpha").unwrap(),
                    ResourceId::new(ResourceKind::Instance, "project-alpha").unwrap(),
                ],
            },
            payload: FixturePayload {
                action: "backup.create".into(),
            },
        },
        success: ResponseEnvelope {
            protocol: PROTOCOL.into(),
            protocol_version: PROTOCOL_VERSION,
            request_id: request_id.clone(),
            operation_id: operation_id.clone(),
            outcome: ResponseOutcome::Ok {
                payload: FixturePayload {
                    action: "backup.created".into(),
                },
            },
        },
        failure: ResponseEnvelope {
            protocol: PROTOCOL.into(),
            protocol_version: PROTOCOL_VERSION,
            request_id,
            operation_id,
            outcome: ResponseOutcome::Error {
                error: ErrorBody {
                    code: ErrorCode::FailedPrecondition,
                    message: "source read stamp is no longer current".into(),
                    retryable: true,
                    details: BTreeMap::from([(
                        CanonicalId::new("observed-cursor").unwrap(),
                        "42".into(),
                    )]),
                },
            },
        },
        idempotency: IdempotencyBinding {
            key: idempotency_key,
            operation_sha256: "3c94150b4ea4f9dcb27d3b602e9f190debe656533c047b99e11367bc6a28017f"
                .into(),
        },
    }
}

#[test]
fn public_contract_matches_frozen_golden_json() {
    let fixture = contract_fixture();
    fixture.service.validate().unwrap();
    fixture.request.validate(true).unwrap();
    fixture.success.validate().unwrap();
    fixture.failure.validate().unwrap();
    fixture.idempotency.validate().unwrap();

    let expected: serde_json::Value =
        serde_json::from_str(include_str!("../fixtures/public-contract-v1.json")).unwrap();
    let actual = serde_json::to_value(&fixture).unwrap();
    assert_eq!(actual, expected);

    let reopened: ContractFixture = serde_json::from_value(expected).unwrap();
    assert_eq!(reopened, fixture);
}

#[test]
fn malformed_identifiers_and_unknown_fields_fail_during_decode() {
    assert!(serde_json::from_str::<CanonicalId>(r#""Not Canonical""#).is_err());
    assert!(CorrelationId::new("request/id").is_err());

    let mut request = serde_json::to_value(&contract_fixture().request).unwrap();
    request
        .as_object_mut()
        .unwrap()
        .insert("unknown".into(), serde_json::json!(true));
    assert!(serde_json::from_value::<RequestEnvelope<FixturePayload>>(request).is_err());
}

#[test]
fn mutation_idempotency_protocol_and_capability_order_fail_closed() {
    let fixture = contract_fixture();
    let mut request = fixture.request;
    request.context.idempotency_key = None;
    assert!(request.validate(true).is_err());
    assert!(request.validate(false).is_ok());

    request.protocol_version += 1;
    assert!(request.validate(false).is_err());

    let mut service = fixture.service;
    service.capabilities.reverse();
    assert!(service.validate().is_err());
    service
        .capabilities
        .sort_by(|left, right| left.name.cmp(&right.name));
    service.capabilities.push(service.capabilities[0].clone());
    assert!(service.validate().is_err());
}

#[test]
fn resource_paths_are_explicit_bounded_and_non_repeating() {
    let empty = ResourcePath { segments: vec![] };
    assert!(empty.validate().is_err());

    let repeated = ResourcePath {
        segments: vec![
            ResourceId::new(ResourceKind::Project, "alpha").unwrap(),
            ResourceId::new(ResourceKind::Project, "beta").unwrap(),
        ],
    };
    assert!(repeated.validate().is_err());
}

#[test]
fn session_and_claim_transaction_contracts_are_bounded() {
    let limits = SessionLimits {
        idle_timeout_ms: 30_000,
        absolute_timeout_ms: 300_000,
        max_open_transactions: 4,
    };
    limits.validate().unwrap();
    SessionLease {
        session_id: CorrelationId::new("session-1").unwrap(),
        token: CorrelationId::new("token-1").unwrap(),
        issued_at_unix_ms: 100,
        idle_expires_at_unix_ms: 30_100,
        absolute_expires_at_unix_ms: 300_100,
        limits: limits.clone(),
    }
    .validate()
    .unwrap();
    BeginTransaction {
        scope: CanonicalId::new("instance-a").unwrap(),
        timeout_ms: 10_000,
    }
    .validate()
    .unwrap();
    let commit = CommitTransaction {
        operation_sha256: "3c94150b4ea4f9dcb27d3b602e9f190debe656533c047b99e11367bc6a28017f".into(),
        mutations: vec![TransactionMutation::AssertClaim {
            subject: CanonicalId::new("task-1").unwrap(),
            predicate: CanonicalId::new("status").unwrap(),
            object: "verified".into(),
            valid_from: 100,
            tx_time: 101,
            producer: CanonicalId::new("agent-test").unwrap(),
            confidence: Some(0.9),
        }],
    };
    commit.validate().unwrap();
    CommitReceipt {
        transaction_id: CorrelationId::new("transaction-1").unwrap(),
        operation_sha256: "3c94150b4ea4f9dcb27d3b602e9f190debe656533c047b99e11367bc6a28017f".into(),
        first_claim_sequence: 7,
        last_claim_sequence: 7,
        mutation_count: 1,
        idempotent_replay: false,
    }
    .validate()
    .unwrap();
    assert_eq!(TransactionState::Open, TransactionState::Open);

    let mut invalid = limits;
    invalid.idle_timeout_ms = invalid.absolute_timeout_ms + 1;
    assert!(invalid.validate().is_err());
    let mut invalid_commit = commit;
    let TransactionMutation::AssertClaim { confidence, .. } = &mut invalid_commit.mutations[0];
    *confidence = Some(f32::NAN);
    assert!(invalid_commit.validate().is_err());
}

#[test]
fn lifecycle_health_and_preview_payloads_are_bounded_and_strict() {
    let mutation = TransactionMutation::AssertClaim {
        subject: CanonicalId::new("task-1").unwrap(),
        predicate: CanonicalId::new("status").unwrap(),
        object: "verified".into(),
        valid_from: 100,
        tx_time: 101,
        producer: CanonicalId::new("agent-test").unwrap(),
        confidence: None,
    };
    let preview = PreviewTransaction {
        mutations: vec![mutation.clone()],
    };
    preview.validate().unwrap();
    TransactionPreview {
        transaction_id: CorrelationId::new("transaction-1").unwrap(),
        read_cursor: 7,
        operation_sha256: "3c94150b4ea4f9dcb27d3b602e9f190debe656533c047b99e11367bc6a28017f".into(),
        mutations: preview.mutations,
    }
    .validate()
    .unwrap();
    SessionTermination {
        session_id: CorrelationId::new("session-1").unwrap(),
        state: SessionEndState::Closed,
        ended_at_unix_ms: 1,
        affected_open_transactions: 2,
        idempotent_replay: false,
    }
    .validate()
    .unwrap();
    Liveness {
        observed_at_unix_ms: 1,
    }
    .validate()
    .unwrap();
    Readiness {
        observed_at_unix_ms: 1,
        claim_sequence: 2,
        runtime_cursor: 3,
        backend: CanonicalId::new("vyrmkv-native").unwrap(),
    }
    .validate()
    .unwrap();

    assert!(serde_json::from_str::<RenewSession>(r#"{"unknown":true}"#).is_err());
    assert!(serde_json::from_str::<CloseSession>(r#"{"unknown":true}"#).is_err());
    assert!(
        serde_json::from_str::<PreviewTransaction>(r#"{"mutations":[],"unknown":true}"#).is_err()
    );
}

#[test]
fn transaction_digest_has_a_cross_language_golden_vector() {
    let first: Vec<TransactionMutation> = serde_json::from_str(
        r#"[{"mutation":"assert_claim","subject":"task-1","predicate":"status","object":"verified","valid_from":100,"tx_time":101,"producer":"agent-test"}]"#,
    )
    .unwrap();
    let reordered: Vec<TransactionMutation> = serde_json::from_str(
        r#"[{"producer":"agent-test","tx_time":101,"valid_from":100,"object":"verified","predicate":"status","subject":"task-1","mutation":"assert_claim"}]"#,
    )
    .unwrap();
    let expected = "d85547ca333304b54db4bc0249b6e11f83cf975fdc7cfebcf551ec6564bb71f8";
    assert_eq!(transaction_operation_sha256(&first), expected);
    assert_eq!(transaction_operation_sha256(&reordered), expected);
}
