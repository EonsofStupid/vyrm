use rrd_contract::{ProductSurface, SurfaceDisposition};

#[test]
fn every_http_operation_and_runtime_tool_has_one_authoritative_surface_row() {
    let catalogue = rrd_engine::product_capability_catalogue();
    catalogue.validate().unwrap();

    for endpoint in rrd_contract::endpoint_catalogue().endpoints {
        let capability = catalogue
            .capabilities
            .iter()
            .find(|capability| capability.id == endpoint.operation.as_str())
            .unwrap_or_else(|| panic!("missing HTTP capability {}", endpoint.operation));
        let binding = capability
            .bindings
            .iter()
            .find(|binding| binding.surface == ProductSurface::RrdHttp)
            .unwrap();
        assert_eq!(binding.disposition, SurfaceDisposition::Available);
        assert!(binding
            .entrypoint
            .as_deref()
            .unwrap()
            .ends_with(&endpoint.path));
    }

    for tool in rrd_engine::runtime_tool_catalogue() {
        let id = tool.capability_id.map(str::to_owned).unwrap_or_else(|| {
            format!(
                "runtime-{}",
                tool.name.trim_start_matches("rrflow_").replace('_', "-")
            )
        });
        let capability = catalogue
            .capabilities
            .iter()
            .find(|capability| capability.id == id)
            .unwrap_or_else(|| panic!("missing runtime capability {id}"));
        let binding = capability
            .bindings
            .iter()
            .find(|binding| binding.surface == ProductSurface::Mcp)
            .unwrap();
        assert_eq!(binding.disposition, SurfaceDisposition::Available);
        assert_eq!(binding.entrypoint.as_deref(), Some(tool.name));
    }
}

#[test]
fn required_unimplemented_foundation_is_visible_and_not_falsely_available() {
    let catalogue = rrd_engine::product_capability_catalogue();
    for id in [
        "document-ingest",
        "document-delete",
        "hybrid-memory-recall",
        "memory-reflect",
        "semantic-code-search",
        "schema-administration",
        "graph-administration",
        "vector-collection-delete",
        "full-text-search",
        "historical-rollback",
        "deployment-in-memory",
        "deployment-wasm-browser",
        "deployment-mobile-edge",
        "deployment-distributed",
        "autonomous-agent-memory",
        "managed-cloud-control-plane",
        "managed-cloud-autoscaling",
        "managed-cloud-egress-policy",
        "security-capability-controls",
    ] {
        let capability = catalogue
            .capabilities
            .iter()
            .find(|capability| capability.id == id)
            .unwrap_or_else(|| panic!("missing planned capability {id}"));
        assert!(capability.bindings.iter().all(|binding| {
            binding.disposition == SurfaceDisposition::Planned && binding.entrypoint.is_none()
        }));
    }
}
