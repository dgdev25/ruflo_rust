use std::fs;
use std::io;
use std::io::Write;
use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};

const CONFIG: &str = "# Native Ruflo project configuration\nversion: 3\n";

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ProjectStatus {
    pub agents: usize,
    pub tasks: usize,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SwarmRecord {
    pub id: String,
    pub topology: String,
    pub max_agents: usize,
    pub strategy: String,
    pub status: String,
    #[serde(default)]
    pub objective: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SwarmStatus {
    pub swarm: Option<SwarmRecord>,
    pub agents_total: usize,
    pub agents_active: usize,
    pub tasks_total: usize,
    pub tasks_completed: usize,
    pub tasks_running: usize,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SessionRecord {
    pub session_id: String,
    pub name: String,
    #[serde(default)]
    pub description: String,
    pub status: String,
    pub saved_at_ms: u128,
    pub agents: Vec<AgentRecord>,
    pub tasks: Vec<TaskRecord>,
    pub swarm: Option<SwarmRecord>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct AgentRecord {
    pub id: String,
    pub agent_type: String,
    pub status: String,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AgentMetrics {
    pub period: String,
    pub total_agents: usize,
    pub active_agents: usize,
    pub idle_agents: usize,
    pub terminated_agents: usize,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct TaskRecord {
    pub id: String,
    #[serde(rename = "type")]
    pub task_type: String,
    pub description: String,
    #[serde(default = "normal_priority")]
    pub priority: String,
    pub status: String,
    #[serde(default)]
    pub assigned_agent_ids: Vec<String>,
    #[serde(default)]
    pub dependencies: Vec<String>,
    #[serde(default)]
    pub tags: Vec<String>,
    #[serde(default)]
    pub retry_count: u32,
    #[serde(default = "default_max_retries")]
    pub max_retries: u32,
    #[serde(default = "default_timeout_ms")]
    pub timeout_ms: u64,
    #[serde(default)]
    pub cancellation_reason: Option<String>,
}

pub fn create_task(
    project_root: &Path,
    task_type: &str,
    description: &str,
    priority: &str,
) -> io::Result<TaskRecord> {
    status(project_root)?;
    if description.trim().is_empty() {
        return Err(io::Error::new(
            io::ErrorKind::InvalidInput,
            "task description must not be empty",
        ));
    }
    let priority = valid_priority(priority)?;
    let base = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .expect("clock")
        .as_millis();
    for suffix in 0..1000_u32 {
        let id = if suffix == 0 {
            format!("task-{base}")
        } else {
            format!("task-{base}-{suffix}")
        };
        let record = TaskRecord {
            id: id.clone(),
            task_type: safe_identifier(task_type)?,
            description: description.into(),
            priority: priority.clone(),
            status: "pending".into(),
            assigned_agent_ids: Vec::new(),
            dependencies: Vec::new(),
            tags: Vec::new(),
            retry_count: 0,
            max_retries: default_max_retries(),
            timeout_ms: default_timeout_ms(),
            cancellation_reason: None,
        };
        let path = project_root.join(".swarm/tasks").join(format!("{id}.json"));
        match write_new_json(&path, &record) {
            Ok(()) => return Ok(record),
            Err(error) if error.kind() == io::ErrorKind::AlreadyExists => continue,
            Err(error) => return Err(error),
        }
    }
    Err(io::Error::new(
        io::ErrorKind::AlreadyExists,
        "unable to allocate a unique task ID",
    ))
}

pub fn save_session(
    project_root: &Path,
    name: &str,
    description: &str,
) -> io::Result<SessionRecord> {
    status(project_root)?;
    if name.trim().is_empty() {
        return Err(io::Error::new(
            io::ErrorKind::InvalidInput,
            "session name must not be empty",
        ));
    }
    let record = SessionRecord {
        session_id: next_session_id(project_root)?,
        name: name.into(),
        description: description.into(),
        status: "saved".into(),
        saved_at_ms: unique_millis(),
        agents: list_agents(project_root)?,
        tasks: list_tasks(project_root)?,
        swarm: read_swarm(project_root)?,
    };
    write_session(project_root, &record)?;
    fs::write(
        project_root.join(".claude-flow/sessions/current.json"),
        serde_json::to_vec_pretty(&record).expect("session serializable"),
    )?;
    Ok(record)
}

pub fn list_sessions(project_root: &Path) -> io::Result<Vec<SessionRecord>> {
    status(project_root)?;
    let mut sessions: Vec<SessionRecord> =
        fs::read_dir(project_root.join(".claude-flow/sessions"))?
            .filter_map(Result::ok)
            .filter(|entry| entry.file_name() != "current.json")
            .filter(|entry| {
                entry
                    .path()
                    .extension()
                    .is_some_and(|extension| extension == "json")
            })
            .map(|entry| serde_json::from_slice(&fs::read(entry.path())?).map_err(io::Error::other))
            .collect::<Result<_, _>>()?;
    sessions.sort_by(|left, right| right.saved_at_ms.cmp(&left.saved_at_ms));
    Ok(sessions)
}

pub fn current_session(project_root: &Path) -> io::Result<SessionRecord> {
    status(project_root)?;
    serde_json::from_slice(&fs::read(
        project_root.join(".claude-flow/sessions/current.json"),
    )?)
    .map_err(io::Error::other)
}

pub fn restore_session(project_root: &Path, session_id: &str) -> io::Result<SessionRecord> {
    status(project_root)?;
    let record = read_session(project_root, session_id)?;
    replace_records(
        project_root.join(".swarm/agents"),
        &record.agents,
        |record| &record.id,
    )?;
    replace_records(project_root.join(".swarm/tasks"), &record.tasks, |record| {
        &record.id
    })?;
    if let Some(swarm) = &record.swarm {
        write_swarm(project_root, swarm)?;
    }
    fs::write(
        project_root.join(".claude-flow/sessions/current.json"),
        serde_json::to_vec_pretty(&record).expect("session serializable"),
    )?;
    Ok(record)
}

pub fn delete_session(project_root: &Path, session_id: &str) -> io::Result<()> {
    status(project_root)?;
    let session_id = safe_identifier(session_id)?;
    let path = project_root
        .join(".claude-flow/sessions")
        .join(format!("{session_id}.json"));
    fs::remove_file(path)
}

pub fn export_session(
    project_root: &Path,
    session_id: &str,
    output: &Path,
) -> io::Result<SessionRecord> {
    let session = read_session(project_root, session_id)?;
    let output = safe_project_path(project_root, output)?;
    fs::write(
        output,
        serde_json::to_vec_pretty(&session).expect("session serializable"),
    )?;
    Ok(session)
}

pub fn import_session(
    project_root: &Path,
    input: &Path,
    name: Option<&str>,
) -> io::Result<SessionRecord> {
    status(project_root)?;
    let input = safe_project_path(project_root, input)?;
    let mut session: SessionRecord =
        serde_json::from_slice(&fs::read(input)?).map_err(io::Error::other)?;
    session.session_id = next_session_id(project_root)?;
    session.saved_at_ms = unique_millis();
    session.status = "saved".into();
    if let Some(name) = name {
        if name.trim().is_empty() {
            return Err(io::Error::new(
                io::ErrorKind::InvalidInput,
                "session name must not be empty",
            ));
        }
        session.name = name.into();
    }
    write_session(project_root, &session)?;
    Ok(session)
}

pub fn initialize_swarm(
    project_root: &Path,
    topology: &str,
    max_agents: usize,
    strategy: &str,
) -> io::Result<SwarmRecord> {
    status(project_root)?;
    if max_agents == 0 || max_agents > 100 {
        return Err(io::Error::new(
            io::ErrorKind::InvalidInput,
            "max agents must be between 1 and 100",
        ));
    }
    let record = SwarmRecord {
        id: format!("swarm-{}", unique_millis()),
        topology: valid_topology(topology)?,
        max_agents,
        strategy: valid_strategy(strategy)?,
        status: "ready".into(),
        objective: None,
    };
    write_swarm(project_root, &record)?;
    Ok(record)
}

pub fn swarm_status(project_root: &Path) -> io::Result<SwarmStatus> {
    status(project_root)?;
    let swarm = read_swarm(project_root)?;
    let agents = list_agents(project_root)?;
    let tasks = list_tasks(project_root)?;
    let agents_active = agents
        .iter()
        .filter(|agent| matches!(agent.status.as_str(), "active" | "running" | "busy"))
        .count();
    let tasks_completed = tasks
        .iter()
        .filter(|task| matches!(task.status.as_str(), "completed" | "done"))
        .count();
    let tasks_running = tasks
        .iter()
        .filter(|task| matches!(task.status.as_str(), "running" | "in_progress"))
        .count();
    Ok(SwarmStatus {
        swarm,
        agents_total: agents.len(),
        agents_active,
        tasks_total: tasks.len(),
        tasks_completed,
        tasks_running,
    })
}

pub fn start_swarm(
    project_root: &Path,
    objective: &str,
    strategy: &str,
) -> io::Result<SwarmRecord> {
    if objective.trim().is_empty() {
        return Err(io::Error::new(
            io::ErrorKind::InvalidInput,
            "swarm objective must not be empty",
        ));
    }
    let mut swarm = read_swarm(project_root)?.ok_or_else(|| {
        io::Error::new(
            io::ErrorKind::NotFound,
            "no swarm is initialized in this directory",
        )
    })?;
    if swarm.status == "stopped" {
        return Err(io::Error::new(
            io::ErrorKind::InvalidInput,
            "stopped swarm cannot be started; initialize a new swarm",
        ));
    }
    swarm.strategy = valid_strategy(strategy)?;
    swarm.objective = Some(objective.into());
    swarm.status = "running".into();
    write_swarm(project_root, &swarm)?;
    Ok(swarm)
}

pub fn finish_swarm(project_root: &Path, succeeded: bool) -> io::Result<SwarmRecord> {
    let mut swarm = read_swarm(project_root)?.ok_or_else(|| {
        io::Error::new(
            io::ErrorKind::NotFound,
            "no swarm is initialized in this directory",
        )
    })?;
    swarm.status = if succeeded { "completed" } else { "failed" }.into();
    write_swarm(project_root, &swarm)?;
    Ok(swarm)
}

pub fn stop_swarm(project_root: &Path, swarm_id: &str) -> io::Result<SwarmRecord> {
    let mut swarm = read_swarm(project_root)?.ok_or_else(|| {
        io::Error::new(
            io::ErrorKind::NotFound,
            "no swarm is initialized in this directory",
        )
    })?;
    if swarm.id != swarm_id {
        return Err(io::Error::new(
            io::ErrorKind::NotFound,
            format!("swarm `{swarm_id}` does not exist"),
        ));
    }
    swarm.status = "stopped".into();
    write_swarm(project_root, &swarm)?;
    Ok(swarm)
}

pub fn list_tasks(project_root: &Path) -> io::Result<Vec<TaskRecord>> {
    status(project_root)?;
    let mut tasks: Vec<TaskRecord> = fs::read_dir(project_root.join(".swarm/tasks"))?
        .filter_map(Result::ok)
        .filter(|entry| entry.path().extension().is_some_and(|ext| ext == "json"))
        .map(|entry| serde_json::from_slice(&fs::read(entry.path())?).map_err(io::Error::other))
        .collect::<Result<_, _>>()?;
    tasks.sort_by(|a, b| a.id.cmp(&b.id));
    Ok(tasks)
}

pub fn get_task(project_root: &Path, task_id: &str) -> io::Result<TaskRecord> {
    status(project_root)?;
    read_task(project_root, task_id)
}

pub fn cancel_task(project_root: &Path, task_id: &str, reason: &str) -> io::Result<TaskRecord> {
    let mut task = read_task(project_root, task_id)?;
    if matches!(task.status.as_str(), "completed" | "failed") {
        return Err(io::Error::new(
            io::ErrorKind::InvalidInput,
            "cannot cancel finished tasks",
        ));
    }
    task.status = "cancelled".into();
    task.cancellation_reason = Some(reason.into());
    write_task(project_root, &task)?;
    Ok(task)
}

pub fn assign_task(
    project_root: &Path,
    task_id: &str,
    agent_ids: &[String],
    unassign: bool,
) -> io::Result<TaskRecord> {
    let mut task = read_task(project_root, task_id)?;
    if !matches!(task.status.as_str(), "pending" | "queued") {
        return Err(io::Error::new(
            io::ErrorKind::InvalidInput,
            "can only assign queued or pending tasks",
        ));
    }
    if unassign {
        task.assigned_agent_ids.clear();
        task.status = "pending".into();
    } else {
        for agent_id in agent_ids {
            let agent_id = safe_identifier(agent_id)?;
            if !project_root
                .join(".swarm/agents")
                .join(format!("{agent_id}.json"))
                .is_file()
            {
                return Err(io::Error::new(
                    io::ErrorKind::NotFound,
                    format!("agent `{agent_id}` does not exist"),
                ));
            }
        }
        task.assigned_agent_ids = agent_ids.to_vec();
        task.status = "assigned".into();
    }
    write_task(project_root, &task)?;
    Ok(task)
}

pub fn retry_task(project_root: &Path, task_id: &str, reset_state: bool) -> io::Result<TaskRecord> {
    let mut task = read_task(project_root, task_id)?;
    if task.status != "failed" {
        return Err(io::Error::new(
            io::ErrorKind::InvalidInput,
            "can only retry failed tasks",
        ));
    }
    if !reset_state && task.retry_count >= task.max_retries {
        return Err(io::Error::new(
            io::ErrorKind::InvalidInput,
            "task retry limit reached",
        ));
    }
    if reset_state {
        task.retry_count = 0;
    } else {
        task.retry_count += 1;
    }
    task.status = "queued".into();
    task.assigned_agent_ids.clear();
    task.cancellation_reason = None;
    write_task(project_root, &task)?;
    Ok(task)
}

pub fn spawn_agent(project_root: &Path, agent_type: &str, name: &str) -> io::Result<AgentRecord> {
    status(project_root)?;
    let id = safe_identifier(name)?;
    let record = AgentRecord {
        id: id.clone(),
        agent_type: safe_identifier(agent_type)?,
        status: "idle".into(),
    };
    let path = project_root
        .join(".swarm/agents")
        .join(format!("{id}.json"));
    if path.exists() {
        return Err(io::Error::new(
            io::ErrorKind::AlreadyExists,
            format!("agent `{id}` already exists"),
        ));
    }
    fs::write(
        path,
        serde_json::to_vec_pretty(&record).expect("agent record is serializable"),
    )?;
    Ok(record)
}

pub fn list_agents(project_root: &Path) -> io::Result<Vec<AgentRecord>> {
    status(project_root)?;
    let mut agents: Vec<AgentRecord> = Vec::new();
    for entry in fs::read_dir(project_root.join(".swarm/agents"))? {
        let entry = entry?;
        if entry.path().extension().is_some_and(|ext| ext == "json") {
            agents
                .push(serde_json::from_slice(&fs::read(entry.path())?).map_err(io::Error::other)?);
        }
    }
    agents.sort_by(|left, right| left.id.cmp(&right.id));
    Ok(agents)
}

pub fn get_agent(project_root: &Path, agent_id: &str) -> io::Result<AgentRecord> {
    status(project_root)?;
    let agent_id = safe_identifier(agent_id)?;
    let path = project_root
        .join(".swarm/agents")
        .join(format!("{agent_id}.json"));
    serde_json::from_slice(&fs::read(path)?).map_err(io::Error::other)
}

pub fn stop_agent(project_root: &Path, agent_id: &str) -> io::Result<AgentRecord> {
    let mut agent = get_agent(project_root, agent_id)?;
    agent.status = "terminated".into();
    fs::write(
        project_root
            .join(".swarm/agents")
            .join(format!("{}.json", agent.id)),
        serde_json::to_vec_pretty(&agent).expect("agent record is serializable"),
    )?;
    Ok(agent)
}

pub fn agent_metrics(project_root: &Path, period: &str) -> io::Result<AgentMetrics> {
    match period {
        "1h" | "24h" | "7d" | "30d" => {}
        _ => {
            return Err(io::Error::new(
                io::ErrorKind::InvalidInput,
                "agent metrics period must be one of 1h, 24h, 7d, or 30d",
            ));
        }
    }
    let agents = list_agents(project_root)?;
    Ok(AgentMetrics {
        period: period.into(),
        total_agents: agents.len(),
        active_agents: agents
            .iter()
            .filter(|agent| matches!(agent.status.as_str(), "active" | "busy"))
            .count(),
        idle_agents: agents.iter().filter(|agent| agent.status == "idle").count(),
        terminated_agents: agents
            .iter()
            .filter(|agent| agent.status == "terminated")
            .count(),
    })
}

pub fn initialize(project_root: &Path) -> io::Result<()> {
    for relative in [
        ".claude-flow/data",
        ".claude-flow/logs",
        ".claude-flow/sessions",
        ".claude-flow/agents",
        ".claude-flow/workflows",
        ".swarm/agents",
        ".swarm/tasks",
        ".swarm/memory",
        ".agents",
    ] {
        fs::create_dir_all(project_root.join(relative))?;
    }

    write_if_absent(
        &project_root.join(".claude-flow/config.yaml"),
        CONFIG.as_bytes(),
    )?;
    write_if_absent(
        &project_root.join(".agents/config.toml"),
        b"[swarm.automation]\nenabled = false\n",
    )?;
    Ok(())
}

pub fn status(project_root: &Path) -> io::Result<ProjectStatus> {
    if !project_root.join(".claude-flow/config.yaml").is_file() {
        return Err(io::Error::new(
            io::ErrorKind::NotFound,
            "RuFlo is not initialized in this directory",
        ));
    }
    Ok(ProjectStatus {
        agents: count_json(project_root.join(".swarm/agents"))?,
        tasks: count_json(project_root.join(".swarm/tasks"))?,
    })
}

fn write_if_absent(path: &Path, contents: &[u8]) -> io::Result<()> {
    if path.exists() {
        return Ok(());
    }
    fs::write(path, contents)
}

fn read_session(project_root: &Path, session_id: &str) -> io::Result<SessionRecord> {
    let session_id = safe_identifier(session_id)?;
    serde_json::from_slice(&fs::read(
        project_root
            .join(".claude-flow/sessions")
            .join(format!("{session_id}.json")),
    )?)
    .map_err(io::Error::other)
}

fn write_session(project_root: &Path, session: &SessionRecord) -> io::Result<()> {
    let session_id = safe_identifier(&session.session_id)?;
    fs::write(
        project_root
            .join(".claude-flow/sessions")
            .join(format!("{session_id}.json")),
        serde_json::to_vec_pretty(session).expect("session serializable"),
    )
}

fn replace_records<T, F>(directory: PathBuf, records: &[T], id: F) -> io::Result<()>
where
    T: Serialize,
    F: Fn(&T) -> &String,
{
    for entry in fs::read_dir(&directory)? {
        let entry = entry?;
        if entry
            .path()
            .extension()
            .is_some_and(|extension| extension == "json")
        {
            fs::remove_file(entry.path())?;
        }
    }
    for record in records {
        let record_id = safe_identifier(id(record))?;
        fs::write(
            directory.join(format!("{record_id}.json")),
            serde_json::to_vec_pretty(record).expect("record serializable"),
        )?;
    }
    Ok(())
}

fn safe_project_path(project_root: &Path, path: &Path) -> io::Result<PathBuf> {
    let path = if path.is_absolute() {
        path.to_path_buf()
    } else {
        project_root.join(path)
    };
    let parent = path
        .parent()
        .ok_or_else(|| io::Error::new(io::ErrorKind::InvalidInput, "path has no parent"))?;
    let project_root = fs::canonicalize(project_root)?;
    let parent = fs::canonicalize(parent)?;
    if !parent.starts_with(&project_root) {
        return Err(io::Error::new(
            io::ErrorKind::PermissionDenied,
            "session path must remain within project root",
        ));
    }
    Ok(path)
}

fn read_swarm(project_root: &Path) -> io::Result<Option<SwarmRecord>> {
    let path = project_root.join(".swarm/state.json");
    if !path.is_file() {
        return Ok(None);
    }
    serde_json::from_slice(&fs::read(path)?)
        .map(Some)
        .map_err(io::Error::other)
}

fn write_swarm(project_root: &Path, swarm: &SwarmRecord) -> io::Result<()> {
    fs::write(
        project_root.join(".swarm/state.json"),
        serde_json::to_vec_pretty(swarm).expect("swarm serializable"),
    )
}

fn read_task(project_root: &Path, task_id: &str) -> io::Result<TaskRecord> {
    let task_id = safe_identifier(task_id)?;
    let path = project_root
        .join(".swarm/tasks")
        .join(format!("{task_id}.json"));
    serde_json::from_slice(&fs::read(path)?).map_err(io::Error::other)
}

fn write_task(project_root: &Path, task: &TaskRecord) -> io::Result<()> {
    let task_id = safe_identifier(&task.id)?;
    fs::write(
        project_root
            .join(".swarm/tasks")
            .join(format!("{task_id}.json")),
        serde_json::to_vec_pretty(task).expect("task serializable"),
    )
}

fn write_new_json(path: &Path, record: &TaskRecord) -> io::Result<()> {
    let mut file = fs::OpenOptions::new()
        .write(true)
        .create_new(true)
        .open(path)?;
    file.write_all(&serde_json::to_vec_pretty(record).expect("task serializable"))
}

fn normal_priority() -> String {
    "normal".into()
}

fn default_max_retries() -> u32 {
    3
}

fn default_timeout_ms() -> u64 {
    300_000
}

fn unique_millis() -> u128 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .expect("clock")
        .as_millis()
}

fn next_session_id(project_root: &Path) -> io::Result<String> {
    let base = unique_millis();
    for suffix in 0..1000_u32 {
        let id = if suffix == 0 {
            format!("session-{base}")
        } else {
            format!("session-{base}-{suffix}")
        };
        if !project_root
            .join(".claude-flow/sessions")
            .join(format!("{id}.json"))
            .exists()
        {
            return Ok(id);
        }
    }
    Err(io::Error::new(
        io::ErrorKind::AlreadyExists,
        "unable to allocate a unique session ID",
    ))
}

fn valid_topology(value: &str) -> io::Result<String> {
    match value {
        "hierarchical" | "mesh" | "ring" | "star" | "hybrid" | "hierarchical-mesh"
        | "pheromone-adaptive" => Ok(value.into()),
        _ => Err(io::Error::new(
            io::ErrorKind::InvalidInput,
            "unsupported swarm topology",
        )),
    }
}

fn valid_strategy(value: &str) -> io::Result<String> {
    match value {
        "specialized" | "balanced" | "adaptive" | "research" | "development" | "testing"
        | "optimization" | "maintenance" | "analysis" => Ok(value.into()),
        _ => Err(io::Error::new(
            io::ErrorKind::InvalidInput,
            "unsupported swarm strategy",
        )),
    }
}

fn valid_priority(value: &str) -> io::Result<String> {
    match value {
        "critical" | "high" | "normal" | "low" => Ok(value.into()),
        _ => Err(io::Error::new(
            io::ErrorKind::InvalidInput,
            "priority must be critical, high, normal, or low",
        )),
    }
}

fn count_json(directory: PathBuf) -> io::Result<usize> {
    if !directory.is_dir() {
        return Ok(0);
    }
    fs::read_dir(directory)?.try_fold(0, |count, entry| {
        let entry = entry?;
        Ok(count + usize::from(entry.path().extension().is_some_and(|ext| ext == "json")))
    })
}

fn safe_identifier(value: &str) -> io::Result<String> {
    if value.is_empty()
        || !value
            .chars()
            .all(|ch| ch.is_ascii_alphanumeric() || ch == '-' || ch == '_')
    {
        return Err(io::Error::new(
            io::ErrorKind::InvalidInput,
            "identifier must contain only letters, digits, '-' or '_'",
        ));
    }
    Ok(value.into())
}
