use crate::runtime::runtime_tool_catalogue;
use rrd_contract::{
    endpoint_catalogue, EndpointAction, ProductCapability, ProductCapabilityCatalogue,
    ProductSurface, SurfaceBinding, SurfaceDisposition, WorkPlanOperation, PROTOCOL_VERSION,
};

/// Builds the single cross-surface capability truth from executable engine and
/// public protocol registries. Planned gaps are explicit catalogue rows, not
/// fake adapter entrypoints.
pub fn product_capability_catalogue() -> ProductCapabilityCatalogue {
    let mut capabilities = endpoint_catalogue()
        .endpoints
        .into_iter()
        .map(|endpoint| {
            let id = endpoint.operation.as_str().to_owned();
            let engine_binding = match endpoint.action {
                EndpointAction::Fixed { action } => {
                    format!("rrd-engine:RrdOperation::{action:?}")
                }
                EndpointAction::RuntimeToolDescriptor => {
                    "rrd-engine:runtime-tool-descriptor-action".into()
                }
            };
            ProductCapability {
                label: title(&id),
                category: category(&id).to_owned(),
                summary: format!(
                    "RRD {} {}: {} to {}.",
                    method(endpoint.method),
                    endpoint.path,
                    endpoint.request_type,
                    endpoint.response_type
                ),
                id,
                bindings: bindings(
                    available(engine_binding),
                    available(format!("{} {}", method(endpoint.method), endpoint.path)),
                    unavailable("No MCP runtime tool maps this HTTP operation."),
                    unavailable("No RRFlow CLI command maps this HTTP operation."),
                    planned("Connectome operation control is scheduled by G09."),
                ),
            }
        })
        .collect::<Vec<_>>();

    for tool in runtime_tool_catalogue() {
        let suffix = tool.name.trim_start_matches("rrflow_").replace('_', "-");
        let id = tool
            .capability_id
            .map(str::to_owned)
            .unwrap_or_else(|| format!("runtime-{suffix}"));
        if let Some(capability) = capabilities
            .iter_mut()
            .find(|capability| capability.id == id)
        {
            expose(
                binding(capability, ProductSurface::Engine),
                format!("rrd-engine:runtime/{}", tool.name),
            );
            expose(
                binding(capability, ProductSurface::RrdHttp),
                format!("POST /v1/runtime/tools/invoke#{}", tool.name),
            );
            expose(binding(capability, ProductSurface::Mcp), tool.name);
            expose(
                binding(capability, ProductSurface::Connectome),
                format!("/api/runtime/tools/invoke#{}", tool.name),
            );
            if let Some(operation) = WorkPlanOperation::ALL
                .into_iter()
                .find(|operation| operation.runtime_tool_name() == tool.name)
            {
                expose(
                    binding(capability, ProductSurface::Cli),
                    operation.cli_command(),
                );
            }
            continue;
        }
        capabilities.push(ProductCapability {
            id,
            label: title(&suffix),
            category: "ai_runtime".into(),
            summary: tool.description.into(),
            bindings: {
                let operation = WorkPlanOperation::ALL
                    .into_iter()
                    .find(|operation| operation.runtime_tool_name() == tool.name);
                bindings(
                    available(format!("rrd-engine:runtime/{}", tool.name)),
                    available(format!("POST /v1/runtime/tools/invoke#{}", tool.name)),
                    available(tool.name),
                    operation.map_or_else(
                        || unavailable("No RRFlow CLI command maps this runtime tool."),
                        |operation| available(operation.cli_command()),
                    ),
                    available(format!("/api/runtime/tools/invoke#{}", tool.name)),
                )
            },
        });
    }

    for (id, label, category, summary) in [
        (
            "document-ingest",
            "Document ingestion",
            "documents",
            "Chunk, overlap, parse, and ingest local documents through one governed engine transaction.",
        ),
        (
            "document-delete",
            "Document deletion",
            "documents",
            "Retire or delete ingested documents and all governed projections without orphaned indexes.",
        ),
        (
            "hybrid-memory-recall",
            "Hybrid memory recall",
            "retrieval",
            "Fuse lexical, dense, sparse, provenance, recency, and graph evidence in one bounded recall plan.",
        ),
        (
            "memory-reflect",
            "Memory reflection",
            "ai_runtime",
            "Distill recorded evidence into attributable candidate learnings without silently replacing source memory.",
        ),
        (
            "semantic-code-search",
            "Semantic code search",
            "retrieval",
            "Search parsed code structure and embeddings with source-complete routing and freshness evidence.",
        ),
        (
            "schema-administration",
            "Schema administration",
            "schema",
            "Inspect and mutate the shared multi-model catalogue through governed public operations.",
        ),
        (
            "graph-administration",
            "Graph administration",
            "graph",
            "Create, inspect, traverse, and retire graph records and relations through the shared transaction boundary.",
        ),
        (
            "vector-collection-delete",
            "Vector collection deletion",
            "vector",
            "Delete a vector collection and its physical artifacts through an audited, recoverable operation.",
        ),
        (
            "full-text-search",
            "Full-text search",
            "retrieval",
            "Index and query analyzed text with bounded scoring evidence through the shared query planner.",
        ),
        (
            "historical-rollback",
            "Historical rollback",
            "temporal",
            "Create an explicit audited forward mutation that restores a selected historical state without rewriting retained history.",
        ),
        (
            "deployment-in-memory",
            "In-memory deployment",
            "deployment",
            "Run the complete RRD logical contract against an ephemeral in-memory authority for bounded cache and test workloads.",
        ),
        (
            "deployment-wasm-browser",
            "WebAssembly browser deployment",
            "deployment",
            "Run a qualified embedded RRD profile inside browser and WebAssembly constraints without changing logical semantics.",
        ),
        (
            "deployment-mobile-edge",
            "Mobile and edge deployment",
            "deployment",
            "Run a resource-bounded offline RRD authority on mobile and edge targets with explicit synchronization semantics.",
        ),
        (
            "deployment-distributed",
            "Distributed deployment",
            "deployment",
            "Run one horizontally scalable RRD authority with qualified consensus, placement, recovery, and consistency semantics.",
        ),
        (
            "autonomous-agent-memory",
            "Autonomous agent memory",
            "ai_runtime",
            "Persist, retrieve, reflect, and retire attributable agent memory through RRD without creating a second source of truth.",
        ),
        (
            "managed-cloud-control-plane",
            "Managed cloud control plane",
            "cloud",
            "Provision, operate, upgrade, recover, and observe tenant-isolated RRD instances through a managed control plane.",
        ),
        (
            "managed-cloud-autoscaling",
            "Managed cloud autoscaling",
            "cloud",
            "Scale qualified RRD resources from measured demand while preserving availability and transaction guarantees.",
        ),
        (
            "managed-cloud-egress-policy",
            "Managed cloud egress policy",
            "cloud",
            "Apply explicit deny-by-default outbound network policy to governed inference and integration workloads.",
        ),
        (
            "security-capability-controls",
            "Granular capability controls",
            "security",
            "Administer least-privilege operation and resource capabilities through the one RRD security authority.",
        ),
    ] {
        if let Some(capability) = capabilities
            .iter_mut()
            .find(|capability| capability.id == id)
        {
            capability.label = label.into();
            capability.category = category.into();
            capability.summary = summary.into();
            continue;
        }
        capabilities.push(ProductCapability {
            id: id.into(),
            label: label.into(),
            category: category.into(),
            summary: summary.into(),
            bindings: bindings(
                planned(FOUNDATION_PLAN_REASON),
                planned(FOUNDATION_PLAN_REASON),
                planned(FOUNDATION_PLAN_REASON),
                planned(FOUNDATION_PLAN_REASON),
                planned(FOUNDATION_PLAN_REASON),
            ),
        });
    }

    capabilities.sort_by(|left, right| left.id.cmp(&right.id));
    ProductCapabilityCatalogue {
        contract_version: PROTOCOL_VERSION,
        capabilities,
    }
}

