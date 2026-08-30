use rrd_contract::{RuntimeToolAuthorization, RuntimeToolLifecyclePolicy, SecurityAction};
use rrd_engine::WorkPlanOperation;

#[test]
fn executable_catalogue_is_the_valid_public_catalogue_with_exact_actions() {
    let executable = rrd_engine::runtime_tool_catalogue();
    let public = rrd_engine::runtime_tool_contract_catalogue();
    public.validate().unwrap();
    assert_eq!(public.tools.len(), 30 + WorkPlanOperation::ALL.len());
    assert_eq!(public.tools.len(), executable.len());

    let expected = [
        ("rrflow_audit_read", SecurityAction::AuditRead),
        ("rrflow_backup_create", SecurityAction::BackupCreate),
        ("rrflow_backup_list", SecurityAction::BackupList),
        ("rrflow_changefeed_follow", SecurityAction::ChangefeedFollow),
        ("rrflow_changefeed_read", SecurityAction::ChangefeedRead),
        ("rrflow_context", SecurityAction::MemoryContextRead),
        ("rrflow_data_commit", SecurityAction::TransactionCommit),
        ("rrflow_data_rollback", SecurityAction::TransactionCommit),
        ("rrflow_estate_read", SecurityAction::EstateRead),
        ("rrflow_forget", SecurityAction::MemoryRetire),
        ("rrflow_hook", SecurityAction::LifecycleApply),
        ("rrflow_inspect", SecurityAction::MemoryInspect),
        ("rrflow_lifecycle", SecurityAction::LifecycleApply),
        ("rrflow_live_query_poll", SecurityAction::QueryLivePoll),
        ("rrflow_preflight", SecurityAction::ProjectAttune),
        ("rrflow_query", SecurityAction::QueryExecute),
        (
            "rrflow_query_index_ensure",
            SecurityAction::QueryIndexEnsure,
        ),
        ("rrflow_query_index_list", SecurityAction::QueryIndexList),
        ("rrflow_reasoning_record", SecurityAction::ReasoningWrite),
        ("rrflow_reasoning_show", SecurityAction::ReasoningRead),
        ("rrflow_recall", SecurityAction::MemoryRecall),
        ("rrflow_remember", SecurityAction::MemoryWrite),
        ("rrflow_restore", SecurityAction::RestoreCreate),
        ("rrflow_route", SecurityAction::ProjectRoute),
        ("rrflow_service_status", SecurityAction::ServiceInspect),
        (
            "rrflow_vector_collection_ensure",
            SecurityAction::VectorCollectionEnsure,
        ),
        (
            "rrflow_vector_collection_list",
            SecurityAction::VectorCollectionList,
        ),
        (
            "rrflow_vector_points_retrieve",
            SecurityAction::VectorPointRetrieve,
        ),
        (
            "rrflow_vector_points_scroll",
            SecurityAction::VectorPointScroll,
        ),
        ("rrflow_vector_search", SecurityAction::VectorSearch),
    ];

    for ((definition, exposed), (name, action)) in
        executable.iter().zip(&public.tools).zip(expected)
    {
        assert_eq!(definition.name, name);
        assert_eq!(definition.action, action);
        assert_eq!(exposed.name.as_str(), name);
        assert_eq!(exposed.action, action);
        assert_eq!(exposed.mutation, definition.mutation);
        assert_eq!(exposed.authorization, definition.authorization);
        assert_eq!(exposed.lifecycle, definition.lifecycle);
        if definition.lifecycle == RuntimeToolLifecyclePolicy::PlannedMutation {
            let coordinates = definition.input_schema["properties"]["rrflow_lifecycle"]
                .as_object()
                .expect("exact mutation publishes canonical lifecycle coordinates");
            assert_eq!(coordinates["type"], "object");
        }
    }

    let status = public
        .tools
        .iter()
        .find(|tool| tool.name.as_str() == "rrflow_service_status")
        .unwrap();
    assert_eq!(status.authorization, RuntimeToolAuthorization::Public);
    assert_eq!(
        public
            .tools
            .iter()
            .filter(|tool| tool.authorization == RuntimeToolAuthorization::Public)
            .count(),
        1
    );

    let planned_mutations: Vec<_> = public
        .tools
        .iter()
        .filter(|tool| tool.lifecycle == RuntimeToolLifecyclePolicy::PlannedMutation)
        .map(|tool| tool.name.as_str())
        .collect();
    assert_eq!(
        planned_mutations,
        [
            "rrflow_backup_create",
            "rrflow_data_commit",
            "rrflow_data_rollback",
            "rrflow_query_index_ensure",
            "rrflow_restore",
            "rrflow_vector_collection_ensure",
        ]
    );
    assert!(public.tools.iter().all(|tool| {
        (tool.lifecycle == RuntimeToolLifecyclePolicy::ReadOnly) == !tool.mutation
    }));

    for operation in WorkPlanOperation::ALL {
        let definition = public
            .tools
            .iter()
            .find(|tool| tool.name.as_str() == operation.runtime_tool_name())
            .unwrap();
        assert_eq!(
            definition.capability_id.as_ref().unwrap().as_str(),
            operation.capability_id()
        );
        assert_eq!(
            definition.action,
            match operation {
                WorkPlanOperation::Status => SecurityAction::WorkPlanRead,
                WorkPlanOperation::Verify => SecurityAction::WorkPlanVerifyExecute,
                _ => SecurityAction::WorkPlanControl,
            }
        );
        assert_eq!(
            definition.lifecycle,
            match operation {
                WorkPlanOperation::Status => RuntimeToolLifecyclePolicy::ReadOnly,
                WorkPlanOperation::Verify => RuntimeToolLifecyclePolicy::VerificationExecution,
                _ => RuntimeToolLifecyclePolicy::ControlTransition,
            }
        );
    }
}
