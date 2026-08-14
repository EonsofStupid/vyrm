//! The port carries the runtime (`PLAN.md` Step S): preflight and prompt
//! recall run unchanged over the reference engine, proving vyrm-node
//! consumes the storage port, not Fjall.

use vyrm_core::{Claim, Predicate, Producer, Subject};
use vyrm_store::{Engine, MemoryEngine};

fn claim(subject: &str, predicate: &str, object: &str, from: u64) -> Claim {
    Claim::new(
        Subject::new(subject).unwrap(),
        Predicate::new(predicate).unwrap(),
        object,
        from,
        from,
        Producer { actor: "test".into(), on_behalf_of: None, session: None },
    )
}

#[test]
fn the_runtime_layer_is_generic_over_the_port() {
    // The preflight and prompt-recall paths run unchanged over the reference
    // engine — the proof that vyrm-node consumes the port, not Fjall.
    let engine = MemoryEngine::new();
    Engine::assert(&engine, &claim("deploy", "status", "blocked-on-migration", 1_000)).unwrap();

    let reader = vyrm_core::Reader::new("test:port").unwrap();
    let dir = tempfile::tempdir().unwrap();
    let flight =
        vyrm_node::preflight(&engine, dir.path(), None, &reader, 2_000, 1_500).unwrap();
    assert!(flight.context.contains("blocked-on-migration"));
    assert_eq!(engine.observe_count(), 1, "recall telemetry flowed through the port");

    let ctx = vyrm_node::HookContext {
        store: &engine,
        root: dir.path(),
        harness: None,
        reader: &reader,
        now: 2_000,
        budget: 1_500,
    };
    let response = vyrm_node::handle(
        &ctx,
        vyrm_node::HookEvent::UserPromptSubmit,
        &serde_json::json!({"prompt": "what is blocking deploy?"}),
    )
    .unwrap();
    assert!(response.stdout.contains("blocked-on-migration"));
}