fn bindings(
    engine: SurfaceBinding,
    rrd_http: SurfaceBinding,
    mcp: SurfaceBinding,
    cli: SurfaceBinding,
    connectome: SurfaceBinding,
) -> Vec<SurfaceBinding> {
    [engine, rrd_http, mcp, cli, connectome]
        .into_iter()
        .zip(ProductSurface::ALL)
        .map(|(mut binding, surface)| {
            binding.surface = surface;
            binding
        })
        .collect()
}

fn available(entrypoint: impl Into<String>) -> SurfaceBinding {
    SurfaceBinding {
        surface: ProductSurface::Engine,
        disposition: SurfaceDisposition::Available,
        entrypoints: vec![entrypoint.into()],
        reason: None,
    }
}

fn planned(reason: impl Into<String>) -> SurfaceBinding {
    SurfaceBinding {
        surface: ProductSurface::Engine,
        disposition: SurfaceDisposition::Planned,
        entrypoints: Vec::new(),
        reason: Some(reason.into()),
    }
}

fn unavailable(reason: impl Into<String>) -> SurfaceBinding {
    SurfaceBinding {
        surface: ProductSurface::Engine,
        disposition: SurfaceDisposition::Unavailable,
        entrypoints: Vec::new(),
        reason: Some(reason.into()),
    }
}

fn binding(capability: &mut ProductCapability, surface: ProductSurface) -> &mut SurfaceBinding {
    capability
        .bindings
        .iter_mut()
        .find(|binding| binding.surface == surface)
        .expect("every product capability must declare every surface")
}

fn expose(binding: &mut SurfaceBinding, entrypoint: impl Into<String>) {
    binding.disposition = SurfaceDisposition::Available;
    binding.reason = None;
    binding.entrypoints.push(entrypoint.into());
    binding.entrypoints.sort();
    binding.entrypoints.dedup();
}

const FOUNDATION_PLAN_REASON: &str =
    "Scheduled by the checked-in RRFlow foundation work plan; no executable implementation is available yet.";

fn category(id: &str) -> &str {
    id.split_once('-').map_or("service", |(prefix, _)| prefix)
}

fn title(id: &str) -> String {
    id.split('-')
        .map(|word| {
            let mut chars = word.chars();
            chars
                .next()
                .map(|first| format!("{}{}", first.to_ascii_uppercase(), chars.as_str()))
                .unwrap_or_default()
        })
        .collect::<Vec<_>>()
        .join(" ")
}

fn method(method: rrd_contract::HttpMethod) -> &'static str {
    match method {
        rrd_contract::HttpMethod::Get => "GET",
        rrd_contract::HttpMethod::Post => "POST",
        rrd_contract::HttpMethod::Delete => "DELETE",
    }
}
