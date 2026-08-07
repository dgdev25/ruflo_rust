use std::fs;
use std::io;
use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};

const CONFIG: &str = "# Native Ruflo project configuration\nversion: 3\n";

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ProjectStatus {
    pub agents: usize,
    pub tasks: usize,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct AgentRecord {
    pub id: String,
    pub agent_type: String,
    pub status: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct TaskRecord {
    pub id: String,
    pub task_type: String,
    pub description: String,
    pub status: String,
}

pub fn create_task(
    project_root: &Path,
    task_type: &str,
    description: &str,
) -> io::Result<TaskRecord> {
    status(project_root)?;
    if description.trim().is_empty() {
        return Err(io::Error::new(
            io::ErrorKind::InvalidInput,
            "task description must not be empty",
        ));
    }
    let id = format!(
        "task-{}",
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .expect("clock")
            .as_millis()
    );
    let record = TaskRecord {
        id: id.clone(),
        task_type: safe_identifier(task_type)?,
        description: description.into(),
        status: "pending".into(),
    };
    fs::write(
        project_root.join(".swarm/tasks").join(format!("{id}.json")),
        serde_json::to_vec_pretty(&record).expect("task serializable"),
    )?;
    Ok(record)
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
