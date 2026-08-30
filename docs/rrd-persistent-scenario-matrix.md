# RRD persistent scenario matrix

Status: executable foundation gate. This matrix is not a claim of blanket
SurrealDB, Qdrant, or Fjall superiority. It proves named RRFlow/RRD persistence,
recovery, query, retrieval, and security invariants independently before the
full workspace and operating-system matrix may be called verified.

## Gate rules

- Every scenario runs as its own test process and must report at least one test
  executed and zero failures. A filtered-out test is a failure of this gate.
- The gate is 100%: no retries are hidden and no scenario may be waived.
- The full locked workspace test and strict Clippy runs happen after these
  scenarios, not instead of them.
- Linux local evidence does not satisfy macOS/ARM or Windows. Those platforms
  become green only when the pushed remote jobs complete successfully.
- A test proves only the invariant named below. Competitive performance claims
  require separate reproducible benchmark evidence.

## Independent scenarios

1. **Native backend identity and reopen**
   - Invariant: a missing root initializes as RRD LSM and reopens from its
     authenticated marker without changing backend or losing data.
   - Test: `rrd-store/persistent::missing_paths_default_to_native_and_reopen_by_authenticated_marker`

2. **Reasoning transaction crash replay**
   - Invariant: a committed RRD transaction reopens, replays idempotently, and
     never duplicates claims or journal transitions.
   - Test: `rrd-engine::engine::tests::recovery::commit_reopens_replays_and_does_not_duplicate_claims`

3. **Unified multi-model commit and evidence reopen**
   - Invariant: records, relations, events, vectors, time-series, geo, claims,
     and immutable objects share one commit; outbox and audit evidence survive
     reopen and exact retry.
   - Test: `rrd-store/unified_data::native_unified_evidence_survives_reopen_and_retry`

4. **Bitemporal correction history**
   - Invariant: a correction at the same valid time retains the superseded
     transaction-time version and supports historical reads.
   - Test: `rrd-store/bitemporal::a_correction_at_the_same_valid_from_preserves_the_claim_it_corrects`

5. **Authenticated historical snapshot after later commits**
   - Invariant: retained prefix proofs remain valid and bounded after the live
     head advances across native, compatibility, and memory engines.
   - Test: `rrd-store/snapshot::retained_prefix_proofs_survive_later_commits_on_all_engines`

6. **Persistent snapshot lease and physical retention**
   - Invariant: native snapshot leases survive flush/restart and pin the exact
     physical manifest until release or expiry.
   - Test: `rrd-store/snapshot::native_snapshot_leases_pin_physical_manifests_until_release_or_expiry`

7. **Realtime live-query semantic delta**
   - Invariant: add/update/remove deltas and resume coordinates are identical
     across native, compatibility, and memory engines.
   - Test: `rrd-query/live_query::semantic_deltas_are_identical_across_every_engine`

8. **Persistent query-index catalogue and planner selection**
   - Invariant: an index catalogue and artifact reopen, invalid definitions do
     not mutate authority, and the planner selects the persisted access path.
   - Test: `rrd-query/index_catalogue::native_catalogue_reopens_and_invalid_fields_fail_before_control_state_changes`

9. **Persistent online HNSW, quantization, BM25 hybrid, and staleness**
   - Invariant: HNSW and active scalar/product/binary/TurboQuant artifacts
     reopen from authenticated lifecycle state; ready and retired quantized
     generations remain planner-invisible; post-HNSW vector versions
     are immediately exact-overlaid, unchanged HNSW configurations append only
     their delta into consecutive immutable generations, incompatible/stale
     paths fail or explicitly fall back, and BM25/vector branches execute at one
     read stamp with hybrid results surviving reopen.
   - Test: `rrd-engine::engine::tests::vector_index::persistent_retrieval_indexes_and_hybrid_fusion_survive_reopen_and_staleness`
   - Test: `rrd-engine::engine::tests::vector_index::quantization_build_list_activate_retire_update_and_recovery_share_exact_truth`

10. **Application-complete vector backup and restore**
    - Invariant: logical state, query/vector catalogues, catalogue revisions,
      and immutable TurboQuant bytes restore into one hidden root; after the
      source root is deleted, required approximate search still selects
      `turboquant`.
    - Test: `rrd-engine::engine::tests::vector_index::application_backup_restores_turboquant_payload_before_instance_activation`

11. **Backup payload corruption fails closed**
    - Invariant: an altered retained immutable payload invalidates the
      authenticated backup catalogue and no restore target is published.
    - Test: `rrd-store/backup_catalogue::application_backup_payload_corruption_fails_before_restore_publication`

12. **Logical archive corruption and retry**
    - Invariant: archive corruption is rejected before target publication,
      staging is cleaned, and a corrected retry succeeds.
    - Test: `rrd-store/logical_archive::corruption_is_denied_before_target_publication_and_retry_succeeds`

13. **Cross-application-format logical recovery**
    - Invariant: a logical archive crosses the native application-format
      boundary and preserves exact claims in the reopened successor.
    - Test: `rrd-store/native_format_upgrade::logical_recovery_crosses_native_application_formats`

14. **Resumable migration at every durability boundary**
    - Invariant: each injected write/sync/rename interruption resumes
      idempotently to a complete native migration.
    - Test: `rrd-store/migration::every_durable_and_rename_boundary_resumes_idempotently`

15. **Persistent deny-by-default policy**
    - Invariant: grants persist across reopen, resource scope is exact, and
      ungranted actions remain denied by default.
   - Test: `rrd-security/security_authority::policy_is_persistent_exact_scope_and_deny_by_default`

16. **Unified multimodal retrieval and analytical shapes**
    - Invariant: keyword, dense, sparse, and named multimodal vectors execute at
      one read stamp through nested RRF, exact/model MaxSim, payload boost, and
      MMR stages; recommendation, discovery, context, groups, facets, and the
      directed matrix preserve identical results after database reopen.
    - Test: `rrd-engine::engine::tests::vector_index::unified_retrieval_algebra_executes_multimodal_late_interaction_and_analytics`

## Required execution order after this matrix

1. Run all 16 scenarios independently and record exact pass/fail evidence in
   `docs/implementation-journal.md`.
2. Run `cargo fmt --all -- --check`.
3. Run `cargo test --workspace --all-features --locked` with the disposable
   pgvector integration environment enabled.
4. Run `cargo clippy --workspace --all-targets --all-features --locked -- -D warnings`.
5. Run architecture, dependency, edge/offline, process, recovery, and eval
   workflow gates exactly as declared by CI.
6. Review the complete diff and update the journal with honest remaining gaps.
7. Push only the reviewed slice, then watch every required Linux, macOS/ARM,
   and Windows job to completion. Fix any failure before starting new feature
   work.
