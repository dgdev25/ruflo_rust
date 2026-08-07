use std::fmt;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub struct AgentId(u64);

impl AgentId {
    pub(crate) fn new(raw: u64) -> Self {
        Self(raw)
    }

    pub fn value(self) -> u64 {
        self.0
    }
}

impl fmt::Display for AgentId {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "agent-{}", self.0)
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct NewAgent {
    pub name: String,
}

impl NewAgent {
    pub fn named(name: impl Into<String>) -> Self {
        Self { name: name.into() }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum AgentState {
    Spawned,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AgentHandle {
    pub id: AgentId,
    pub name: String,
    pub state: AgentState,
}
