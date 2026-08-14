//! The drift alarm, proven at a fixed clock (`PLAN.md` Step P acceptance):
//! the kernel never reads a clock, so neither do these tests — expiry is a
//! resolved instant past the interval, not a sleep.

use vyrm_node::{Registry, Verification, VERIFICATION_TTL_MS};
use vyrm_store::Store;

#[test]
fn verification_is_a_bitemporal_claim_that_expires_and_makes_noise() {
    let dir = tempfile::tempdir().unwrap();
    let store = Store::open(dir.path()).unwrap();
    let registry = Registry::builtin();
    let adapter = registry.get("codex-cli").unwrap();

    // Never audited: the state before the first claim.
    assert_eq!(
        registry.verification(&store, adapter, 1_000).unwrap(),
        Verification::Never
    );

    let claim = registry
        .record_verification(&store, "codex-cli", 1_000, "checked v3.2 against AGENTS.md docs", "operator:test")
        .unwrap();
    assert_eq!(claim.valid_to, Some(1_000 + VERIFICATION_TTL_MS));

    // In force one millisecond before expiry; the interval is half-open, so
    // the expiry instant itself is already outside it.
    assert!(matches!(
        registry.verification(&store, adapter, 1_000 + VERIFICATION_TTL_MS - 1).unwrap(),
        Verification::Current { .. }
    ));
    assert_eq!(
        registry.verification(&store, adapter, 1_000 + VERIFICATION_TTL_MS).unwrap(),
        Verification::Expired { days: 0 }
    );
    // 34 days after the audit = 13 days past the 21-day interval.
    let day = 24 * 60 * 60 * 1000;
    assert_eq!(
        registry.verification(&store, adapter, 1_000 + 34 * day).unwrap(),
        Verification::Expired { days: 13 }
    );

    // A re-audit supersedes: current again, history keeps both.
    registry
        .record_verification(&store, "codex-cli", 1_000 + 34 * day, "re-checked v3.4", "operator:test")
        .unwrap();
    assert!(matches!(
        registry.verification(&store, adapter, 1_000 + 35 * day).unwrap(),
        Verification::Current { .. }
    ));
}

#[test]
fn preflight_surfaces_the_expired_adapter_at_a_fixed_instant() {
    let dir = tempfile::tempdir().unwrap();
    let store = Store::open(dir.path()).unwrap();
    let registry = Registry::builtin();
    registry
        .record_verification(&store, "claude-code", 1_000, "hooks reference checked", "operator:test")
        .unwrap();

    let reader = vyrm_core::Reader::new("test:preflight").unwrap();
    let day = 24 * 60 * 60 * 1000;

    // Inside the interval: no noise.
    let quiet = vyrm_node::preflight(
        &store, dir.path(), Some("claude-code"), &reader, 1_000 + day, 1_500,
    )
    .unwrap();
    assert!(
        quiet.warnings.iter().all(|w| !w.contains("unverified")),
        "no drift warning inside the interval: {:?}",
        quiet.warnings
    );

    // 34 days on: the injected context itself carries the alarm.
    let noisy = vyrm_node::preflight(
        &store, dir.path(), Some("claude-code"), &reader, 1_000 + 34 * day, 1_500,
    )
    .unwrap();
    assert!(
        noisy.warnings.iter().any(|w| w.contains("unverified for 13 day(s)")),
        "expiry must make noise: {:?}",
        noisy.warnings
    );
    assert!(noisy.context.contains("unverified for 13 day(s)"));
}
