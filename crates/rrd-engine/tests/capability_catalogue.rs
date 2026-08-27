use rrd_contract::{ProductSurface, SurfaceDisposition};
use rrd_engine::WorkPlanOperation;

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
            .entrypoints
            .iter()
            .any(|entrypoint| entrypoint.ends_with(&endpoint.path)));
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
        let mcp = capability
            .bindings
            .iter()
            .find(|binding| binding.surface == ProductSurface::Mcp)
            .unwrap();
        assert_eq!(mcp.disposition, SurfaceDisposition::Available);
        assert!(mcp
            .entrypoints
            .iter()
            .any(|entrypoint| entrypoint == tool.name));

        let http = capability
            .bindings
            .iter()
            .find(|binding| binding.surface == ProductSurface::RrdHttp)
            .unwrap();
        assert_eq!(http.disposition, SurfaceDisposition::Available);
        assert!(http.entrypoints.iter().any(|entrypoint| {
            entrypoint == &format!("POST /v1/runtime/tools/invoke#{}", tool.name)
        }));

        let engine = capability
            .bindings
            .iter()
            .find(|binding| binding.surface == ProductSurface::Engine)
            .unwrap();
        assert_eq!(engine.disposition, SurfaceDisposition::Available);
        assert!(engine
            .entrypoints
            .iter()
            .any(|entrypoint| entrypoint == &format!("rrd-engine:runtime/{}", tool.name)));

        let connectome = capability
            .bindings
            .iter()
            .find(|binding| binding.surface == ProductSurface::Connectome)
            .unwrap();
        assert_eq!(connectome.disposition, SurfaceDisposition::Available);
        assert!(connectome.entrypoints.iter().any(|entrypoint| {
            entrypoint == &format!("/api/runtime/tools/invoke#{}", tool.name)
        }));
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
            binding.disposition == SurfaceDisposition::Planned
                && binding.entrypoints.is_empty()
                && binding
                    .reason
                    .as_deref()
                    .is_some_and(|reason| !reason.is_empty())
        }));
    }
}

#[test]
fn generated_work_plan_capabilities_are_available_on_every_required_surface() {
    let catalogue = rrd_engine::product_capability_catalogue();
    for operation in WorkPlanOperation::ALL {
        let capability = catalogue
            .capabilities
            .iter()
            .find(|capability| capability.id == operation.capability_id())
            .unwrap();
        assert_eq!(capability.bindings.len(), ProductSurface::ALL.len());
        assert!(capability.bindings.iter().all(|binding| {
            binding.disposition == SurfaceDisposition::Available
                && !binding.entrypoints.is_empty()
                && binding.reason.is_none()
        }));
        let cli = capability
            .bindings
            .iter()
            .find(|binding| binding.surface == ProductSurface::Cli)
            .unwrap();
        assert!(cli
            .entrypoints
            .iter()
            .any(|entrypoint| entrypoint == operation.cli_command()));
    }
}

#[test]
fn every_non_executable_surface_has_an_explicit_reason() {
    let catalogue = rrd_engine::product_capability_catalogue();
    catalogue.validate().unwrap();
    for capability in catalogue.capabilities {
        for binding in capability.bindings {
            if binding.disposition != SurfaceDisposition::Available {
                assert!(
                    binding.entrypoints.is_empty(),
                    "{} exposed a false entrypoint",
                    capability.id
                );
                assert!(
                    binding
                        .reason
                        .as_deref()
                        .is_some_and(|reason| !reason.is_empty()),
                    "{} omitted the reason for {:?}",
                    capability.id,
                    binding.surface
                );
            }
        }
    }
}
