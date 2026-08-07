use std::fmt;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub struct WorkflowId(u64);

impl WorkflowId {
    pub fn value(self) -> u64 {
        self.0
    }
}

impl fmt::Display for WorkflowId {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "workflow-{}", self.0)
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum WorkflowState {
    Defined,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct WorkflowHandle {
    pub id: WorkflowId,
    pub name: String,
    pub state: WorkflowState,
}
