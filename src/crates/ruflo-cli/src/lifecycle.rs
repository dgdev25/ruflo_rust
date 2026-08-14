use std::fs;
use std::io;
use std::io::Write;
use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};

const CONFIG: &str = "# Native Ruflo project configuration\nversion: 3\n";

fn pid_is_alive(pid: u32) -> bool {
    #[cfg(unix)]
    {
        std::path::Path::new(&format!("/proc/{pid}")).exists()
    }
    #[cfg(not(unix))]
    {
        let _ = pid;
        false
    }
}

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

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SwarmScaleResult {
    pub swarm_id: String,
    pub target_agents: usize,
    pub delta: isize,
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
pub struct AgentPoolRecord {
    pub min_size: usize,
    pub max_size: usize,
    pub current_size: usize,
    pub auto_scale: bool,
}
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct AgentLogEntry {
    pub timestamp_ms: u128,
    pub level: String,
    pub message: String,
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
    if let Ok(store) = appliance_store(project_root) {
        let _ = store.clear_agents();
        for agent in &record.agents {
            let _ = persist_agent(project_root, agent);
        }
    }
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

pub fn scale_swarm(
    project_root: &Path,
    swarm_id: &str,
    target_agents: usize,
    agent_type: Option<&str>,
) -> io::Result<SwarmScaleResult> {
    if !(1..=100).contains(&target_agents) {
        return Err(io::Error::new(
            io::ErrorKind::InvalidInput,
            "target agent count must be between 1 and 100",
        ));
    }
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
    if swarm.status == "stopped" {
        return Err(io::Error::new(
            io::ErrorKind::InvalidInput,
            "stopped swarm cannot be scaled; initialize a new swarm",
        ));
    }

    let mut active = list_agents(project_root)?
        .into_iter()
        .filter(|agent| agent.status != "terminated")
        .collect::<Vec<_>>();
    let delta = target_agents as isize - active.len() as isize;
    if delta > 0 {
        let agent_type = safe_identifier(agent_type.unwrap_or("worker"))?;
        let mut next = 1_usize;
        while active.len() < target_agents {
            let id = format!("scale-{agent_type}-{next}");
            next += 1;
            if get_agent(project_root, &id).is_ok() {
                continue;
            }
            active.push(spawn_agent(project_root, &agent_type, &id)?);
        }
    } else if delta < 0 {
        active.sort_by(|left, right| right.id.cmp(&left.id));
        for agent in active.iter().take((-delta) as usize) {
            stop_agent(project_root, &agent.id)?;
        }
    }
    swarm.max_agents = target_agents;
    write_swarm(project_root, &swarm)?;
    Ok(SwarmScaleResult {
        swarm_id: swarm.id,
        target_agents,
        delta,
    })
}

pub fn coordinate_swarm(project_root: &Path, agents: usize) -> io::Result<SwarmRecord> {
    if !(1..=15).contains(&agents) {
        return Err(io::Error::new(
            io::ErrorKind::InvalidInput,
            "coordination agent count must be between 1 and 15",
        ));
    }
    status(project_root)?;
    let roles = [
        "queen-coordinator",
        "security-architect",
        "security-auditor",
        "test-architect",
        "core-architect",
        "memory-specialist",
        "swarm-specialist",
        "integration-architect",
        "performance-engineer",
        "cli-developer",
        "hooks-developer",
        "mcp-specialist",
        "project-coordinator",
        "documentation-lead",
        "devops-engineer",
    ];
    let mut swarm = read_swarm(project_root)?.unwrap_or(SwarmRecord {
        id: format!("swarm-{}", unique_millis()),
        topology: "hierarchical-mesh".into(),
        max_agents: agents,
        strategy: "specialized".into(),
        status: "ready".into(),
        objective: None,
    });
    if swarm.status == "stopped" {
        return Err(io::Error::new(
            io::ErrorKind::InvalidInput,
            "stopped swarm cannot coordinate",
        ));
    }
    swarm.topology = "hierarchical-mesh".into();
    swarm.strategy = "specialized".into();
    swarm.max_agents = agents;
    for (index, role) in roles.iter().take(agents).enumerate() {
        let id = format!("v3-{:02}-{}", index + 1, role);
        if get_agent(project_root, &id).is_err() {
            spawn_agent(project_root, role, &id)?;
        }
    }
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
            if get_agent(project_root, &agent_id).is_err() {
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

fn appliance_store(project_root: &Path) -> io::Result<ruflo_storage::ApplianceStore> {
    ruflo_storage::ApplianceStore::open(project_root).map_err(io::Error::other)
}

fn persist_agent(project_root: &Path, record: &AgentRecord) -> io::Result<()> {
    let store = appliance_store(project_root)?;
    store
        .upsert_agent(&ruflo_storage::AgentRow {
            id: record.id.clone(),
            agent_type: record.agent_type.clone(),
            status: record.status.clone(),
            role: record.agent_type.clone(),
            heartbeat_ms: 0,
        })
        .map_err(io::Error::other)?;
    Ok(())
}

fn migrate_json_agents(project_root: &Path) -> io::Result<()> {
    let dir = project_root.join(".swarm/agents");
    if !dir.is_dir() {
        return Ok(());
    }
    let store = appliance_store(project_root)?;
    for entry in fs::read_dir(&dir)? {
        let entry = entry?;
        if entry.path().extension().is_some_and(|ext| ext == "json") {
            if let Ok(record) = serde_json::from_slice::<AgentRecord>(&fs::read(entry.path())?) {
                let _ = store.upsert_agent(&ruflo_storage::AgentRow {
                    id: record.id,
                    agent_type: record.agent_type,
                    status: record.status,
                    role: String::new(),
                    heartbeat_ms: 0,
                });
            }
        }
    }
    Ok(())
}

pub fn spawn_agent(project_root: &Path, agent_type: &str, name: &str) -> io::Result<AgentRecord> {
    status(project_root)?;
    let id = safe_identifier(name)?;
    if get_agent(project_root, &id).is_ok() {
        return Err(io::Error::new(
            io::ErrorKind::AlreadyExists,
            format!("agent `{id}` already exists"),
        ));
    }
    let record = AgentRecord {
        id: id.clone(),
        agent_type: safe_identifier(agent_type)?,
        status: "idle".into(),
    };
    persist_agent(project_root, &record)?;
    append_agent_log(project_root, &id, "info", "agent spawned")?;
    Ok(record)
}

pub fn list_agents(project_root: &Path) -> io::Result<Vec<AgentRecord>> {
    status(project_root)?;
    migrate_json_agents(project_root)?;
    let store = appliance_store(project_root)?;
    let mut agents: Vec<AgentRecord> = store
        .list_agents()
        .map_err(io::Error::other)?
        .into_iter()
        .map(|row| AgentRecord {
            id: row.id,
            agent_type: row.agent_type,
            status: row.status,
        })
        .collect();
    agents.sort_by(|left, right| left.id.cmp(&right.id));
    Ok(agents)
}

pub fn get_agent(project_root: &Path, agent_id: &str) -> io::Result<AgentRecord> {
    status(project_root)?;
    let agent_id = safe_identifier(agent_id)?;
    migrate_json_agents(project_root)?;
    let store = appliance_store(project_root)?;
    store
        .get_agent(&agent_id)
        .map_err(io::Error::other)?
        .map(|row| AgentRecord {
            id: row.id,
            agent_type: row.agent_type,
            status: row.status,
        })
        .ok_or_else(|| io::Error::new(io::ErrorKind::NotFound, format!("agent `{agent_id}` not found")))
}

pub fn stop_agent(project_root: &Path, agent_id: &str) -> io::Result<AgentRecord> {
    let mut agent = get_agent(project_root, agent_id)?;
    agent.status = "terminated".into();
    persist_agent(project_root, &agent)?;
    append_agent_log(project_root, &agent.id, "info", "agent stopped")?;
    Ok(agent)
}

pub fn agent_logs(
    project_root: &Path,
    agent_id: &str,
    tail: usize,
    level: &str,
    since: Option<&str>,
) -> io::Result<Vec<AgentLogEntry>> {
    get_agent(project_root, agent_id)?;
    let minimum_level = log_level_rank(level)?;
    let since_ms = since.map(parse_since_ms).transpose()?;
    let path = project_root
        .join(".swarm/logs")
        .join(format!("{}.jsonl", safe_identifier(agent_id)?));
    let mut entries = if path.exists() {
        fs::read_to_string(path)?
            .lines()
            .map(|line| serde_json::from_str(line).map_err(io::Error::other))
            .collect::<io::Result<Vec<AgentLogEntry>>>()?
    } else {
        Vec::new()
    };
    entries.retain(|entry| {
        log_level_rank(&entry.level).is_ok_and(|rank| rank >= minimum_level)
            && since_ms.is_none_or(|threshold| entry.timestamp_ms >= threshold)
    });
    entries.reverse();
    entries.truncate(tail);
    Ok(entries)
}

fn log_level_rank(level: &str) -> io::Result<u8> {
    match level {
        "debug" => Ok(0),
        "info" => Ok(1),
        "warn" => Ok(2),
        "error" => Ok(3),
        _ => Err(io::Error::new(
            io::ErrorKind::InvalidInput,
            "log level must be debug, info, warn, or error",
        )),
    }
}

fn parse_since_ms(value: &str) -> io::Result<u128> {
    let (amount, unit) = value.split_at(value.len().saturating_sub(1));
    let amount = amount.parse::<u128>().map_err(|_| {
        io::Error::new(
            io::ErrorKind::InvalidInput,
            "log since must be a positive duration such as 30m or 1h",
        )
    })?;
    let multiplier = match unit {
        "s" => 1_000,
        "m" => 60_000,
        "h" => 3_600_000,
        "d" => 86_400_000,
        "w" => 604_800_000,
        _ => {
            return Err(io::Error::new(
                io::ErrorKind::InvalidInput,
                "log since must use s, m, h, d, or w units",
            ));
        }
    };
    if amount == 0 {
        return Err(io::Error::new(
            io::ErrorKind::InvalidInput,
            "log since must be a positive duration",
        ));
    }
    Ok(unique_millis().saturating_sub(amount.saturating_mul(multiplier)))
}

fn append_agent_log(
    project_root: &Path,
    agent_id: &str,
    level: &str,
    message: &str,
) -> io::Result<()> {
    let directory = project_root.join(".swarm/logs");
    fs::create_dir_all(&directory)?;
    let mut file = fs::OpenOptions::new()
        .append(true)
        .create(true)
        .open(directory.join(format!("{agent_id}.jsonl")))?;
    writeln!(
        file,
        "{}",
        serde_json::to_string(&AgentLogEntry {
            timestamp_ms: unique_millis(),
            level: level.into(),
            message: message.into()
        })
        .expect("log serializable")
    )
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

pub fn configure_agent_pool(
    project_root: &Path,
    size: Option<usize>,
    min: usize,
    max: usize,
    auto_scale: bool,
) -> io::Result<AgentPoolRecord> {
    status(project_root)?;
    if min == 0 || max == 0 || min > max || size.is_some_and(|value| value < min || value > max) {
        return Err(io::Error::new(
            io::ErrorKind::InvalidInput,
            "agent pool requires 1 <= min <= size <= max",
        ));
    }
    let current_size = size.unwrap_or_else(|| {
        list_agents(project_root).map_or(min, |agents| agents.len().clamp(min, max))
    });
    let record = AgentPoolRecord {
        min_size: min,
        max_size: max,
        current_size,
        auto_scale,
    };
    fs::write(
        project_root.join(".swarm/agent-pool.json"),
        serde_json::to_vec_pretty(&record).expect("pool serializable"),
    )?;
    Ok(record)
}

pub fn agent_health(
    project_root: &Path,
    agent_id: Option<&str>,
) -> io::Result<Vec<(AgentRecord, &'static str)>> {
    let agents = match agent_id {
        Some(id) => vec![get_agent(project_root, id)?],
        None => list_agents(project_root)?,
    };
    Ok(agents
        .into_iter()
        .map(|agent| {
            let _supervisor_live = std::fs::read_to_string(project_root.join(".claude-flow/daemon.pid"))
                .ok()
                .and_then(|s| s.trim().parse::<u32>().ok())
                .is_some_and(pid_is_alive);
            let health = if agent.status == "error" {
                "unhealthy"
            } else if agent.status == "terminated" {
                "degraded"
            } else {
                "healthy"
            };
            (agent, health)
        })
        .collect())
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
        ".swarm/logs",
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
