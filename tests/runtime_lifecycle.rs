use ruflo_runtime::{
    AgentState, HandleRef, NewAgent, NewSwarm, NewTask, Runtime, RuntimeHandle, TaskAuditKind,
    TaskState,
};
use ruflo_types::RufloError;

#[test]
fn cancelled_task_is_terminal_and_retains_auditable_handle() {
    let runtime = Runtime::ephemeral();
    let agent = runtime
        .spawn_agent(NewAgent::named("fixture-agent"))
        .unwrap();
    let task = runtime.create_task(NewTask::named("fixture-task")).unwrap();
    let swarm = runtime
        .init_swarm(
            NewSwarm::named("fixture-swarm")
                .with_agents(vec![agent.id])
                .with_tasks(vec![task.id]),
        )
        .unwrap();

    let cancelled = runtime.cancel_task(task.id).unwrap();
    assert_eq!(cancelled.state, TaskState::Cancelled);
    assert_eq!(cancelled.audit_log.len(), 2);
    assert_eq!(cancelled.audit_log[0].kind, TaskAuditKind::Created);
    assert_eq!(cancelled.audit_log[1].kind, TaskAuditKind::Cancelled);

    match runtime.get_handle(task.id).unwrap() {
        RuntimeHandle::Task(handle) => {
            assert_eq!(handle.state, TaskState::Cancelled);
            assert_eq!(handle.audit_log, cancelled.audit_log);
        }
        other => panic!("expected task handle, got {other:?}"),
    }

    match runtime.get_handle(agent.id).unwrap() {
        RuntimeHandle::Agent(handle) => assert_eq!(handle.name, "fixture-agent"),
        other => panic!("expected agent handle, got {other:?}"),
    }

    match runtime.get_handle(swarm.id).unwrap() {
        RuntimeHandle::Swarm(handle) => {
            assert_eq!(handle.agents, vec![agent.id]);
            assert_eq!(handle.tasks, vec![task.id]);
        }
        other => panic!("expected swarm handle, got {other:?}"),
    }
}

#[test]
fn duplicate_cancellation_is_rejected_without_mutating_state() {
    let runtime = Runtime::ephemeral();
    let task = runtime.create_task(NewTask::named("fixture-task")).unwrap();

    runtime.cancel_task(task.id).unwrap();
    let error = runtime.cancel_task(task.id).unwrap_err();
    assert!(matches!(
        error,
        RufloError::InvalidInput { code, .. } if code == "runtime.task.already_cancelled"
    ));

    match runtime.get_handle(task.id).unwrap() {
        RuntimeHandle::Task(handle) => {
            assert_eq!(handle.state, TaskState::Cancelled);
            assert_eq!(handle.audit_log.len(), 2);
        }
        other => panic!("expected task handle, got {other:?}"),
    }
}

#[test]
fn unknown_handles_and_invalid_swarm_references_are_rejected_stably() {
    let runtime = Runtime::ephemeral();
    let foreign_runtime = Runtime::ephemeral();
    let agent = runtime
        .spawn_agent(NewAgent::named("fixture-agent"))
        .unwrap();
    let foreign_task = foreign_runtime
        .create_task(NewTask::named("foreign-task"))
        .unwrap();
    let missing_task_error = runtime.cancel_task(foreign_task.id).unwrap_err();
    assert!(matches!(
        missing_task_error,
        RufloError::InvalidInput { code, .. } if code == "runtime.handle.unknown"
    ));

    let missing_agent_error = runtime
        .init_swarm(
            NewSwarm::named("fixture-swarm")
                .with_agents(vec![agent.id])
                .with_tasks(vec![foreign_task.id]),
        )
        .unwrap_err();
    assert!(matches!(
        missing_agent_error,
        RufloError::InvalidInput { code, .. } if code == "runtime.handle.unknown"
    ));

    match runtime.get_handle(HandleRef::Agent(agent.id)).unwrap() {
        RuntimeHandle::Agent(handle) => assert_eq!(handle.state, AgentState::Spawned),
        other => panic!("expected agent handle, got {other:?}"),
    }
}
