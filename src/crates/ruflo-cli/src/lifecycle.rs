use std::fs;
use std::io;
use std::path::{Path, PathBuf};

const CONFIG: &str = "# Native Ruflo project configuration\nversion: 3\n";

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ProjectStatus {
    pub agents: usize,
    pub tasks: usize,
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
