use crate::{invalid, validate_sha256, CanonicalId, QueryValue, Result};
use base64::engine::general_purpose::STANDARD;
use base64::Engine as _;
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::collections::{BTreeMap, BTreeSet};

pub const FUNCTION_CONTRACT_VERSION: u16 = 1;
pub const MAX_FUNCTIONS_PER_CATALOGUE: usize = 64;
pub const MAX_FUNCTION_TRIGGERS_PER_CATALOGUE: usize = 128;
pub const MAX_FUNCTION_SOURCE_BYTES: usize = 64 * 1024;
pub const MAX_FUNCTION_WASM_BYTES: usize = 256 * 1024;
pub const MAX_FUNCTION_INPUT_BYTES: usize = 256 * 1024;
pub const MAX_FUNCTION_OUTPUT_BYTES: usize = 256 * 1024;
pub const MAX_FUNCTION_VALUE_DEPTH: usize = 64;
pub const MAX_FUNCTION_VALUE_ITEMS: usize = 8_192;
pub const MAX_FUNCTION_JSON_SAFE_INTEGER: u64 = 9_007_199_254_740_991;
const MIN_FUNCTION_MEMORY_BYTES: u64 = 64 * 1024;
const MAX_FUNCTION_MEMORY_BYTES: u64 = 64 * 1024 * 1024;
const MIN_FUNCTION_STACK_BYTES: u32 = 16 * 1024;
const MAX_FUNCTION_STACK_BYTES: u32 = 1024 * 1024;
const MAX_FUNCTION_INTERRUPTS: u32 = 1_000_000;
const MAX_FUNCTION_FUEL: u64 = 1_000_000_000;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum FunctionRuntimeKind {
    JavaScriptEs2020,
    WebAssemblyV1,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum WebAssemblyAbi {
    /// The module exports `memory`, `rrd_alloc(i32) -> i32`, and
    /// `rrd_run(i32, i32) -> i64`. The host writes canonical JSON input at
    /// the allocated pointer. The result packs an output pointer in the high
    /// 32 bits and a UTF-8 JSON byte length in the low 32 bits.
    JsonV1,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(tag = "runtime", rename_all = "snake_case", deny_unknown_fields)]
pub enum FunctionRuntime {
    JavaScriptEs2020 {
        source: String,
        source_sha256: String,
    },
    WebAssemblyV1 {
        abi: WebAssemblyAbi,
        module_base64: String,
        module_sha256: String,
    },
}

impl FunctionRuntime {
    pub fn kind(&self) -> FunctionRuntimeKind {
        match self {
            Self::JavaScriptEs2020 { .. } => FunctionRuntimeKind::JavaScriptEs2020,
            Self::WebAssemblyV1 { .. } => FunctionRuntimeKind::WebAssemblyV1,
        }
    }

    pub fn content_sha256(&self) -> &str {
        match self {
            Self::JavaScriptEs2020 { source_sha256, .. } => source_sha256,
            Self::WebAssemblyV1 { module_sha256, .. } => module_sha256,
        }
    }

    pub fn validate(&self) -> Result<()> {
        match self {
            Self::JavaScriptEs2020 {
                source,
                source_sha256,
            } => {
                if source.trim().is_empty() || source.len() > MAX_FUNCTION_SOURCE_BYTES {
                    return invalid(format!(
                        "JavaScript source length must be in 1..={MAX_FUNCTION_SOURCE_BYTES} bytes"
                    ));
                }
                validate_sha256(source_sha256, "function source_sha256")?;
                if sha256(source.as_bytes()) != *source_sha256 {
                    return invalid("JavaScript source_sha256 does not match source bytes");
                }
            }
            Self::WebAssemblyV1 {
                module_base64,
                module_sha256,
                ..
            } => {
                validate_sha256(module_sha256, "function module_sha256")?;
                let module = STANDARD
                    .decode(module_base64)
                    .map_err(|_| crate::ContractError("WebAssembly module is not base64".into()))?;
                if module.is_empty() || module.len() > MAX_FUNCTION_WASM_BYTES {
                    return invalid(format!(
                        "WebAssembly module length must be in 1..={MAX_FUNCTION_WASM_BYTES} bytes"
                    ));
                }
                if sha256(&module) != *module_sha256 {
                    return invalid("WebAssembly module_sha256 does not match decoded bytes");
                }
            }
        }
        Ok(())
    }
}

#[derive(
    Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize, JsonSchema,
)]
#[serde(rename_all = "snake_case")]
pub enum FunctionCapability {
    /// Allows a trigger result object to be lowered into one typed append-only
    /// event in the same authoritative data transaction.
    EmitEvent,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct FunctionLimits {
    pub max_input_bytes: u32,
    pub max_output_bytes: u32,
    pub memory_bytes: u64,
    pub stack_bytes: u32,
    /// QuickJS interrupt-callback budget. The engine version pins callback
    /// scheduling; exhaustion is an uncatchable terminal failure.
    pub max_interrupts: u32,
    /// Wasm instruction fuel. Wasmi traps deterministically at exhaustion.
    pub max_fuel: u64,
}

impl FunctionLimits {
    pub fn validate(&self) -> Result<()> {
        if self.max_input_bytes == 0
            || self.max_input_bytes as usize > MAX_FUNCTION_INPUT_BYTES
            || self.max_output_bytes == 0
            || self.max_output_bytes as usize > MAX_FUNCTION_OUTPUT_BYTES
        {
            return invalid("function input/output byte limits are outside the v1 bounds");
        }
        if !(MIN_FUNCTION_MEMORY_BYTES..=MAX_FUNCTION_MEMORY_BYTES).contains(&self.memory_bytes)
            || !(MIN_FUNCTION_STACK_BYTES..=MAX_FUNCTION_STACK_BYTES).contains(&self.stack_bytes)
            || u64::from(self.stack_bytes) >= self.memory_bytes
        {
            return invalid("function memory/stack limits are outside the v1 bounds");
        }
        if self.max_interrupts == 0
            || self.max_interrupts > MAX_FUNCTION_INTERRUPTS
            || self.max_fuel == 0
            || self.max_fuel > MAX_FUNCTION_FUEL
        {
            return invalid("function interrupt/fuel limits are outside the v1 bounds");
        }
        Ok(())
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct FunctionDefinition {
    pub function_id: CanonicalId,
    pub runtime: FunctionRuntime,
    pub limits: FunctionLimits,
    #[serde(default, skip_serializing_if = "BTreeSet::is_empty")]
    pub capabilities: BTreeSet<FunctionCapability>,
}

impl FunctionDefinition {
    pub fn validate(&self) -> Result<()> {
        self.runtime.validate()?;
        self.limits.validate()
    }
}

#[derive(
    Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize, JsonSchema,
)]
#[serde(rename_all = "snake_case")]
pub enum FunctionTriggerMutation {
    AssertClaim,
    PutSchema,
    PutRecord,
    PutRelation,
    AppendEvent,
    PutVector,
    AppendSeriesSample,
    PutGeo,
    PublishObjectReference,
    RetireData,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(tag = "effect", rename_all = "snake_case", deny_unknown_fields)]
pub enum FunctionTriggerEffect {
    /// The function must return boolean true or the complete transaction is
    /// refused before any data mutation is published.
    RequireTrue,
    /// The function must return an object. It is lowered into one typed event
    /// appended in the same authoritative commit as the triggering mutation.
    AppendEvent { kind: CanonicalId },
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct FunctionTrigger {
    pub trigger_id: CanonicalId,
    pub function_id: CanonicalId,
    pub mutation: FunctionTriggerMutation,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub kind: Option<CanonicalId>,
    pub effect: FunctionTriggerEffect,
    /// V1 synchronous functions are deterministic and never retry a runtime
    /// failure internally. The enclosing idempotent transaction is the only
    /// retry boundary, so this value must be exactly one.
    pub max_attempts: u8,
}

impl FunctionTrigger {
    pub fn validate(&self, functions: &BTreeMap<CanonicalId, FunctionDefinition>) -> Result<()> {
        if self.max_attempts != 1 {
            return invalid("v1 synchronous triggers require max_attempts=1");
        }
        if self.kind.is_some()
            && matches!(
                self.mutation,
                FunctionTriggerMutation::AssertClaim
                    | FunctionTriggerMutation::PutSchema
                    | FunctionTriggerMutation::RetireData
            )
        {
            return invalid("trigger kind cannot narrow a mutation without a kind coordinate");
        }
        let function = functions
            .get(&self.function_id)
            .ok_or_else(|| crate::ContractError("trigger references an unknown function".into()))?;
        if matches!(self.effect, FunctionTriggerEffect::AppendEvent { .. })
            && !function
                .capabilities
                .contains(&FunctionCapability::EmitEvent)
        {
            return invalid("append-event triggers require the emit_event capability");
        }
        Ok(())
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct AutomationCatalogue {
    pub contract_version: u16,
    pub revision: u64,
    #[serde(default)]
    pub functions: BTreeMap<CanonicalId, FunctionDefinition>,
    #[serde(default)]
    pub triggers: BTreeMap<CanonicalId, FunctionTrigger>,
}

impl AutomationCatalogue {
    pub fn empty() -> Self {
        Self {
            contract_version: FUNCTION_CONTRACT_VERSION,
            revision: 0,
            functions: BTreeMap::new(),
            triggers: BTreeMap::new(),
        }
    }

    pub fn validate(&self) -> Result<()> {
        if self.contract_version != FUNCTION_CONTRACT_VERSION {
            return invalid("unsupported function contract version");
        }
        if self.revision == 0 && (!self.functions.is_empty() || !self.triggers.is_empty()) {
            return invalid("automation catalogue revision zero must be empty");
        }
        if self.functions.len() > MAX_FUNCTIONS_PER_CATALOGUE
            || self.triggers.len() > MAX_FUNCTION_TRIGGERS_PER_CATALOGUE
        {
            return invalid("function or trigger catalogue exceeds the v1 bound");
        }
        for (function_id, function) in &self.functions {
            if function_id != &function.function_id {
                return invalid("function catalogue key does not match function_id");
            }
            function.validate()?;
        }
        for (trigger_id, trigger) in &self.triggers {
            if trigger_id != &trigger.trigger_id {
                return invalid("trigger catalogue key does not match trigger_id");
            }
            trigger.validate(&self.functions)?;
        }
        Ok(())
    }

    pub fn sha256(&self) -> String {
        sha256(&serde_json::to_vec(self).expect("automation catalogue serializes"))
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct ReplaceAutomationCatalogue {
    pub expected_revision: u64,
    pub catalogue: AutomationCatalogue,
}

impl ReplaceAutomationCatalogue {
    pub fn validate(&self) -> Result<()> {
        self.catalogue.validate()?;
        let next_revision = self
            .expected_revision
            .checked_add(1)
            .ok_or_else(|| crate::ContractError("automation catalogue revision overflow".into()))?;
        if self.catalogue.revision != next_revision {
            return invalid("replacement catalogue revision must follow expected_revision");
        }
        Ok(())
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct ListAutomationCatalogue {}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct ExecuteFunction {
    pub function_id: CanonicalId,
    pub input: QueryValue,
}

impl ExecuteFunction {
    pub fn validate(&self) -> Result<()> {
        validate_function_value(&self.input)?;
        let bytes = serde_json::to_vec(&self.input)
            .map_err(|error| crate::ContractError(error.to_string()))?;
        if bytes.len() > MAX_FUNCTION_INPUT_BYTES {
            return invalid("function input exceeds the public v1 bound");
        }
        Ok(())
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct FunctionExecutionResult {
    pub contract_version: u16,
    pub catalogue_revision: u64,
    pub catalogue_sha256: String,
    pub function_id: CanonicalId,
    pub runtime: FunctionRuntimeKind,
    pub source_sha256: String,
    pub input_sha256: String,
    pub output_sha256: String,
    pub output: QueryValue,
    pub attempts: u8,
    pub interrupts_consumed: u32,
    pub fuel_consumed: u64,
}

impl FunctionExecutionResult {
    pub fn validate(&self) -> Result<()> {
        if self.contract_version != FUNCTION_CONTRACT_VERSION
            || self.catalogue_revision == 0
            || self.attempts != 1
        {
            return invalid("function execution coordinates are invalid");
        }
        validate_sha256(&self.catalogue_sha256, "function catalogue_sha256")?;
        validate_sha256(&self.source_sha256, "function source_sha256")?;
        validate_sha256(&self.input_sha256, "function input_sha256")?;
        validate_sha256(&self.output_sha256, "function output_sha256")?;
        validate_function_value(&self.output)?;
        let output = serde_json::to_vec(&self.output)
            .map_err(|error| crate::ContractError(error.to_string()))?;
        if output.len() > MAX_FUNCTION_OUTPUT_BYTES || sha256(&output) != self.output_sha256 {
            return invalid("function output bytes or digest are invalid");
        }
        Ok(())
    }
}

pub fn validate_function_value(value: &QueryValue) -> Result<()> {
    let mut stack = vec![(value, 0_usize)];
    let mut items = 0_usize;
    while let Some((value, depth)) = stack.pop() {
        if depth > MAX_FUNCTION_VALUE_DEPTH {
            return invalid("function value exceeds the v1 nesting-depth bound");
        }
        items = items
            .checked_add(1)
            .ok_or_else(|| crate::ContractError("function value item count overflow".into()))?;
        if items > MAX_FUNCTION_VALUE_ITEMS {
            return invalid("function value exceeds the v1 item-count bound");
        }
        match value {
            QueryValue::List(values) => {
                stack.extend(values.iter().map(|value| (value, depth + 1)));
            }
            QueryValue::Map(values) => {
                stack.extend(values.values().map(|value| (value, depth + 1)));
            }
            QueryValue::Null
            | QueryValue::Bool(_)
            | QueryValue::Integer(_)
            | QueryValue::Unsigned(_)
            | QueryValue::Decimal(_)
            | QueryValue::String(_)
            | QueryValue::Digest(_) => {}
        }
    }
    Ok(())
}

fn sha256(bytes: &[u8]) -> String {
    let digest = Sha256::digest(bytes);
    let mut output = String::with_capacity(64);
    for byte in digest {
        use std::fmt::Write as _;
        write!(&mut output, "{byte:02x}").expect("writing to string cannot fail");
    }
    output
}

#[cfg(test)]
mod tests {
    use super::*;

    fn limits() -> FunctionLimits {
        FunctionLimits {
            max_input_bytes: 4_096,
            max_output_bytes: 4_096,
            memory_bytes: 1024 * 1024,
            stack_bytes: 64 * 1024,
            max_interrupts: 1_000,
            max_fuel: 100_000,
        }
    }

    fn javascript(id: &str, source: &str) -> FunctionDefinition {
        FunctionDefinition {
            function_id: CanonicalId::new(id).unwrap(),
            runtime: FunctionRuntime::JavaScriptEs2020 {
                source: source.into(),
                source_sha256: sha256(source.as_bytes()),
            },
            limits: limits(),
            capabilities: BTreeSet::new(),
        }
    }

    #[test]
    fn catalogue_rejects_digest_identity_capability_and_retry_substitution() {
        let function = javascript("validator", "(input) => input.allowed === true");
        let function_id = function.function_id.clone();
        let trigger_id = CanonicalId::new("validate-record").unwrap();
        let mut catalogue = AutomationCatalogue {
            contract_version: FUNCTION_CONTRACT_VERSION,
            revision: 1,
            functions: [(function_id.clone(), function)].into_iter().collect(),
            triggers: [(
                trigger_id.clone(),
                FunctionTrigger {
                    trigger_id,
                    function_id,
                    mutation: FunctionTriggerMutation::PutRecord,
                    kind: Some(CanonicalId::new("person").unwrap()),
                    effect: FunctionTriggerEffect::RequireTrue,
                    max_attempts: 1,
                },
            )]
            .into_iter()
            .collect(),
        };
        catalogue.validate().unwrap();

        let definition = catalogue
            .functions
            .get_mut(&CanonicalId::new("validator").unwrap())
            .unwrap();
        let FunctionRuntime::JavaScriptEs2020 { source_sha256, .. } = &mut definition.runtime
        else {
            unreachable!()
        };
        *source_sha256 = "0".repeat(64);
        assert!(catalogue.validate().is_err());

        let mut event_function = javascript("event-writer", "() => ({ ok: true })");
        let event_function_id = event_function.function_id.clone();
        let event_trigger_id = CanonicalId::new("write-event").unwrap();
        let event_trigger = FunctionTrigger {
            trigger_id: event_trigger_id.clone(),
            function_id: event_function_id.clone(),
            mutation: FunctionTriggerMutation::AppendEvent,
            kind: None,
            effect: FunctionTriggerEffect::AppendEvent {
                kind: CanonicalId::new("derived-event").unwrap(),
            },
            max_attempts: 1,
        };
        let mut event_catalogue = AutomationCatalogue {
            contract_version: FUNCTION_CONTRACT_VERSION,
            revision: 1,
            functions: [(event_function_id.clone(), event_function.clone())]
                .into_iter()
                .collect(),
            triggers: [(event_trigger_id.clone(), event_trigger.clone())]
                .into_iter()
                .collect(),
        };
        assert!(event_catalogue.validate().is_err());
        event_function
            .capabilities
            .insert(FunctionCapability::EmitEvent);
        event_catalogue
            .functions
            .insert(event_function_id, event_function);
        event_catalogue.validate().unwrap();
        event_catalogue
            .triggers
            .get_mut(&event_trigger_id)
            .unwrap()
            .max_attempts = 2;
        assert!(event_catalogue.validate().is_err());

        let function = javascript("claim-validator", "() => true");
        let function_id = function.function_id.clone();
        let narrowed_claim = FunctionTrigger {
            trigger_id: CanonicalId::new("narrowed-claim").unwrap(),
            function_id: function_id.clone(),
            mutation: FunctionTriggerMutation::AssertClaim,
            kind: Some(CanonicalId::new("impossible-kind").unwrap()),
            effect: FunctionTriggerEffect::RequireTrue,
            max_attempts: 1,
        };
        assert!(narrowed_claim
            .validate(&BTreeMap::from([(function_id, function)]))
            .is_err());

        assert!(ReplaceAutomationCatalogue {
            expected_revision: u64::MAX,
            catalogue: AutomationCatalogue {
                contract_version: FUNCTION_CONTRACT_VERSION,
                revision: u64::MAX,
                functions: BTreeMap::new(),
                triggers: BTreeMap::new(),
            },
        }
        .validate()
        .is_err());
    }

    #[test]
    fn webassembly_module_digest_covers_decoded_bytes() {
        let module = b"\0asm\x01\0\0\0";
        let mut runtime = FunctionRuntime::WebAssemblyV1 {
            abi: WebAssemblyAbi::JsonV1,
            module_base64: STANDARD.encode(module),
            module_sha256: sha256(module),
        };
        runtime.validate().unwrap();
        let FunctionRuntime::WebAssemblyV1 { module_base64, .. } = &mut runtime else {
            unreachable!()
        };
        module_base64.push('A');
        assert!(runtime.validate().is_err());
    }

    #[test]
    fn function_values_fail_closed_on_depth_and_item_amplification() {
        let mut nested = QueryValue::Null;
        for _ in 0..=MAX_FUNCTION_VALUE_DEPTH {
            nested = QueryValue::List(vec![nested]);
        }
        assert!(validate_function_value(&nested).is_err());
        assert!(validate_function_value(&QueryValue::List(vec![
            QueryValue::Null;
            MAX_FUNCTION_VALUE_ITEMS
        ]))
        .is_err());
    }
}
