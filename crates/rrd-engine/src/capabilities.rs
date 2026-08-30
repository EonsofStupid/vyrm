use crate::runtime::runtime_tool_catalogue;
use rrd_contract::{
    endpoint_catalogue, EndpointAction, ProductCapability, ProductCapabilityCatalogue,
    ProductSurface, SurfaceBinding, SurfaceDisposition, WorkPlanOperation, PROTOCOL_VERSION,
};

/// Builds the single cross-surface capability truth from executable engine and
/// public protocol registries. Planned gaps are explicit catalogue rows, not
/// fake adapter entrypoints.
pub fn product_capability_catalogue() -> ProductCapabilityCatalogue {
    let endpoints = endpoint_catalogue();
    let mut capabilities = endpoints
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
                bindings: surface_bindings(
                    unavailable(NO_SURFACE_BINDING_REASON),
                    [
                        (ProductSurface::Engine, available(engine_binding)),
                        (
                            ProductSurface::RrdHttp,
                            available(format!("{} {}", method(endpoint.method), endpoint.path)),
                        ),
                        (
                            ProductSurface::Sdk,
                            available(format!("openapi:operation#{}", endpoint.operation)),
                        ),
                        (
                            ProductSurface::Connectome,
                            planned("Connectome operation control is scheduled by G09."),
                        ),
                    ],
                ),
            }
        })
        .collect::<Vec<_>>();

    if let Some(query) = capabilities
        .iter_mut()
        .find(|capability| capability.id == "query-execute")
    {
        expose(
            binding(query, ProductSurface::Rrflowql),
            "rrd-query:RrflowQlQuery::parse_and_bind",
        );
    }

    for endpoint in endpoints.websocket_endpoints {
        let id = endpoint.operation.as_str().to_owned();
        capabilities.push(ProductCapability {
            label: title(&id),
            category: category(&id).to_owned(),
            summary: format!(
                "RRD WebSocket {}: {} to {}.",
                endpoint.path, endpoint.client_frame_type, endpoint.server_frame_type
            ),
            id,
            bindings: surface_bindings(
                unavailable(NO_SURFACE_BINDING_REASON),
                [
                    (
                        ProductSurface::Engine,
                        available(format!(
                            "rrd-engine:RrdOperation::{:?}",
                            endpoint.connect_action
                        )),
                    ),
                    (
                        ProductSurface::WebSocket,
                        available(format!("GET {}", endpoint.path)),
                    ),
                    (
                        ProductSurface::Sdk,
                        available("rrd-client:RrdClient::connect_subscription"),
                    ),
                    (
                        ProductSurface::Connectome,
                        planned("Connectome subscription streaming is scheduled by G09."),
                    ),
                ],
            ),
        });
    }

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
                binding(capability, ProductSurface::Cli),
                format!("rrflow runtime call --tool {}", tool.name),
            );
            expose(
                binding(capability, ProductSurface::Sdk),
                format!("openapi:runtime-tool-invoke#{}", tool.name),
            );
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
                surface_bindings(
                    unavailable(NO_SURFACE_BINDING_REASON),
                    [
                        (
                            ProductSurface::Engine,
                            available(format!("rrd-engine:runtime/{}", tool.name)),
                        ),
                        (
                            ProductSurface::RrdHttp,
                            available(format!("POST /v1/runtime/tools/invoke#{}", tool.name)),
                        ),
                        (ProductSurface::Mcp, available(tool.name)),
                        (
                            ProductSurface::Cli,
                            available(format!("rrflow runtime call --tool {}", tool.name)),
                        ),
                        (
                            ProductSurface::Sdk,
                            available(format!("openapi:runtime-tool-invoke#{}", tool.name)),
                        ),
                        (
                            ProductSurface::Connectome,
                            available(format!("/api/runtime/tools/invoke#{}", tool.name)),
                        ),
                    ],
                )
            },
        });
        if let Some(operation) = WorkPlanOperation::ALL
            .into_iter()
            .find(|operation| operation.runtime_tool_name() == tool.name)
        {
            let capability = capabilities
                .last_mut()
                .expect("runtime capability was appended");
            expose(
                binding(capability, ProductSurface::Cli),
                operation.cli_command(),
            );
        }
    }

    let mut embedded_functions = ProductCapability {
        id: "embedded-functions".into(),
        label: "Embedded functions".into(),
        category: "query".into(),
        summary: "Run revision-pinned JavaScript ES2020 and portable WebAssembly JSON functions, including synchronous transaction triggers, inside deterministic engine-owned sandboxes.".into(),
        bindings: surface_bindings(
            planned(FUNCTION_SURFACE_PLAN_REASON),
            [(
                ProductSurface::Engine,
                available("rrd-engine:RrdEngine::execute_function"),
            )],
        ),
    };
    expose(
        binding(&mut embedded_functions, ProductSurface::Engine),
        "rrd-engine:RrdEngine::automation_catalogue",
    );
    expose(
        binding(&mut embedded_functions, ProductSurface::Engine),
        "rrd-engine:RrdEngine::replace_automation_catalogue",
    );
    capabilities.push(embedded_functions);

    let mut native_inference = ProductCapability {
        id: "native-embedding-inference".into(),
        label: "Native Embedding Inference".into(),
        category: "ai_runtime".into(),
        summary: "Run exact-model bounded batch inference and atomic embed-and-vector-search through an engine-owned registry with explicit local/offline and remote-provider trust boundaries.".into(),
        bindings: surface_bindings(
            planned(ENGINE_SURFACE_PLAN_REASON),
            [(
                ProductSurface::Engine,
                available("rrd-engine:RrdEngine::generate_embeddings"),
            )],
        ),
    };
    expose(
        binding(&mut native_inference, ProductSurface::Engine),
        "rrd-engine:RrdEngine::list_embedding_models",
    );
    expose(
        binding(&mut native_inference, ProductSurface::Engine),
        "rrd-engine:RrdEngine::embed_and_search_vectors",
    );
    capabilities.push(native_inference);

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
            bindings: surface_bindings(
                planned(FOUNDATION_PLAN_REASON),
                std::iter::empty(),
            ),
        });
    }

    if let Some(capability) = capabilities
        .iter_mut()
        .find(|capability| capability.id == "vector-collection-delete")
    {
        capability.summary = "Delete an empty collection through a durable idempotent engine operation after proving no live/future points or active approximate artifacts can be orphaned.".into();
        expose(
            binding(capability, ProductSurface::Engine),
            "rrd-engine:RrdEngine::delete_vector_collection",
        );
        for outward in capability
            .bindings
            .iter_mut()
            .filter(|binding| binding.surface != ProductSurface::Engine)
        {
            let surface = outward.surface;
            *outward = planned(ENGINE_SURFACE_PLAN_REASON);
            outward.surface = surface;
        }
    }
    capabilities.push(ProductCapability {
        id: "vector-payload-index-administration".into(),
        label: "Vector payload index administration".into(),
        category: "vector".into(),
        summary: "Ensure, list, and delete typed collection payload indexes through the revisioned RRD vector catalogue.".into(),
        bindings: surface_bindings(
            planned(ENGINE_SURFACE_PLAN_REASON),
            [(
                ProductSurface::Engine,
                available("rrd-engine:RrdEngine::ensure_vector_payload_index,list_vector_payload_indexes,delete_vector_payload_index"),
            )],
        ),
    });
    capabilities.push(ProductCapability {
        id: "vector-query-algebra".into(),
        label: "Vector query algebra".into(),
        category: "vector".into(),
        summary: "Execute bounded nested keyword, dense, sparse, multivector, recommendation, discovery, fusion, reranking, diversity, grouping, facet, and matrix stages at one authoritative read stamp.".into(),
        bindings: surface_bindings(
            planned(ENGINE_SURFACE_PLAN_REASON),
            [(
                ProductSurface::Engine,
                available("rrd-engine:RrdEngine::execute_retrieval_query"),
            )],
        ),
    });
    capabilities.push(ProductCapability {
        id: "vector-quantization-lifecycle".into(),
        label: "Vector quantization lifecycle".into(),
        category: "vector".into(),
        summary: "Build, list, activate, and retire immutable scalar, product, binary, and TurboQuant artifacts while preserving canonical exact reranking.".into(),
        bindings: surface_bindings(
            planned(ENGINE_SURFACE_PLAN_REASON),
            [(
                ProductSurface::Engine,
                available("rrd-engine:RrdEngine::activate_vector_quantization_artifact,build_vector_quantization_artifact,list_vector_quantization_artifacts,retire_vector_quantization_artifact"),
            )],
        ),
    });
    capabilities.push(ProductCapability {
        id: "vector-memory-residency".into(),
        label: "Vector memory residency".into(),
        category: "vector".into(),
        summary: "Enforce per-named-vector pinned, byte-bounded cached LRU, and transient cold mmap/owned artifact placement with exact pressure fallback and restart reconstruction.".into(),
        bindings: surface_bindings(
            planned(ENGINE_SURFACE_PLAN_REASON),
            [(
                ProductSurface::Engine,
                available("rrd-engine:RrdEngine::search_vectors,vector_residency_snapshot"),
            )],
        ),
    });

    capabilities.sort_by(|left, right| left.id.cmp(&right.id));
    ProductCapabilityCatalogue {
        contract_version: PROTOCOL_VERSION,
        capabilities,
    }
}

fn surface_bindings(
    default: SurfaceBinding,
    overrides: impl IntoIterator<Item = (ProductSurface, SurfaceBinding)>,
) -> Vec<SurfaceBinding> {
    let mut bindings = ProductSurface::ALL
        .into_iter()
        .map(|surface| {
            let mut binding = default.clone();
            binding.surface = surface;
            binding
        })
        .collect::<Vec<_>>();
    for (surface, mut replacement) in overrides {
        replacement.surface = surface;
        *bindings
            .iter_mut()
            .find(|binding| binding.surface == surface)
            .expect("canonical product surface must have a generated binding") = replacement;
    }
    bindings
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
const FUNCTION_SURFACE_PLAN_REASON: &str =
    "The engine implementation is available; generated HTTP, MCP, CLI, SDK, and Connectome bindings are owned by G06.";
const ENGINE_SURFACE_PLAN_REASON: &str =
    "The engine implementation is available; generated outward bindings are owned by G06.";
const NO_SURFACE_BINDING_REASON: &str =
    "No executable binding for this operation exists on this product surface.";

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
