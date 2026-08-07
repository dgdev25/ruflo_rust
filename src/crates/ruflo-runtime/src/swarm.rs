use crate::{AgentId, TaskId};
use std::fmt;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub struct SwarmId(u64);

impl SwarmId {
    pub(crate) fn new(raw: u64) -> Self {
        Self(raw)
    }

    pub fn value(self) -> u64 {
        self.0
    }
}

impl fmt::Display for SwarmId {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "swarm-{}", self.0)
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct NewSwarm {
    pub name: String,
    pub agents: Vec<AgentId>,
    pub tasks: Vec<TaskId>,
}

impl NewSwarm {
    pub fn named(name: impl Into<String>) -> Self {
        Self {
            name: name.into(),
            agents: Vec::new(),
            tasks: Vec::new(),
        }
    }

    pub fn with_agents(mut self, agents: impl Into<Vec<AgentId>>) -> Self {
        self.agents = agents.into();
        self
    }

    pub fn with_tasks(mut self, tasks: impl Into<Vec<TaskId>>) -> Self {
        self.tasks = tasks.into();
        self
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SwarmState {
    Initialized,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SwarmHandle {
    pub id: SwarmId,
    pub name: String,
    pub state: SwarmState,
    pub agents: Vec<AgentId>,
    pub tasks: Vec<TaskId>,
}
