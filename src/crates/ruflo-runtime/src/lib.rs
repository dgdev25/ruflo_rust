mod agent;
mod swarm;
mod task;
mod transport;
mod workflow;

use std::collections::HashMap;
use std::sync::Mutex;

use agent::AgentHandle as StoredAgentHandle;
pub use agent::{AgentHandle, AgentId, AgentState, NewAgent};
use ruflo_types::RufloError;
use swarm::SwarmHandle as StoredSwarmHandle;
pub use swarm::{NewSwarm, SwarmHandle, SwarmId, SwarmState};
pub use task::{NewTask, TaskAuditEntry, TaskAuditKind, TaskHandle, TaskId, TaskState};
use task::{TaskHandle as StoredTaskHandle, TaskTransition};
pub use transport::{
    activate_slim, NoSlimTransportAdapter, SlimTransportAdapter, TransportOutcome,
};
pub use workflow::{WorkflowHandle, WorkflowId, WorkflowState};

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum RuntimeHandle {
    Agent(AgentHandle),
    Task(TaskHandle),
    Swarm(SwarmHandle),
    Workflow(WorkflowHandle),
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum HandleRef {
    Agent(AgentId),
    Task(TaskId),
    Swarm(SwarmId),
    Workflow(WorkflowId),
}

impl From<AgentId> for HandleRef {
    fn from(value: AgentId) -> Self {
        Self::Agent(value)
    }
}

impl From<TaskId> for HandleRef {
    fn from(value: TaskId) -> Self {
        Self::Task(value)
    }
}

impl From<SwarmId> for HandleRef {
    fn from(value: SwarmId) -> Self {
        Self::Swarm(value)
    }
}

impl From<WorkflowId> for HandleRef {
    fn from(value: WorkflowId) -> Self {
        Self::Workflow(value)
    }
}

#[derive(Debug, Default)]
pub struct Runtime {
    inner: Mutex<RuntimeState>,
}

impl Runtime {
    pub fn ephemeral() -> Self {
        Self::default()
    }

    pub fn spawn_agent(&self, agent: NewAgent) -> Result<AgentHandle, RufloError> {
        validate_name("runtime.agent.name", &agent.name)?;

        let mut state = self.inner.lock().unwrap_or_else(|e| e.into_inner());
        let id = AgentId::new(state.next_agent_id);
        state.next_agent_id += 1;

        let handle = AgentHandle {
            id,
            name: agent.name,
            state: AgentState::Spawned,
        };
        state.agents.insert(id, handle.clone());
        Ok(handle)
    }

    pub fn create_task(&self, task: NewTask) -> Result<TaskHandle, RufloError> {
        validate_name("runtime.task.name", &task.name)?;

        let mut state = self.inner.lock().unwrap_or_else(|e| e.into_inner());
        let id = TaskId::new(state.next_task_id);
        state.next_task_id += 1;

        let handle = TaskHandle {
            id,
            name: task.name,
            state: TaskState::Pending,
            audit_log: vec![TaskAuditEntry {
                sequence: 1,
                kind: TaskAuditKind::Created,
                note: "task created",
            }],
        };
        state.tasks.insert(id, handle.clone());
        Ok(handle)
    }

    pub fn init_swarm(&self, swarm: NewSwarm) -> Result<SwarmHandle, RufloError> {
        validate_name("runtime.swarm.name", &swarm.name)?;

        let mut state = self.inner.lock().unwrap_or_else(|e| e.into_inner());
        for agent_id in &swarm.agents {
            if !state.agents.contains_key(agent_id) {
                return Err(unknown_handle_error(HandleRef::Agent(*agent_id)));
            }
        }
        for task_id in &swarm.tasks {
            if !state.tasks.contains_key(task_id) {
                return Err(unknown_handle_error(HandleRef::Task(*task_id)));
            }
        }

        let id = SwarmId::new(state.next_swarm_id);
        state.next_swarm_id += 1;

        let handle = SwarmHandle {
            id,
            name: swarm.name,
            state: SwarmState::Initialized,
            agents: swarm.agents,
            tasks: swarm.tasks,
        };
        state.swarms.insert(id, handle.clone());
        Ok(handle)
    }

    pub fn cancel_task(&self, task_id: TaskId) -> Result<TaskHandle, RufloError> {
        let mut state = self.inner.lock().unwrap_or_else(|e| e.into_inner());
        let task = state
            .tasks
            .get_mut(&task_id)
            .ok_or_else(|| unknown_handle_error(HandleRef::Task(task_id)))?;

        if !task.state.can_transition(TaskTransition::Cancel) {
            return if task.state == TaskState::Cancelled {
                Err(RufloError::invalid_input(
                    "runtime.task.already_cancelled",
                    format!("task `{task_id}` is already cancelled"),
                ))
            } else {
                Err(RufloError::invalid_input(
                    "runtime.task.invalid_transition",
                    format!("task `{task_id}` cannot transition from {:?}", task.state),
                ))
            };
        }

        task.state = TaskState::Cancelled;
        let sequence = task.audit_log.len() as u64 + 1;
        task.audit_log.push(TaskAuditEntry {
            sequence,
            kind: TaskAuditKind::Cancelled,
            note: "task cancelled",
        });
        Ok(task.clone())
    }

    pub fn get_handle(&self, id: impl Into<HandleRef>) -> Result<RuntimeHandle, RufloError> {
        let state = self.inner.lock().unwrap_or_else(|e| e.into_inner());
        match id.into() {
            HandleRef::Agent(id) => state
                .agents
                .get(&id)
                .cloned()
                .map(RuntimeHandle::Agent)
                .ok_or_else(|| unknown_handle_error(HandleRef::Agent(id))),
            HandleRef::Task(id) => state
                .tasks
                .get(&id)
                .cloned()
                .map(RuntimeHandle::Task)
                .ok_or_else(|| unknown_handle_error(HandleRef::Task(id))),
            HandleRef::Swarm(id) => state
                .swarms
                .get(&id)
                .cloned()
                .map(RuntimeHandle::Swarm)
                .ok_or_else(|| unknown_handle_error(HandleRef::Swarm(id))),
            HandleRef::Workflow(id) => state
                .workflows
                .get(&id)
                .cloned()
                .map(RuntimeHandle::Workflow)
                .ok_or_else(|| unknown_handle_error(HandleRef::Workflow(id))),
        }
    }
}

#[derive(Debug)]
struct RuntimeState {
    next_agent_id: u64,
    next_task_id: u64,
    next_swarm_id: u64,
    agents: HashMap<AgentId, StoredAgentHandle>,
    tasks: HashMap<TaskId, StoredTaskHandle>,
    swarms: HashMap<SwarmId, StoredSwarmHandle>,
    workflows: HashMap<WorkflowId, WorkflowHandle>,
}

impl Default for RuntimeState {
    fn default() -> Self {
        Self {
            next_agent_id: 1,
            next_task_id: 1,
            next_swarm_id: 1,
            agents: HashMap::new(),
            tasks: HashMap::new(),
            swarms: HashMap::new(),
            workflows: HashMap::new(),
        }
    }
}

fn validate_name(code: &'static str, value: &str) -> Result<(), RufloError> {
    if value.trim().is_empty() {
        return Err(RufloError::invalid_input(code, "name must not be empty"));
    }
    Ok(())
}

fn unknown_handle_error(handle: HandleRef) -> RufloError {
    let (kind, id) = match handle {
        HandleRef::Agent(id) => ("agent", id.value()),
        HandleRef::Task(id) => ("task", id.value()),
        HandleRef::Swarm(id) => ("swarm", id.value()),
        HandleRef::Workflow(id) => ("workflow", id.value()),
    };
    RufloError::invalid_input(
        "runtime.handle.unknown",
        format!("unknown {kind} handle `{kind}-{id}`"),
    )
}
