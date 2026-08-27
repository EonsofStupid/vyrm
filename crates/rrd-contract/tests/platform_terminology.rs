use rrd_contract::{PlatformTermRole, ResourceKind, PLATFORM_TERMS};
use std::collections::BTreeSet;

#[test]
fn canonical_platform_terms_are_unique_complete_and_classified() {
    let terms = PLATFORM_TERMS
        .iter()
        .map(|definition| definition.term)
        .collect::<Vec<_>>();
    assert_eq!(terms.len(), 25);
    assert_eq!(terms.iter().copied().collect::<BTreeSet<_>>().len(), 25);
    assert_eq!(
        terms,
        [
            "RRFlow",
            "RRD",
            "organization",
            "estate",
            "project",
            "environment",
            "instance",
            "workspace",
            "namespace",
            "database",
            "tenant",
            "table",
            "collection",
            "record",
            "point",
            "vector",
            "payload",
            "relation",
            "alias",
            "strict mode",
            "cluster",
            "node",
            "shard",
            "replica",
            "segment",
        ]
    );

    for rejected in ["domain", "boundary", "workspace", "umbrella"] {
        let definition = PLATFORM_TERMS
            .iter()
            .find(|definition| definition.term == rejected);
        assert!(
            definition.is_none_or(|definition| definition.resource_kind.is_none()),
            "{rejected} must not be a public resource"
        );
    }

    assert!(PLATFORM_TERMS.iter().any(|definition| {
        definition.term == "workspace"
            && definition.role == PlatformTermRole::BuildTopology
            && definition.resource_kind.is_none()
    }));
}

#[test]
fn public_resource_vocabulary_contains_every_canonical_resource() {
    let resources = PLATFORM_TERMS
        .iter()
        .filter_map(|definition| definition.resource_kind)
        .collect::<BTreeSet<_>>();
    assert_eq!(
        resources,
        BTreeSet::from([
            ResourceKind::Organization,
            ResourceKind::Estate,
            ResourceKind::Project,
            ResourceKind::Instance,
            ResourceKind::Tenant,
            ResourceKind::Namespace,
            ResourceKind::Database,
            ResourceKind::Table,
            ResourceKind::Collection,
            ResourceKind::Record,
            ResourceKind::Point,
            ResourceKind::Relation,
            ResourceKind::Alias,
            ResourceKind::Cluster,
            ResourceKind::Node,
            ResourceKind::Shard,
            ResourceKind::Replica,
            ResourceKind::Segment,
        ])
    );
}

#[test]
fn normative_markdown_contains_the_exact_machine_term_order() {
    let path =
        std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../../docs/platform/README.md");
    let markdown = std::fs::read_to_string(path).unwrap();
    let documented = markdown
        .lines()
        .filter_map(|line| line.strip_prefix("| `"))
        .filter_map(|line| line.split_once('`').map(|(term, _)| term))
        .collect::<Vec<_>>();
    let canonical = PLATFORM_TERMS
        .iter()
        .map(|definition| definition.term)
        .collect::<Vec<_>>();
    assert_eq!(documented, canonical);
}
