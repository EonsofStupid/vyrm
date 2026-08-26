use super::*;

pub(super) fn parse_correlation(value: &str) -> std::result::Result<CorrelationId, ApiError> {
    CorrelationId::new(value)
        .map_err(|error| ApiError::new(ErrorCode::InvalidArgument, error.to_string(), false))
}

pub(super) fn session_action<'a>(path: &'a str, action: &str) -> Option<&'a str> {
    path.strip_prefix("/v1/sessions/")?
        .strip_suffix(&format!("/{action}"))
        .filter(|id| !id.is_empty() && !id.contains('/'))
}

pub(super) fn session_id(path: &str) -> Option<&str> {
    let id = path.strip_prefix("/v1/sessions/")?;
    (!id.is_empty() && !id.contains('/')).then_some(id)
}

pub(super) fn transaction_action<'a>(path: &'a str, action: &str) -> Option<&'a str> {
    path.strip_prefix("/v1/transactions/")?
        .strip_suffix(&format!("/{action}"))
        .filter(|id| !id.is_empty() && !id.contains('/'))
}

pub(super) fn transaction_id(path: &str) -> Option<&str> {
    let id = path.strip_prefix("/v1/transactions/")?;
    (!id.is_empty() && !id.contains('/')).then_some(id)
}

pub(super) fn estate_action<'a>(path: &'a str, action: &str) -> Option<&'a str> {
    path.strip_prefix("/v1/estates/")?
        .strip_suffix(&format!("/{action}"))
        .filter(|id| !id.is_empty() && !id.contains('/'))
}

