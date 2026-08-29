use super::security::AuditEvent;
use super::*;
use base64::engine::general_purpose::STANDARD;
use base64::Engine as _;
use rquickjs::{CatchResultExt as _, Context as JavaScriptContext, Runtime as JavaScriptRuntime};
use std::sync::atomic::{AtomicU32, Ordering};
use std::sync::Arc;

const AUTOMATION_HEAD_FORMAT_VERSION: u16 = 1;

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct AutomationHead {
    format_version: u16,
    revision: u64,
    catalogue_sha256: String,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(in crate::engine) struct ExecutedFunction {
    pub output: QueryValue,
    pub interrupts_consumed: u32,
    pub fuel_consumed: u64,
}

impl RrdEngine {
    #[allow(clippy::too_many_arguments)]
    pub fn replace_automation_catalogue(
        &self,
        session_id: &CorrelationId,
        token: &CorrelationId,
        request: &ReplaceAutomationCatalogue,
        now: u64,
        request_id: &str,
        operation_id: &str,
    ) -> Result<AutomationCatalogue> {
        request
            .validate()
            .map_err(|error| ServiceError::Contract(error.to_string()))?;
        self.authorize(
            session_id,
            token,
            SecurityAction::FunctionCatalogueWrite,
            now,
            request_id,
            operation_id,
        )?;
        let _transaction_guard = self
            .transaction_gate
            .lock()
            .map_err(|_| ServiceError::Storage("engine transaction gate is poisoned".into()))?;
        let head_key = automation_head_key(&self.instance);
        let expected_head = self.storage.control_record(&head_key)?;
        let current = self.load_automation_catalogue_from_head(expected_head.as_deref())?;
        if current.revision == request.catalogue.revision
            && current.sha256() == request.catalogue.sha256()
        {
            return Ok(current);
        }
        if current.revision != request.expected_revision {
            return Err(ServiceError::StorageConflict(format!(
                "automation catalogue expected revision {} but current revision is {}",
                request.expected_revision, current.revision
            )));
        }
        let catalogue_bytes = serde_json::to_vec(&request.catalogue).map_err(contract_json)?;
        let catalogue_sha256 = request.catalogue.sha256();
        let revision_key = automation_revision_key(&self.instance, request.catalogue.revision);
        if self.storage.control_record(&revision_key)?.is_some() {
            return Err(ServiceError::StorageConflict(
                "automation catalogue revision identity already exists".into(),
            ));
        }
        let head = AutomationHead {
            format_version: AUTOMATION_HEAD_FORMAT_VERSION,
            revision: request.catalogue.revision,
            catalogue_sha256,
        };
        self.storage.commit_control_batch(&[
            ControlTransition {
                key: revision_key,
                expected: None,
                replacement: Some(catalogue_bytes),
                at: now,
                actor: format!("session:{}", session_id.as_str()),
                action: "automation.catalogue.revision_published".into(),
                request_id: request_id.into(),
                operation_id: operation_id.into(),
            },
            ControlTransition {
                key: head_key,
                expected: expected_head,
                replacement: Some(serde_json::to_vec(&head).map_err(contract_json)?),
                at: now,
                actor: format!("session:{}", session_id.as_str()),
                action: "automation.catalogue.head_advanced".into(),
                request_id: request_id.into(),
                operation_id: operation_id.into(),
            },
        ])?;
        Ok(request.catalogue.clone())
    }

    #[allow(clippy::too_many_arguments)]
    pub fn automation_catalogue(
        &self,
        session_id: &CorrelationId,
        token: &CorrelationId,
        now: u64,
        request_id: &str,
        operation_id: &str,
    ) -> Result<AutomationCatalogue> {
        self.authorize(
            session_id,
            token,
            SecurityAction::FunctionCatalogueRead,
            now,
            request_id,
            operation_id,
        )?;
        self.load_current_automation_catalogue()
    }

    #[allow(clippy::too_many_arguments)]
    pub fn execute_function(
        &self,
        session_id: &CorrelationId,
        token: &CorrelationId,
        request: &ExecuteFunction,
        now: u64,
        request_id: &str,
        operation_id: &str,
    ) -> Result<FunctionExecutionResult> {
        request
            .validate()
            .map_err(|error| ServiceError::Contract(error.to_string()))?;
        self.authorize(
            session_id,
            token,
            SecurityAction::FunctionExecute,
            now,
            request_id,
            operation_id,
        )?;
        let catalogue = self.load_current_automation_catalogue()?;
        let definition = catalogue
            .functions
            .get(&request.function_id)
            .ok_or(ServiceError::FunctionNotFound)?;
        let executed = execute_definition(definition, &request.input)?;
        let input_bytes = serde_json::to_vec(&request.input).map_err(contract_json)?;
        let output_bytes = serde_json::to_vec(&executed.output).map_err(contract_json)?;
        let result = FunctionExecutionResult {
            contract_version: rrd_contract::FUNCTION_CONTRACT_VERSION,
            catalogue_revision: catalogue.revision,
            catalogue_sha256: catalogue.sha256(),
            function_id: request.function_id.clone(),
            runtime: definition.runtime.kind(),
            source_sha256: definition.runtime.content_sha256().into(),
            input_sha256: digest::sha256_hex(&input_bytes),
            output_sha256: digest::sha256_hex(&output_bytes),
            output: executed.output,
            attempts: 1,
            interrupts_consumed: executed.interrupts_consumed,
            fuel_consumed: executed.fuel_consumed,
        };
        result
            .validate()
            .map_err(|error| ServiceError::Contract(error.to_string()))?;
        Ok(result)
    }

    pub(in crate::engine) fn load_current_automation_catalogue(
        &self,
    ) -> Result<AutomationCatalogue> {
        let head = self
            .storage
            .control_record(&automation_head_key(&self.instance))?;
        self.load_automation_catalogue_from_head(head.as_deref())
    }

    pub(in crate::engine) fn load_automation_catalogue_revision(
        &self,
        revision: u64,
    ) -> Result<AutomationCatalogue> {
        if revision == 0 {
            return Ok(AutomationCatalogue::empty());
        }
        let bytes = self
            .storage
            .control_record(&automation_revision_key(&self.instance, revision))?
            .ok_or(ServiceError::AutomationRevisionNotFound)?;
        decode_catalogue(&bytes, Some(revision), None)
    }

    fn load_automation_catalogue_from_head(
        &self,
        head_bytes: Option<&[u8]>,
    ) -> Result<AutomationCatalogue> {
        let Some(head_bytes) = head_bytes else {
            return Ok(AutomationCatalogue::empty());
        };
        let head: AutomationHead = serde_json::from_slice(head_bytes).map_err(contract_json)?;
        if head.format_version != AUTOMATION_HEAD_FORMAT_VERSION || head.revision == 0 {
            return Err(ServiceError::Contract(
                "automation catalogue head is invalid".into(),
            ));
        }
        let bytes = self
            .storage
            .control_record(&automation_revision_key(&self.instance, head.revision))?
            .ok_or(ServiceError::AutomationRevisionNotFound)?;
        decode_catalogue(&bytes, Some(head.revision), Some(&head.catalogue_sha256))
    }

    #[allow(clippy::too_many_arguments)]
    pub(in crate::engine) fn apply_transaction_triggers(
        &self,
        catalogue: &AutomationCatalogue,
        request: &CommitTransaction,
        mut commit: RuntimeCommit,
        principal_id: Option<CanonicalId>,
        at: u64,
        request_id: &str,
        operation_id: &str,
    ) -> Result<RuntimeCommit> {
        if catalogue.triggers.is_empty() {
            return Ok(commit);
        }
        for (index, mutation) in request.mutations.iter().enumerate() {
            for trigger in catalogue
                .triggers
                .values()
                .filter(|trigger| trigger_matches(trigger, mutation))
            {
                let definition = catalogue
                    .functions
                    .get(&trigger.function_id)
                    .ok_or(ServiceError::FunctionNotFound)?;
                let input = trigger_input(index, mutation, at)?;
                let trigger_operation_id = format!("{operation_id}:trigger:{}", trigger.trigger_id);
                let input_sha256 =
                    digest::sha256_hex(&serde_json::to_vec(&input).map_err(contract_json)?);
                self.append_audit(AuditEvent {
                    at_unix_ms: at,
                    attempt: 1,
                    principal_id: principal_id.clone(),
                    action: SecurityAction::FunctionExecute,
                    resource: self.instance_resource(),
                    request_id: request_id.into(),
                    operation_id: trigger_operation_id.clone(),
                    phase: AuditPhase::Authorized,
                    decision: AuditDecision::Allowed,
                    status_code: 100,
                    request_sha256: input_sha256.clone(),
                    response_sha256: digest::sha256_hex(b"rrd-audit-completion-pending"),
                })?;
                let executed = match execute_definition(definition, &input) {
                    Ok(executed) => executed,
                    Err(error) => {
                        self.append_audit(AuditEvent {
                            at_unix_ms: at,
                            attempt: 1,
                            principal_id: principal_id.clone(),
                            action: SecurityAction::FunctionExecute,
                            resource: self.instance_resource(),
                            request_id: request_id.into(),
                            operation_id: trigger_operation_id,
                            phase: AuditPhase::Completed,
                            decision: AuditDecision::Failed,
                            status_code: 422,
                            request_sha256: input_sha256,
                            response_sha256: digest::sha256_hex(error.to_string().as_bytes()),
                        })?;
                        return Err(error);
                    }
                };
                apply_trigger_effect(trigger, executed.output.clone(), &mut commit)?;
                self.append_audit(AuditEvent {
                    at_unix_ms: at,
                    attempt: 1,
                    principal_id: principal_id.clone(),
                    action: SecurityAction::FunctionExecute,
                    resource: self.instance_resource(),
                    request_id: request_id.into(),
                    operation_id: trigger_operation_id,
                    phase: AuditPhase::Completed,
                    decision: AuditDecision::Allowed,
                    status_code: 200,
                    request_sha256: input_sha256,
                    response_sha256: digest::sha256_hex(
                        &serde_json::to_vec(&executed.output).map_err(contract_json)?,
                    ),
                })?;
            }
        }
        commit.validate().map_err(core_contract)?;
        Ok(commit)
    }

    pub(in crate::engine) fn authorize_transaction_triggers(
        &self,
        state: &SessionState,
        catalogue: &AutomationCatalogue,
        now: u64,
        request_id: &str,
        operation_id: &str,
    ) -> Result<()> {
        if catalogue.triggers.is_empty() {
            return Ok(());
        }
        match self.authorize_session_policy(state, SecurityAction::FunctionExecute, now) {
            Ok(()) => Ok(()),
            Err(error) => {
                let (decision, status_code) = super::invocation::audit_failure(&error);
                self.append_audit(AuditEvent {
                    at_unix_ms: now,
                    attempt: 1,
                    principal_id: state.principal_id.clone(),
                    action: SecurityAction::FunctionExecute,
                    resource: self.instance_resource(),
                    request_id: request_id.into(),
                    operation_id: format!("{operation_id}:trigger-authorization"),
                    phase: AuditPhase::Completed,
                    decision,
                    status_code,
                    request_sha256: digest::sha256_hex(
                        &serde_json::to_vec(&(catalogue.revision, catalogue.sha256()))
                            .map_err(contract_json)?,
                    ),
                    response_sha256: digest::sha256_hex(error.to_string().as_bytes()),
                })?;
                Err(error)
            }
        }
    }
}

pub(in crate::engine) fn execute_definition(
    definition: &FunctionDefinition,
    input: &QueryValue,
) -> Result<ExecutedFunction> {
    definition
        .validate()
        .map_err(|error| ServiceError::Contract(error.to_string()))?;
    let natural_input = query_value_to_json(input)?;
    let input_bytes = serde_json::to_vec(&natural_input).map_err(contract_json)?;
    if input_bytes.len() > definition.limits.max_input_bytes as usize {
        return Err(ServiceError::FunctionLimit(
            "function input exceeds its configured byte limit".into(),
        ));
    }
    let (output_bytes, interrupts_consumed, fuel_consumed) = match &definition.runtime {
        FunctionRuntime::JavaScriptEs2020 { source, .. } => {
            execute_javascript(source, &input_bytes, definition)?
        }
        FunctionRuntime::WebAssemblyV1 { module_base64, .. } => {
            let module = STANDARD
                .decode(module_base64)
                .map_err(|_| ServiceError::Contract("WebAssembly module is not base64".into()))?;
            execute_webassembly(&module, &input_bytes, definition)?
        }
    };
    if output_bytes.len() > definition.limits.max_output_bytes as usize {
        return Err(ServiceError::FunctionLimit(
            "function output exceeds its configured byte limit".into(),
        ));
    }
    let output: serde_json::Value = serde_json::from_slice(&output_bytes)
        .map_err(|error| ServiceError::Function(format!("invalid JSON output: {error}")))?;
    let output = json_to_query_value(output)?;
    rrd_contract::validate_function_value(&output)
        .map_err(|error| ServiceError::Contract(error.to_string()))?;
    Ok(ExecutedFunction {
        output,
        interrupts_consumed,
        fuel_consumed,
    })
}

fn execute_javascript(
    source: &str,
    input_bytes: &[u8],
    definition: &FunctionDefinition,
) -> Result<(Vec<u8>, u32, u64)> {
    let runtime = JavaScriptRuntime::new()
        .map_err(|error| ServiceError::Function(format!("QuickJS runtime: {error}")))?;
    runtime.set_memory_limit(
        definition
            .limits
            .memory_bytes
            .try_into()
            .map_err(|_| ServiceError::FunctionLimit("memory limit exceeds usize".into()))?,
    );
    runtime.set_max_stack_size(definition.limits.stack_bytes as usize);
    let interrupts = Arc::new(AtomicU32::new(0));
    let interrupt_counter = Arc::clone(&interrupts);
    let max_interrupts = definition.limits.max_interrupts;
    runtime.set_interrupt_handler(Some(Box::new(move || {
        interrupt_counter.fetch_add(1, Ordering::Relaxed) >= max_interrupts
    })));
    let context = JavaScriptContext::full(&runtime)
        .map_err(|error| ServiceError::Function(format!("QuickJS context: {error}")))?;
    let input = std::str::from_utf8(input_bytes)
        .map_err(|error| ServiceError::Function(format!("input is not UTF-8: {error}")))?;
    let encoded_input = serde_json::to_string(input).map_err(contract_json)?;
    let script = format!(
        r#""use strict";
Object.defineProperty(globalThis, "Date", {{ value: undefined, writable: false, configurable: false }});
Object.defineProperty(globalThis, "Intl", {{ value: undefined, writable: false, configurable: false }});
Object.defineProperty(globalThis, "eval", {{ value: undefined, writable: false, configurable: false }});
Object.defineProperty(globalThis, "Function", {{ value: undefined, writable: false, configurable: false }});
Object.defineProperty(Math, "random", {{ value: undefined, writable: false, configurable: false }});
const __rrd_input = JSON.parse({encoded_input});
const __rrd_function = ({source});
if (typeof __rrd_function !== "function") throw new TypeError("rrd function source must evaluate to a function");
const __rrd_output = __rrd_function(__rrd_input);
if (__rrd_output && typeof __rrd_output.then === "function") throw new TypeError("rrd-function-v1 is synchronous");
const __rrd_json = JSON.stringify(__rrd_output);
if (typeof __rrd_json !== "string") throw new TypeError("rrd function output must be JSON serializable");
__rrd_json;"#
    );
    let output = context
        .with(|context| {
            context
                .eval::<String, _>(script.as_bytes())
                .catch(&context)
                .map_err(|error| error.to_string())
        })
        .map_err(|message| {
            if interrupts.load(Ordering::Relaxed) > max_interrupts {
                ServiceError::FunctionLimit("JavaScript interrupt budget exhausted".into())
            } else if message.to_ascii_lowercase().contains("memory")
                || message.to_ascii_lowercase().contains("stack")
            {
                ServiceError::FunctionLimit(format!("JavaScript resource limit: {message}"))
            } else {
                ServiceError::Function(format!("JavaScript execution failed: {message}"))
            }
        })?;
    Ok((output.into_bytes(), interrupts.load(Ordering::Relaxed), 0))
}

fn execute_webassembly(
    module_bytes: &[u8],
    input_bytes: &[u8],
    definition: &FunctionDefinition,
) -> Result<(Vec<u8>, u32, u64)> {
    let mut config = wasmi::Config::default();
    config.consume_fuel(true);
    config.compilation_mode(wasmi::CompilationMode::Eager);
    config.set_max_stack_height(definition.limits.stack_bytes as usize);
    config.set_max_recursion_depth((definition.limits.stack_bytes as usize / 1024).max(1));
    config.set_max_cached_stacks(0);
    let engine = wasmi::Engine::new(&config);
    let module = wasmi::Module::new(&engine, module_bytes)
        .map_err(|error| ServiceError::Function(format!("WebAssembly module: {error}")))?;
    if module.imports().next().is_some() {
        return Err(ServiceError::Function(
            "WebAssembly imports are forbidden by the function-v1 capability sandbox".into(),
        ));
    }
    let limits =
        wasmi::StoreLimitsBuilder::new()
            .memory_size(
                definition.limits.memory_bytes.try_into().map_err(|_| {
                    ServiceError::FunctionLimit("memory limit exceeds usize".into())
                })?,
            )
            .instances(1)
            .memories(1)
            .tables(0)
            .trap_on_grow_failure(true)
            .build();
    let mut store = wasmi::Store::new(&engine, limits);
    store.limiter(|limits| limits);
    store
        .set_fuel(definition.limits.max_fuel)
        .map_err(|error| ServiceError::Function(format!("WebAssembly fuel: {error}")))?;
    let linker = wasmi::Linker::new(&engine);
    let instance = linker
        .instantiate_and_start(&mut store, &module)
        .map_err(|error| ServiceError::Function(format!("WebAssembly instance: {error}")))?;
    let memory = instance
        .get_memory(&store, "memory")
        .ok_or_else(|| ServiceError::Function("WebAssembly memory export is missing".into()))?;
    let allocate = instance
        .get_typed_func::<i32, i32>(&store, "rrd_alloc")
        .map_err(|error| ServiceError::Function(format!("WebAssembly rrd_alloc ABI: {error}")))?;
    let run = instance
        .get_typed_func::<(i32, i32), i64>(&store, "rrd_run")
        .map_err(|error| ServiceError::Function(format!("WebAssembly rrd_run ABI: {error}")))?;
    let input_len: i32 = input_bytes
        .len()
        .try_into()
        .map_err(|_| ServiceError::FunctionLimit("WebAssembly input exceeds i32".into()))?;
    let input_ptr = allocate
        .call(&mut store, input_len)
        .map_err(map_wasm_execution_error)?;
    let input_offset: usize = input_ptr.try_into().map_err(|_| {
        ServiceError::Function("WebAssembly returned a negative input pointer".into())
    })?;
    memory
        .write(&mut store, input_offset, input_bytes)
        .map_err(|error| ServiceError::Function(format!("WebAssembly input memory: {error}")))?;
    let packed = run
        .call(&mut store, (input_ptr, input_len))
        .map_err(map_wasm_execution_error)? as u64;
    let output_ptr = (packed >> 32) as u32 as usize;
    let output_len = (packed & u64::from(u32::MAX)) as u32 as usize;
    if output_len > definition.limits.max_output_bytes as usize {
        return Err(ServiceError::FunctionLimit(
            "WebAssembly output exceeds its configured byte limit".into(),
        ));
    }
    let mut output = vec![0_u8; output_len];
    memory
        .read(&store, output_ptr, &mut output)
        .map_err(|error| ServiceError::Function(format!("WebAssembly output memory: {error}")))?;
    let fuel_remaining = store
        .get_fuel()
        .map_err(|error| ServiceError::Function(format!("WebAssembly fuel: {error}")))?;
    Ok((
        output,
        0,
        definition.limits.max_fuel.saturating_sub(fuel_remaining),
    ))
}

fn map_wasm_execution_error(error: wasmi::Error) -> ServiceError {
    let message = error.to_string();
    if message.to_ascii_lowercase().contains("fuel")
        || message.to_ascii_lowercase().contains("resource limit")
        || message.to_ascii_lowercase().contains("grow")
        || message.to_ascii_lowercase().contains("stack")
        || message.to_ascii_lowercase().contains("recursion")
    {
        ServiceError::FunctionLimit(format!("WebAssembly resource limit: {message}"))
    } else {
        ServiceError::Function(format!("WebAssembly execution failed: {message}"))
    }
}

fn decode_catalogue(
    bytes: &[u8],
    expected_revision: Option<u64>,
    expected_sha256: Option<&str>,
) -> Result<AutomationCatalogue> {
    let catalogue: AutomationCatalogue = serde_json::from_slice(bytes).map_err(contract_json)?;
    catalogue
        .validate()
        .map_err(|error| ServiceError::Contract(error.to_string()))?;
    if expected_revision.is_some_and(|revision| catalogue.revision != revision)
        || expected_sha256.is_some_and(|sha256| catalogue.sha256() != sha256)
    {
        return Err(ServiceError::Contract(
            "automation catalogue revision or digest does not match its head".into(),
        ));
    }
    Ok(catalogue)
}

fn trigger_matches(trigger: &FunctionTrigger, mutation: &TransactionMutation) -> bool {
    let (mutation_kind, kind) = mutation_coordinates(mutation);
    trigger.mutation == mutation_kind
        && trigger
            .kind
            .as_ref()
            .is_none_or(|expected| kind.is_some_and(|actual| actual == expected))
}

fn mutation_coordinates(
    mutation: &TransactionMutation,
) -> (FunctionTriggerMutation, Option<&CanonicalId>) {
    match mutation {
        TransactionMutation::AssertClaim { .. } => (FunctionTriggerMutation::AssertClaim, None),
        TransactionMutation::PutSchema { .. } => (FunctionTriggerMutation::PutSchema, None),
        TransactionMutation::PutRecord { reference, .. } => {
            (FunctionTriggerMutation::PutRecord, Some(&reference.kind))
        }
        TransactionMutation::PutRelation { reference, .. } => {
            (FunctionTriggerMutation::PutRelation, Some(&reference.kind))
        }
        TransactionMutation::AppendEvent { kind, .. } => {
            (FunctionTriggerMutation::AppendEvent, Some(kind))
        }
        TransactionMutation::PutVector { reference, .. } => {
            (FunctionTriggerMutation::PutVector, Some(&reference.kind))
        }
        TransactionMutation::AppendSeriesSample { reference, .. } => (
            FunctionTriggerMutation::AppendSeriesSample,
            Some(&reference.kind),
        ),
        TransactionMutation::PutGeo { reference, .. } => {
            (FunctionTriggerMutation::PutGeo, Some(&reference.kind))
        }
        TransactionMutation::PublishObjectReference { reference, .. } => (
            FunctionTriggerMutation::PublishObjectReference,
            Some(&reference.kind),
        ),
        TransactionMutation::RetireData { .. } => (FunctionTriggerMutation::RetireData, None),
    }
}

fn trigger_input(index: usize, mutation: &TransactionMutation, at: u64) -> Result<QueryValue> {
    let mutation_value = serde_json::to_value(mutation).map_err(contract_json)?;
    Ok(QueryValue::Map(BTreeMap::from([
        ("at_unix_ms".into(), QueryValue::Unsigned(at)),
        (
            "mutation_index".into(),
            QueryValue::Unsigned(
                index.try_into().map_err(|_| {
                    ServiceError::Contract("trigger mutation index overflow".into())
                })?,
            ),
        ),
        ("mutation".into(), json_to_query_value(mutation_value)?),
    ])))
}

fn apply_trigger_effect(
    trigger: &FunctionTrigger,
    output: QueryValue,
    commit: &mut RuntimeCommit,
) -> Result<()> {
    match &trigger.effect {
        FunctionTriggerEffect::RequireTrue if output == QueryValue::Bool(true) => Ok(()),
        FunctionTriggerEffect::RequireTrue => Err(ServiceError::Function(format!(
            "trigger {} rejected the transaction",
            trigger.trigger_id
        ))),
        FunctionTriggerEffect::AppendEvent { kind } => {
            let QueryValue::Map(properties) = output else {
                return Err(ServiceError::Function(format!(
                    "trigger {} append_event output must be an object",
                    trigger.trigger_id
                )));
            };
            commit.mutations.push(RuntimeMutation::Event {
                event: RuntimeEvent {
                    kind: RuntimeType::new(kind.as_str()).map_err(core_contract)?,
                    subject: None,
                    properties: runtime_properties(&properties)?,
                },
            });
            Ok(())
        }
    }
}

fn query_value_to_json(value: &QueryValue) -> Result<serde_json::Value> {
    Ok(match value {
        QueryValue::Null => serde_json::Value::Null,
        QueryValue::Bool(value) => serde_json::Value::Bool(*value),
        QueryValue::Integer(value) => {
            let maximum = rrd_contract::MAX_FUNCTION_JSON_SAFE_INTEGER as i64;
            if !(-maximum..=maximum).contains(value) {
                return Err(ServiceError::Contract(
                    "function integer input exceeds the JSON-v1 exact range".into(),
                ));
            }
            serde_json::Value::Number((*value).into())
        }
        QueryValue::Unsigned(value) => {
            if *value > rrd_contract::MAX_FUNCTION_JSON_SAFE_INTEGER {
                return Err(ServiceError::Contract(
                    "function unsigned input exceeds the JSON-v1 exact range".into(),
                ));
            }
            serde_json::Value::Number((*value).into())
        }
        QueryValue::Decimal(value) => {
            let number: serde_json::Number = value.parse().map_err(|_| {
                ServiceError::Contract("function decimal input is not a JSON number".into())
            })?;
            if number.to_string() != *value {
                return Err(ServiceError::Contract(
                    "function decimal input is not canonical in the JSON-v1 numeric domain".into(),
                ));
            }
            serde_json::Value::Number(number)
        }
        QueryValue::String(value) | QueryValue::Digest(value) => {
            serde_json::Value::String(value.clone())
        }
        QueryValue::List(values) => serde_json::Value::Array(
            values
                .iter()
                .map(query_value_to_json)
                .collect::<Result<_>>()?,
        ),
        QueryValue::Map(values) => serde_json::Value::Object(
            values
                .iter()
                .map(|(key, value)| Ok((key.clone(), query_value_to_json(value)?)))
                .collect::<Result<_>>()?,
        ),
    })
}

fn json_to_query_value(value: serde_json::Value) -> Result<QueryValue> {
    Ok(match value {
        serde_json::Value::Null => QueryValue::Null,
        serde_json::Value::Bool(value) => QueryValue::Bool(value),
        serde_json::Value::Number(value) => {
            if let Some(value) = value.as_i64() {
                let maximum = rrd_contract::MAX_FUNCTION_JSON_SAFE_INTEGER as i64;
                if !(-maximum..=maximum).contains(&value) {
                    return Err(ServiceError::Contract(
                        "function integer output exceeds the JSON-v1 exact range".into(),
                    ));
                }
                QueryValue::Integer(value)
            } else if let Some(value) = value.as_u64() {
                if value > rrd_contract::MAX_FUNCTION_JSON_SAFE_INTEGER {
                    return Err(ServiceError::Contract(
                        "function unsigned output exceeds the JSON-v1 exact range".into(),
                    ));
                }
                QueryValue::Unsigned(value)
            } else {
                QueryValue::Decimal(value.to_string())
            }
        }
        serde_json::Value::String(value) => QueryValue::String(value),
        serde_json::Value::Array(values) => QueryValue::List(
            values
                .into_iter()
                .map(json_to_query_value)
                .collect::<Result<_>>()?,
        ),
        serde_json::Value::Object(values) => QueryValue::Map(
            values
                .into_iter()
                .map(|(key, value)| Ok((key, json_to_query_value(value)?)))
                .collect::<Result<_>>()?,
        ),
    })
}

fn automation_head_key(instance: &CanonicalId) -> String {
    format!("server/state/{instance}/automation/head-v1")
}

fn automation_revision_key(instance: &CanonicalId, revision: u64) -> String {
    format!("server/state/{instance}/automation/revision/{revision:020}")
}
