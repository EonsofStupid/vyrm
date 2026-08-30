//! Canonical RRFlow platform terminology.
//!
//! This is the machine-readable counterpart to `docs/platform/README.md`.
//! Public surfaces may project it, but may not maintain another term list.

use crate::ResourceKind;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PlatformTermRole {
    Product,
    Engine,
    ControlIdentity,
    ControlResource,
    WorkloadIdentity,
    InstanceAttribute,
    DeploymentAuthority,
    BuildTopology,
    LogicalResource,
    SecurityIdentity,
    DataValue,
    EnforcementPolicy,
    PhysicalTopology,
    PhysicalStorage,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct PlatformTermDefinition {
    pub term: &'static str,
    pub role: PlatformTermRole,
    pub resource_kind: Option<ResourceKind>,
}

pub const PLATFORM_TERMS: [PlatformTermDefinition; 25] = [
    term("RRFlow", PlatformTermRole::Product, None),
    term("RRD", PlatformTermRole::Engine, None),
    term(
        "organization",
        PlatformTermRole::ControlIdentity,
        Some(ResourceKind::Organization),
    ),
    term(
        "estate",
        PlatformTermRole::ControlResource,
        Some(ResourceKind::Estate),
    ),
    term(
        "project",
        PlatformTermRole::WorkloadIdentity,
        Some(ResourceKind::Project),
    ),
    term("environment", PlatformTermRole::InstanceAttribute, None),
    term(
        "instance",
        PlatformTermRole::DeploymentAuthority,
        Some(ResourceKind::Instance),
    ),
    term("workspace", PlatformTermRole::BuildTopology, None),
    term(
        "namespace",
        PlatformTermRole::LogicalResource,
        Some(ResourceKind::Namespace),
    ),
    term(
        "database",
        PlatformTermRole::LogicalResource,
        Some(ResourceKind::Database),
    ),
    term(
        "tenant",
        PlatformTermRole::SecurityIdentity,
        Some(ResourceKind::Tenant),
    ),
    term(
        "table",
        PlatformTermRole::LogicalResource,
        Some(ResourceKind::Table),
    ),
    term(
        "collection",
        PlatformTermRole::LogicalResource,
        Some(ResourceKind::Collection),
    ),
    term(
        "record",
        PlatformTermRole::LogicalResource,
        Some(ResourceKind::Record),
    ),
    term(
        "point",
        PlatformTermRole::LogicalResource,
        Some(ResourceKind::Point),
    ),
    term("vector", PlatformTermRole::DataValue, None),
    term("payload", PlatformTermRole::DataValue, None),
    term(
        "relation",
        PlatformTermRole::LogicalResource,
        Some(ResourceKind::Relation),
    ),
    term(
        "alias",
        PlatformTermRole::LogicalResource,
        Some(ResourceKind::Alias),
    ),
    term("strict mode", PlatformTermRole::EnforcementPolicy, None),
    term(
        "cluster",
        PlatformTermRole::PhysicalTopology,
        Some(ResourceKind::Cluster),
    ),
    term(
        "node",
        PlatformTermRole::PhysicalTopology,
        Some(ResourceKind::Node),
    ),
    term(
        "shard",
        PlatformTermRole::PhysicalStorage,
        Some(ResourceKind::Shard),
    ),
    term(
        "replica",
        PlatformTermRole::PhysicalStorage,
        Some(ResourceKind::Replica),
    ),
    term(
        "segment",
        PlatformTermRole::PhysicalStorage,
        Some(ResourceKind::Segment),
    ),
];

const fn term(
    term: &'static str,
    role: PlatformTermRole,
    resource_kind: Option<ResourceKind>,
) -> PlatformTermDefinition {
    PlatformTermDefinition {
        term,
        role,
        resource_kind,
    }
}
