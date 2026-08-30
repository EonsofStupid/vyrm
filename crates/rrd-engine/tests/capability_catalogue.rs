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
        let sdk = capability
            .bindings
            .iter()
            .find(|binding| binding.surface == ProductSurface::Sdk)
            .unwrap();
        assert_eq!(sdk.disposition, SurfaceDisposition::Available);
        assert!(sdk
            .entrypoints
            .iter()
            .any(|entrypoint| entrypoint == &format!("openapi:operation#{}", endpoint.operation)));
    }

    for endpoint in rrd_contract::endpoint_catalogue().websocket_endpoints {
        let capability = catalogue
            .capabilities
            .iter()
            .find(|capability| capability.id == endpoint.operation.as_str())
            .unwrap_or_else(|| panic!("missing WebSocket capability {}", endpoint.operation));
        let websocket = capability
            .bindings
            .iter()
            .find(|binding| binding.surface == ProductSurface::WebSocket)
            .unwrap();
        assert_eq!(websocket.disposition, SurfaceDisposition::Available);
        assert_eq!(
            websocket.entrypoints,
            vec![format!("GET {}", endpoint.path)]
        );
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
fn every_runtime_tool_has_the_generated_cli_entrypoint() {
    let catalogue = rrd_engine::product_capability_catalogue();
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
        let cli = capability
            .bindings
            .iter()
            .find(|binding| binding.surface == ProductSurface::Cli)
            .unwrap_or_else(|| panic!("missing CLI binding for {}", tool.name));
        assert_eq!(cli.disposition, SurfaceDisposition::Available);
        assert!(cli.entrypoints.iter().any(|entrypoint| {
            entrypoint == &format!("rrflow runtime call --tool {}", tool.name)
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
        "full-text-search",
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

    let rollback = catalogue
        .capabilities
        .iter()
        .find(|capability| capability.id == "historical-rollback")
        .unwrap();
    for surface in [
        ProductSurface::Engine,
        ProductSurface::RrdHttp,
        ProductSurface::Mcp,
        ProductSurface::Sdk,
        ProductSurface::Connectome,
    ] {
        let binding = rollback
            .bindings
            .iter()
            .find(|binding| binding.surface == surface)
            .unwrap();
        assert_eq!(binding.disposition, SurfaceDisposition::Available);
        assert!(!binding.entrypoints.is_empty());
    }
    let cli = rollback
        .bindings
        .iter()
        .find(|binding| binding.surface == ProductSurface::Cli)
        .unwrap();
    assert_eq!(cli.disposition, SurfaceDisposition::Available);
    assert!(cli
        .entrypoints
        .iter()
        .any(|entrypoint| entrypoint == "rrflow runtime call --tool rrflow_data_rollback"));

    let functions = catalogue
        .capabilities
        .iter()
        .find(|capability| capability.id == "embedded-functions")
        .expect("embedded function capability");
    let engine = functions
        .bindings
        .iter()
        .find(|binding| binding.surface == ProductSurface::Engine)
        .unwrap();
    assert_eq!(engine.disposition, SurfaceDisposition::Available);
    assert_eq!(engine.entrypoints.len(), 3);
    assert!(functions.bindings.iter().all(|binding| {
        binding.surface == ProductSurface::Engine
            || (binding.disposition == SurfaceDisposition::Planned
                && binding.entrypoints.is_empty()
                && binding
                    .reason
                    .as_deref()
                    .is_some_and(|reason| reason.contains("G06")))
    }));

    for id in [
        "vector-collection-delete",
        "vector-payload-index-administration",
        "vector-query-algebra",
        "vector-quantization-lifecycle",
    ] {
        let capability = catalogue
            .capabilities
            .iter()
            .find(|capability| capability.id == id)
            .unwrap_or_else(|| panic!("missing vector engine capability {id}"));
        let engine = capability
            .bindings
            .iter()
            .find(|binding| binding.surface == ProductSurface::Engine)
            .unwrap();
        assert_eq!(engine.disposition, SurfaceDisposition::Available);
        assert!(!engine.entrypoints.is_empty());
        assert!(capability.bindings.iter().all(|binding| {
            binding.surface == ProductSurface::Engine
                || (binding.disposition == SurfaceDisposition::Planned
                    && binding.entrypoints.is_empty()
                    && binding
                        .reason
                        .as_deref()
                        .is_some_and(|reason| reason.contains("G06")))
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
        for surface in [
            ProductSurface::Engine,
            ProductSurface::RrdHttp,
            ProductSurface::Mcp,
            ProductSurface::Cli,
            ProductSurface::Sdk,
            ProductSurface::Connectome,
        ] {
            let binding = capability
                .bindings
                .iter()
                .find(|binding| binding.surface == surface)
                .unwrap();
            assert_eq!(binding.disposition, SurfaceDisposition::Available);
            assert!(!binding.entrypoints.is_empty());
            assert!(binding.reason.is_none());
        }
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
fn every_capability_has_the_complete_generated_surface_taxonomy() {
    let catalogue = rrd_engine::product_capability_catalogue();
    for capability in &catalogue.capabilities {
        assert_eq!(
            capability
                .bindings
                .iter()
                .map(|binding| binding.surface)
                .collect::<Vec<_>>(),
            ProductSurface::ALL,
            "{} drifted from the canonical surface order",
            capability.id
        );
    }

    let query = catalogue
        .capabilities
        .iter()
        .find(|capability| capability.id == "query-execute")
        .unwrap();
    let rrflowql = query
        .bindings
        .iter()
        .find(|binding| binding.surface == ProductSurface::Rrflowql)
        .unwrap();
    assert_eq!(rrflowql.disposition, SurfaceDisposition::Available);
    for surface in [ProductSurface::Graphql, ProductSurface::Grpc] {
        let binding = query
            .bindings
            .iter()
            .find(|binding| binding.surface == surface)
            .unwrap();
        assert_eq!(binding.disposition, SurfaceDisposition::Unavailable);
        assert!(binding.entrypoints.is_empty());
        assert!(binding.reason.is_some());
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
