use crate::{invalid, Result, MAX_MESSAGE_BYTES, PROTOCOL_VERSION};
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};
use std::collections::BTreeSet;

#[derive(
    Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize, JsonSchema,
)]
#[serde(rename_all = "snake_case")]
pub enum ProductSurface {
    Engine,
    #[serde(rename = "rrflowql")]
    Rrflowql,
    #[serde(rename = "graphql")]
    Graphql,
    RrdHttp,
    #[serde(rename = "websocket")]
    WebSocket,
    #[serde(rename = "grpc")]
    Grpc,
    Mcp,
    Cli,
    Sdk,
    Connectome,
}

impl ProductSurface {
    pub const ALL: [Self; 10] = [
        Self::Engine,
        Self::Rrflowql,
        Self::Graphql,
        Self::RrdHttp,
        Self::WebSocket,
        Self::Grpc,
        Self::Mcp,
        Self::Cli,
        Self::Sdk,
        Self::Connectome,
    ];
}

#[derive(
    Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize, JsonSchema,
)]
#[serde(rename_all = "snake_case")]
pub enum SurfaceDisposition {
    Available,
    Planned,
    Denied,
    Unavailable,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct SurfaceBinding {
    pub surface: ProductSurface,
    pub disposition: SurfaceDisposition,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub entrypoints: Vec<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub reason: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct ProductCapability {
    pub id: String,
    pub label: String,
    pub category: String,
    pub summary: String,
    pub bindings: Vec<SurfaceBinding>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct ProductCapabilityCatalogue {
    pub contract_version: u16,
    pub capabilities: Vec<ProductCapability>,
}

impl ProductCapabilityCatalogue {
    pub fn validate(&self) -> Result<()> {
        if self.contract_version != PROTOCOL_VERSION {
            return invalid("product capability catalogue version must match RRD protocol");
        }
        let mut ids = BTreeSet::new();
        for capability in &self.capabilities {
            if capability.id.is_empty()
                || capability.label.is_empty()
                || capability.category.is_empty()
                || capability.summary.is_empty()
            {
                return invalid("product capability fields must not be empty");
            }
            if !ids.insert(capability.id.as_str()) {
                return invalid("product capability ids must be unique");
            }
            if capability.bindings.len() != ProductSurface::ALL.len() {
                return invalid("every product capability must declare every surface");
            }
            let mut surfaces = BTreeSet::new();
            for (expected_surface, binding) in
                ProductSurface::ALL.into_iter().zip(&capability.bindings)
            {
                if binding.surface != expected_surface {
                    return invalid(
                        "product capability surfaces must follow the canonical surface order",
                    );
                }
                if !surfaces.insert(binding.surface) {
                    return invalid("product capability surfaces must be unique");
                }
                if binding
                    .entrypoints
                    .iter()
                    .any(|entrypoint| entrypoint.is_empty() || entrypoint.len() > MAX_MESSAGE_BYTES)
                {
                    return invalid("product surface entrypoints must be bounded and non-empty");
                }
                if binding
                    .entrypoints
                    .windows(2)
                    .any(|pair| pair[0] >= pair[1])
                {
                    return invalid("product surface entrypoints must be sorted and unique");
                }
                match binding.disposition {
                    SurfaceDisposition::Available => {
                        if binding.entrypoints.is_empty() {
                            return invalid("available product surfaces require an entrypoint");
                        }
                        if binding.reason.is_some() {
                            return invalid("available product surfaces cannot carry a reason");
                        }
                    }
                    SurfaceDisposition::Planned
                    | SurfaceDisposition::Denied
                    | SurfaceDisposition::Unavailable => {
                        if !binding.entrypoints.is_empty() {
                            return invalid(
                                "non-executable product surfaces cannot advertise entrypoints",
                            );
                        }
                        if binding.reason.as_deref().is_none_or(|reason| {
                            reason.is_empty() || reason.len() > MAX_MESSAGE_BYTES
                        }) {
                            return invalid(
                                "non-executable product surfaces require a bounded reason",
                            );
                        }
                    }
                }
            }
        }
        if self
            .capabilities
            .windows(2)
            .any(|pair| pair[0].id >= pair[1].id)
        {
            return invalid("product capabilities must be sorted by id");
        }
        Ok(())
    }
}
