use crate::{invalid, Result, PROTOCOL_VERSION};
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};
use std::collections::BTreeSet;

#[derive(
    Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize, JsonSchema,
)]
#[serde(rename_all = "snake_case")]
pub enum ProductSurface {
    Engine,
    RrdHttp,
    Mcp,
    Cli,
    Connectome,
}

impl ProductSurface {
    pub const ALL: [Self; 5] = [
        Self::Engine,
        Self::RrdHttp,
        Self::Mcp,
        Self::Cli,
        Self::Connectome,
    ];
}

#[derive(
    Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize, JsonSchema,
)]
#[serde(rename_all = "snake_case")]
pub enum SurfaceDisposition {
    Available,
    Experimental,
    Planned,
    NotApplicable,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct SurfaceBinding {
    pub surface: ProductSurface,
    pub disposition: SurfaceDisposition,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub entrypoint: Option<String>,
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
            for binding in &capability.bindings {
                if !surfaces.insert(binding.surface) {
                    return invalid("product capability surfaces must be unique");
                }
                if binding.disposition == SurfaceDisposition::Available
                    && binding.entrypoint.as_deref().is_none_or(str::is_empty)
                {
                    return invalid("available product surfaces require an entrypoint");
                }
                if binding.disposition == SurfaceDisposition::NotApplicable
                    && binding.entrypoint.is_some()
                {
                    return invalid("not-applicable product surfaces cannot have an entrypoint");
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
