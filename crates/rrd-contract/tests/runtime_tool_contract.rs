use rrd_contract::{
    runtime_tool_arguments_sha256, CanonicalId, RuntimeToolAuthorization, RuntimeToolCatalogue,
    RuntimeToolDescriptor, RuntimeToolInvocation, RuntimeToolInvocationResult,
    RuntimeToolLifecyclePolicy, SecurityAction, PROTOCOL, PROTOCOL_VERSION,
    RUNTIME_TOOL_CATALOGUE_VERSION,
};
use serde_json::json;

fn descriptor(name: &str) -> RuntimeToolDescriptor {
    RuntimeToolDescriptor {
        name: CanonicalId::new(name).unwrap(),
        capability_id: None,
        description: "A governed runtime operation".into(),
        input_schema: json!({"type":"object","properties":{}}),
        mutation: false,
        authorization: RuntimeToolAuthorization::Governed,
        action: SecurityAction::MemoryRecall,
        lifecycle: RuntimeToolLifecyclePolicy::ReadOnly,
    }
}

#[test]
fn catalogue_requires_sorted_unique_validated_tools() {
    let valid = RuntimeToolCatalogue {
        protocol: PROTOCOL.into(),
        protocol_version: PROTOCOL_VERSION,
        catalogue_version: RUNTIME_TOOL_CATALOGUE_VERSION,
        tools: vec![descriptor("rrflow_context"), descriptor("rrflow_recall")],
    };
    valid.validate().unwrap();
    let encoded = serde_json::to_vec(&valid).unwrap();
    let reopened: RuntimeToolCatalogue = serde_json::from_slice(&encoded).unwrap();
    assert_eq!(reopened, valid);

    let mut unsorted = valid.clone();
    unsorted.tools.reverse();
    assert!(unsorted.validate().is_err());

    let mut duplicate = valid.clone();
    duplicate.tools[1].name = duplicate.tools[0].name.clone();
    assert!(duplicate.validate().is_err());

    let mut unsafe_public = valid;
    unsafe_public.tools[0].authorization = RuntimeToolAuthorization::Public;
    assert!(unsafe_public.validate().is_err());

    let mut ungoverned_mutation = descriptor("rrflow_mutate");
    ungoverned_mutation.mutation = true;
    ungoverned_mutation.lifecycle = RuntimeToolLifecyclePolicy::ReadOnly;
    assert!(ungoverned_mutation.validate().is_err());
}

#[test]
fn invocation_binds_exact_arguments_and_bounded_result() {
    let arguments = json!({"subject":"project:alpha","limit":16});
    let arguments_sha256 = runtime_tool_arguments_sha256(&arguments).unwrap();
    let invocation = RuntimeToolInvocation {
        catalogue_version: RUNTIME_TOOL_CATALOGUE_VERSION,
        tool: CanonicalId::new("rrflow_inspect").unwrap(),
        arguments,
        arguments_sha256: arguments_sha256.clone(),
    };
    invocation.validate().unwrap();

    let mut changed = invocation.clone();
    changed.arguments["limit"] = json!(17);
    assert!(changed.validate().is_err());

    RuntimeToolInvocationResult {
        catalogue_version: RUNTIME_TOOL_CATALOGUE_VERSION,
        tool: invocation.tool,
        arguments_sha256,
        content: String::new(),
        detail: None,
        content_sha256: "e3b0c44298fc1c149afbf4c8996fb92427ae41e4649b934ca495991b7852b855".into(),
    }
    .validate()
    .unwrap();
}

#[test]
fn unknown_fields_and_non_object_arguments_fail_closed() {
    let invalid = json!({
        "catalogue_version": RUNTIME_TOOL_CATALOGUE_VERSION,
        "tool": "rrflow_recall",
        "arguments": [],
        "arguments_sha256": "00".repeat(32),
        "surprise": true
    });
    assert!(serde_json::from_value::<RuntimeToolInvocation>(invalid).is_err());

    let arguments = json!([]);
    assert!(runtime_tool_arguments_sha256(&arguments).is_err());
}
