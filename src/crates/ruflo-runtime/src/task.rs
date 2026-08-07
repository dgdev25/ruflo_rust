use std::fmt;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub struct TaskId(u64);

impl TaskId {
    pub(crate) fn new(raw: u64) -> Self {
        Self(raw)
    }

    pub fn value(self) -> u64 {
        self.0
    }
}

impl fmt::Display for TaskId {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "task-{}", self.0)
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct NewTask {
    pub name: String,
}

impl NewTask {
    pub fn named(name: impl Into<String>) -> Self {
        Self { name: name.into() }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TaskState {
    Pending,
    Cancelled,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TaskAuditKind {
    Created,
    Cancelled,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TaskAuditEntry {
    pub sequence: u64,
    pub kind: TaskAuditKind,
    pub note: &'static str,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TaskHandle {
    pub id: TaskId,
    pub name: String,
    pub state: TaskState,
    pub audit_log: Vec<TaskAuditEntry>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum TaskTransition {
    Cancel,
}

impl TaskState {
    pub(crate) fn can_transition(self, transition: TaskTransition) -> bool {
        matches!((self, transition), (Self::Pending, TaskTransition::Cancel))
    }
}