pub(super) fn capabilities(
    instance: &CanonicalId,
    backend: CanonicalId,
    security_enforced: bool,
    tls_enabled: bool,
) -> ServiceCapabilities {
    let mut capabilities = vec![
        CapabilityDescriptor {
            name: CanonicalId::new("changefeed-follow").unwrap(),
            contract_version: 1,
            status: CapabilityStatus::Experimental,
            limits: BTreeMap::from([(
                CanonicalId::new("max-wait-ms").unwrap(),
                rrd_contract::MAX_CHANGEFEED_WAIT_MS,
            )]),
            limitation: Some(
                "bounded authenticated long-poll over retained replay; streaming transport, durable subscription leases, and server push remain open"
                    .into(),
            ),
        },
        CapabilityDescriptor {
            name: CanonicalId::new("changefeed-replay").unwrap(),
            contract_version: 1,
            status: CapabilityStatus::Experimental,
            limits: BTreeMap::from([(
                CanonicalId::new("max-page-changes").unwrap(),
                rrd_contract::MAX_CHANGEFEED_PAGE,
            )]),
            limitation: Some(
                "authenticated retained cursor replay with lossless claim and typed data snapshots"
                    .into(),
            ),
        },
        CapabilityDescriptor {
            name: CanonicalId::new("claim-transactions").unwrap(),
            contract_version: 1,
            status: CapabilityStatus::Experimental,
            limits: BTreeMap::from([(
                CanonicalId::new("max-mutations").unwrap(),
                rrd_contract::MAX_TRANSACTION_CLAIMS as u64,
            )]),
            limitation: Some(format!("legacy claim-only scope on {backend}")),
        },
        CapabilityDescriptor {
            name: CanonicalId::new("multi-model-transactions").unwrap(),
            contract_version: 1,
            status: CapabilityStatus::Experimental,
            limits: BTreeMap::from([(
                CanonicalId::new("max-mutations").unwrap(),
                rrd_contract::MAX_TRANSACTION_CLAIMS as u64,
            )]),
            limitation: Some(
                "atomic schema, claim, record, relation, event, vector, series, geo, and pre-staged object-reference commits; mutating RRFlowQL and read-your-writes remain open"
                    .into(),
            ),
        },
        CapabilityDescriptor {
            name: CanonicalId::new("exact-rrflowql-query").unwrap(),
            contract_version: 1,
            status: CapabilityStatus::Experimental,
            limits: BTreeMap::from([
                (
                    CanonicalId::new("max-query-bytes").unwrap(),
                    rrd_contract::MAX_QUERY_BYTES as u64,
                ),
                (
                    CanonicalId::new("max-output-bytes").unwrap(),
                    rrd_contract::MAX_QUERY_OUTPUT_BYTES,
                ),
            ]),
            limitation: Some(
                "exact session-scoped RRFlowQL/RRD query engine reads; mutating RRFlowQL and live queries remain open"
                    .into(),
            ),
        },
        CapabilityDescriptor {
            name: CanonicalId::new("endpoint-catalogue").unwrap(),
            contract_version: 1,
            status: CapabilityStatus::Available,
            limits: BTreeMap::from([(
                CanonicalId::new("endpoint-count").unwrap(),
                rrd_contract::endpoint_catalogue().endpoints.len() as u64,
            )]),
            limitation: Some(
                "machine-readable operation catalogue and OpenAPI 3.1 schemas; generated language packages and shared conformance remain F5 work"
                    .into(),
            ),
        },
        CapabilityDescriptor {
            name: CanonicalId::new("estate-authority-read").unwrap(),
            contract_version: 1,
            status: CapabilityStatus::Experimental,
            limits: BTreeMap::new(),
            limitation: Some(
                "read-only estate snapshots; estate mutations remain unavailable"
                    .into(),
            ),
        },
        CapabilityDescriptor {
            name: CanonicalId::new("lifecycle-journal").unwrap(),
            contract_version: 1,
            status: CapabilityStatus::Available,
            limits: BTreeMap::new(),
            limitation: None,
        },
        CapabilityDescriptor {
            name: CanonicalId::new("local-transport-leases").unwrap(),
            contract_version: 1,
            status: CapabilityStatus::Experimental,
            limits: BTreeMap::from([(
                CanonicalId::new("max-body-bytes").unwrap(),
                RRD_MAX_BODY_BYTES as u64,
            )]),
            limitation: Some(if security_enforced {
                if tls_enabled {
                    "principal-authenticated policy-bound sessions over TLS 1.3 mutual authentication"
                        .into()
                } else {
                    "principal-authenticated policy-bound loopback sessions; configure mTLS for remote transport"
                        .into()
                }
            } else {
                "development availability leases; no security authority is initialized".into()
            }),
        },
        CapabilityDescriptor {
            name: CanonicalId::new("logical-backup-restore").unwrap(),
            contract_version: 1,
            status: CapabilityStatus::Experimental,
            limits: BTreeMap::new(),
            limitation: Some(
                "content-authenticated logical backup and restore-to-generated-new-root; object payloads are referenced-only and restore never switches the active instance"
                    .into(),
            ),
        },
        CapabilityDescriptor {
            name: CanonicalId::new("remote-listen").unwrap(),
            contract_version: 1,
            status: if tls_enabled {
                CapabilityStatus::Experimental
            } else {
                CapabilityStatus::Unavailable
            },
            limits: BTreeMap::new(),
            limitation: Some(if tls_enabled {
                "TLS 1.3 mTLS plus application policy are enforced; certificate reload/revocation and distributed qualification remain open"
                    .into()
            } else {
                "plain HTTP is loopback-only; configure mTLS and initialize security for remote bind"
                    .into()
            }),
        },
        CapabilityDescriptor {
            name: CanonicalId::new("security-policy").unwrap(),
            contract_version: 1,
            status: if security_enforced {
                CapabilityStatus::Experimental
            } else {
                CapabilityStatus::Unavailable
            },
            limits: BTreeMap::new(),
            limitation: Some(if security_enforced {
                "persistent principals and exact deny-by-default endpoint policy; provisioning and TLS remain open"
                    .into()
            } else {
                "no persistent security authority is initialized for this instance".into()
            }),
        },
        CapabilityDescriptor {
            name: CanonicalId::new("security-audit").unwrap(),
            contract_version: 1,
            status: if security_enforced {
                CapabilityStatus::Experimental
            } else {
                CapabilityStatus::Unavailable
            },
            limits: BTreeMap::from([(
                CanonicalId::new("max-page-records").unwrap(),
                MAX_AUDIT_PAGE_RECORDS,
            )]),
            limitation: Some(if security_enforced {
                "redacted authenticated-journal audit for routed HTTP outcomes; atomic authorization reservation and external archival remain open"
                    .into()
            } else {
                "audit requires initialized persistent security authority".into()
            }),
        },
        CapabilityDescriptor {
            name: CanonicalId::new("vector-search").unwrap(),
            contract_version: 1,
            status: CapabilityStatus::Experimental,
            limits: BTreeMap::from([
                (
                    CanonicalId::new("max-scanned-changes").unwrap(),
                    rrd_contract::MAX_VECTOR_SEARCH_CHANGES,
                ),
                (
                    CanonicalId::new("max-top-k").unwrap(),
                    rrd_contract::MAX_VECTOR_SEARCH_TOP_K,
                ),
            ]),
            limitation: Some(
                "journaled collection and named-vector administration plus exact canonical dense, sparse, and multi-vector search; point/payload administration, public filters, and persisted HNSW/TurboQuant artifact serving remain open"
                    .into(),
            ),
        },
    ];
    capabilities.sort_by(|left, right| left.name.cmp(&right.name));
    ServiceCapabilities {
        protocol: PROTOCOL.into(),
        protocol_version: PROTOCOL_VERSION,
        implementation: CanonicalId::new("rrflow").unwrap(),
        implementation_version: env!("CARGO_PKG_VERSION").into(),
        deployment_mode: DeploymentMode::LocalServer,
        instance: ResourceId {
            kind: ResourceKind::Instance,
            id: instance.clone(),
        },
        capabilities,
    }
}

pub(super) fn unix_time_ms() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_millis()
        .try_into()
        .unwrap_or(u64::MAX)
}
