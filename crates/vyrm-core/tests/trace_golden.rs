use serde_json::Value;
use vyrm_core::RuntimeTraceEvent;

#[test]
fn persisted_trace_wire_contract_matches_the_checked_in_vector() {
    let fixture: Value =
        serde_json::from_str(include_str!("../fixtures/runtime-trace-v1.json")).unwrap();
    let event: RuntimeTraceEvent = serde_json::from_value(fixture["event"].clone()).unwrap();
    event.validate().unwrap();
    assert_eq!(serde_json::to_value(&event).unwrap(), fixture["event"]);
    assert_eq!(
        serde_json::to_value(event.into_runtime_event().unwrap()).unwrap(),
        fixture["runtime_event"]
    );
}
