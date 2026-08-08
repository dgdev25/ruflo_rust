use std::fs::{self, File};
use std::io::{self, Write};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};
use std::time::{SystemTime, UNIX_EPOCH};

use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum DeploymentCommand {
    Overview,
    Help {
        subcommand: Option<String>,
    },
    Deploy {
        env: String,
        version: Option<String>,
        dry_run: bool,
        description: Option<String>,
    },
    Status {
        env: Option<String>,
    },
    Rollback {
        env: String,
        version: Option<String>,
        steps: i64,
    },
    History {
        env: Option<String>,
        limit: i64,
    },
    Environments {
        action: String,
        name: Option<String>,
        env_type: String,
        url: Option<String>,
    },
    Logs {
        deployment: Option<String>,
        env: Option<String>,
        lines: i64,
    },
    Release {
        version: Option<String>,
        env: String,
        description: Option<String>,
    },
}

#[derive(Debug, Clone, Serialize, Deserialize)]
struct DeploymentEnv {
    name: String,
    #[serde(rename = "type")]
    env_type: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    url: Option<String>,
    #[serde(rename = "createdAt")]
    created_at: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
struct DeploymentRecord {
    id: String,
    environment: String,
    version: String,
    status: String,
    timestamp: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    description: Option<String>,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
struct DeploymentState {
    #[serde(default)]
    environments: serde_json::Map<String, serde_json::Value>,
    #[serde(default)]
    history: Vec<DeploymentRecord>,
    #[serde(
        rename = "activeDeployment",
        default,
        skip_serializing_if = "Option::is_none"
    )]
    active_deployment: Option<String>,
}

impl DeploymentState {
    fn environment(&self, name: &str) -> Option<DeploymentEnv> {
        self.environments
            .get(name)
            .and_then(|value| serde_json::from_value(value.clone()).ok())
    }

    fn environment_values(&self) -> Vec<DeploymentEnv> {
        self.environments
            .values()
            .filter_map(|value| serde_json::from_value(value.clone()).ok())
            .collect()
    }

    fn insert_environment(&mut self, env: DeploymentEnv) {
        self.environments.insert(
            env.name.clone(),
            serde_json::to_value(env).expect("deployment environments serialize"),
        );
    }
}

pub fn run(root: &Path, command: DeploymentCommand) -> u8 {
    match run_inner(root, command) {
        Ok(code) => code,
        Err((heading, error)) => {
            eprintln!("[ERROR] {heading}");
            eprintln!("  {error}");
            1
        }
    }
}

fn run_inner(root: &Path, command: DeploymentCommand) -> Result<u8, (String, String)> {
    match command {
        DeploymentCommand::Overview => {
            print!("{OVERVIEW}");
            Ok(0)
        }
        DeploymentCommand::Help { subcommand } => {
            print!("{}", help(subcommand.as_deref()));
            Ok(0)
        }
        DeploymentCommand::Deploy {
            env,
            version,
            dry_run,
            description,
        } => deploy(root, env, version, dry_run, description)
            .map_err(|error| ("Deploy failed".into(), error.to_string())),
        DeploymentCommand::Status { env } => {
            status(root, env).map_err(|error| ("Status check failed".into(), error.to_string()))
        }
        DeploymentCommand::Rollback {
            env,
            version,
            steps: _,
        } => rollback(root, env, version)
            .map_err(|error| ("Rollback failed".into(), error.to_string())),
        DeploymentCommand::History { env, limit } => history(root, env, limit)
            .map_err(|error| ("Failed to load history".into(), error.to_string())),
        DeploymentCommand::Environments {
            action,
            name,
            env_type,
            url,
        } => environments(root, action, name, env_type, url)
            .map_err(|error| ("Environments command failed".into(), error.to_string())),
        DeploymentCommand::Logs {
            deployment,
            env,
            lines,
        } => logs(root, deployment, env, lines)
            .map_err(|error| ("Failed to load logs".into(), error.to_string())),
        DeploymentCommand::Release {
            version,
            env,
            description,
        } => release(root, version, env, description)
            .map_err(|error| ("Release failed".into(), error.to_string())),
    }
}

fn deploy(
    root: &Path,
    env_name: String,
    version: Option<String>,
    dry_run: bool,
    description: Option<String>,
) -> io::Result<u8> {
    let version = version
        .or_else(|| read_project_version(root))
        .unwrap_or_else(|| "0.0.0".into());
    let mut state = load(root);
    if state.environment(&env_name).is_none() {
        state.insert_environment(DeploymentEnv {
            name: env_name.clone(),
            env_type: match env_name.as_str() {
                "prod" | "production" => "production",
                "staging" => "staging",
                _ => "local",
            }
            .into(),
            url: None,
            created_at: now_iso8601(),
        });
    }
    let record = DeploymentRecord {
        id: generate_id(),
        environment: env_name.clone(),
        version: version.clone(),
        status: "deployed".into(),
        timestamp: now_iso8601(),
        description: description.clone(),
    };

    if dry_run {
        println!();
        eprintln!("[INFO] Dry run - no changes will be made");
        println!();
        println!("Deployment Preview");
        println!(
            "{}",
            table(
                &["Field", "Value"],
                &[
                    vec!["ID".into(), record.id],
                    vec!["Environment".into(), env_name],
                    vec!["Version".into(), version],
                    vec!["Status".into(), "deployed (dry-run)".into()],
                    vec![
                        "Description".into(),
                        description.unwrap_or_else(|| "-".into())
                    ],
                ]
            )
        );
        return Ok(0);
    }

    state.history.push(record.clone());
    state.active_deployment = Some(record.id.clone());
    save(root, &state)?;
    println!();
    println!("[OK] Deployed version {version} to {env_name}");
    println!();
    println!(
        "{}",
        table(
            &["Field", "Value"],
            &[
                vec!["ID".into(), record.id],
                vec!["Environment".into(), env_name],
                vec!["Version".into(), version],
                vec!["Status".into(), record.status],
                vec!["Timestamp".into(), record.timestamp],
                vec![
                    "Description".into(),
                    description.unwrap_or_else(|| "-".into())
                ],
            ]
        )
    );
    Ok(0)
}

fn status(root: &Path, filter_env: Option<String>) -> io::Result<u8> {
    let state = load(root);
    println!();
    println!("Deployment Status");
    println!();
    if let Some(active_id) = &state.active_deployment {
        if let Some(active) = state.history.iter().find(|record| &record.id == active_id) {
            eprintln!(
                "[INFO] Active deployment: {} (v{} on {})",
                active.id, active.version, active.environment
            );
        }
    } else {
        println!("No active deployment");
    }

    if let Some(name) = filter_env.as_deref() {
        let Some(env) = state.environment(name) else {
            eprintln!("[WARN] Environment '{name}' not found");
            return Ok(0);
        };
        println!();
        println!("Environment");
        println!("{}", environment_table(&[env]));
    } else {
        let envs = state.environment_values();
        if envs.is_empty() {
            println!("No environments configured");
        } else {
            println!();
            println!("Environments");
            println!("{}", environment_table(&envs));
        }
    }

    let mut recent = state
        .history
        .iter()
        .rev()
        .take(5)
        .cloned()
        .collect::<Vec<_>>();
    if let Some(name) = filter_env.as_deref() {
        recent.retain(|record| record.environment == name);
    }
    if !recent.is_empty() {
        println!();
        println!("Recent Deployments");
        println!("{}", record_table(&recent, false));
    }
    Ok(0)
}

fn rollback(root: &Path, env_name: String, target_version: Option<String>) -> io::Result<u8> {
    if env_name.is_empty() {
        eprintln!("[ERROR] Environment is required");
        eprintln!("  Use --env or -e to specify");
        return Ok(1);
    }
    let mut state = load(root);
    let env_history = state
        .history
        .iter()
        .rev()
        .filter(|record| record.environment == env_name && record.status == "deployed")
        .cloned()
        .collect::<Vec<_>>();
    if env_history.len() < 2 && target_version.is_none() {
        eprintln!("[WARN] No previous deployment to rollback to");
        return Ok(1);
    }
    let rollback_to = if let Some(target) = target_version {
        let Some(record) = env_history.iter().find(|record| record.version == target) else {
            eprintln!(
                "[ERROR] Version '{target}' not found in deployment history for '{env_name}'"
            );
            return Ok(1);
        };
        record.clone()
    } else {
        env_history[1].clone()
    };
    let current = env_history.first().cloned();
    if let Some(current) = &current {
        if let Some(record) = state
            .history
            .iter_mut()
            .find(|record| record.id == current.id)
        {
            record.status = "rolled-back".into();
        }
    }
    let from_version = current
        .as_ref()
        .map(|record| record.version.clone())
        .unwrap_or_else(|| "unknown".into());
    let record = DeploymentRecord {
        id: generate_id(),
        environment: env_name.clone(),
        version: rollback_to.version.clone(),
        status: "deployed".into(),
        timestamp: now_iso8601(),
        description: Some(format!(
            "Rollback from {from_version} to {}",
            rollback_to.version
        )),
    };
    state.history.push(record.clone());
    state.active_deployment = Some(record.id.clone());
    save(root, &state)?;
    println!();
    println!(
        "[OK] Rolled back {env_name} to version {}",
        rollback_to.version
    );
    println!();
    println!(
        "{}",
        table(
            &["Field", "Value"],
            &[
                vec!["Rollback ID".into(), record.id],
                vec!["Environment".into(), env_name],
                vec!["From Version".into(), from_version],
                vec!["To Version".into(), rollback_to.version],
                vec!["Timestamp".into(), record.timestamp],
            ]
        )
    );
    Ok(0)
}

fn history(root: &Path, filter_env: Option<String>, limit: i64) -> io::Result<u8> {
    let state = load(root);
    let mut records = state.history.iter().rev().cloned().collect::<Vec<_>>();
    if let Some(name) = filter_env.as_deref() {
        records.retain(|record| record.environment == name);
    }
    records = js_slice_zero(records, if limit == 0 { 10 } else { limit });
    println!();
    println!("Deployment History");
    if let Some(name) = filter_env {
        println!("Filtered by environment: {name}");
    }
    println!();
    if records.is_empty() {
        println!("No deployment history found");
        return Ok(0);
    }
    println!("{}", record_table(&records, true));
    println!();
    println!(
        "Showing {} of {} total records",
        records.len(),
        state.history.len()
    );
    Ok(0)
}

fn environments(
    root: &Path,
    action: String,
    name: Option<String>,
    env_type: String,
    url: Option<String>,
) -> io::Result<u8> {
    let mut state = load(root);
    match action.as_str() {
        "list" => {
            let envs = state.environment_values();
            println!();
            println!("Deployment Environments");
            println!();
            if envs.is_empty() {
                println!("No environments configured. Use --action add to create one.");
            } else {
                println!("{}", environment_table(&envs));
            }
            Ok(0)
        }
        "add" => {
            let Some(name) = name else {
                eprintln!("[ERROR] Environment name is required");
                eprintln!("  Use --name or -n to specify");
                return Ok(1);
            };
            if state.environments.contains_key(&name) {
                eprintln!("[WARN] Environment '{name}' already exists");
                return Ok(1);
            }
            state.insert_environment(DeploymentEnv {
                name: name.clone(),
                env_type: env_type.clone(),
                url: url.clone(),
                created_at: now_iso8601(),
            });
            save(root, &state)?;
            println!();
            println!("[OK] Added environment '{name}' ({env_type})");
            if let Some(url) = url {
                println!("  URL: {url}");
            }
            Ok(0)
        }
        "remove" => {
            let Some(name) = name else {
                eprintln!("[ERROR] Environment name is required");
                eprintln!("  Use --name or -n to specify");
                return Ok(1);
            };
            if state.environments.remove(&name).is_none() {
                eprintln!("[WARN] Environment '{name}' not found");
                return Ok(1);
            }
            save(root, &state)?;
            println!();
            println!("[OK] Removed environment '{name}'");
            Ok(0)
        }
        _ => {
            eprintln!("[ERROR] Unknown action '{action}'");
            eprintln!("  Valid actions: list, add, remove");
            Ok(1)
        }
    }
}

fn logs(
    root: &Path,
    deployment_id: Option<String>,
    filter_env: Option<String>,
    lines: i64,
) -> io::Result<u8> {
    let state = load(root);
    println!();
    println!("Deployment Logs");
    println!();
    let mut records = state.history.iter().rev().cloned().collect::<Vec<_>>();
    if let Some(id) = deployment_id {
        records.retain(|record| record.id == id);
        if records.is_empty() {
            eprintln!("[WARN] Deployment '{id}' not found");
            return Ok(1);
        }
    }
    if let Some(name) = filter_env {
        records.retain(|record| record.environment == name);
    }
    records = js_slice_zero(records, if lines == 0 { 50 } else { lines });
    if records.is_empty() {
        println!("No deployment logs found");
        return Ok(0);
    }
    println!("{}", record_table(&records, true));
    println!();
    println!("{} entries shown", records.len());
    Ok(0)
}

fn release(
    root: &Path,
    version: Option<String>,
    env_name: String,
    description: Option<String>,
) -> io::Result<u8> {
    let Some(version) = version.or_else(|| read_project_version(root)) else {
        eprintln!("[ERROR] Version is required");
        eprintln!("  Use --version or -v, or ensure package.json has a version field");
        return Ok(1);
    };
    let mut state = load(root);
    if state.environment(&env_name).is_none() {
        state.insert_environment(DeploymentEnv {
            name: env_name.clone(),
            env_type: if matches!(env_name.as_str(), "prod" | "production") {
                "production"
            } else {
                "staging"
            }
            .into(),
            url: None,
            created_at: now_iso8601(),
        });
    }
    let description = description.unwrap_or_else(|| format!("Release {version}"));
    let record = DeploymentRecord {
        id: generate_id(),
        environment: env_name.clone(),
        version: version.clone(),
        status: "deployed".into(),
        timestamp: now_iso8601(),
        description: Some(description.clone()),
    };
    state.history.push(record.clone());
    state.active_deployment = Some(record.id.clone());
    save(root, &state)?;
    println!();
    println!("[OK] Released version {version} to {env_name}");
    println!();
    println!(
        "{}",
        table(
            &["Field", "Value"],
            &[
                vec!["Release ID".into(), record.id],
                vec!["Environment".into(), env_name],
                vec!["Version".into(), version],
                vec!["Status".into(), record.status],
                vec!["Timestamp".into(), record.timestamp],
                vec!["Description".into(), description],
            ]
        )
    );
    Ok(0)
}

fn load(root: &Path) -> DeploymentState {
    fs::read_to_string(state_path(root))
        .ok()
        .and_then(|raw| serde_json::from_str(&raw).ok())
        .unwrap_or_default()
}

fn save(root: &Path, state: &DeploymentState) -> io::Result<()> {
    let dir = root.join(".claude-flow");
    fs::create_dir_all(&dir)?;
    let path = state_path(root);
    let tmp = PathBuf::from(format!("{}.tmp", path.display()));
    let bytes = serde_json::to_vec_pretty(state).expect("deployment state serializes");
    let mut file = File::create(&tmp)?;
    file.write_all(&bytes)?;
    file.sync_all()?;
    fs::rename(&tmp, &path)?;
    if let Ok(dir_file) = File::open(dir) {
        let _ = dir_file.sync_all();
    }
    Ok(())
}

fn state_path(root: &Path) -> PathBuf {
    root.join(".claude-flow/deployments.json")
}

fn read_project_version(root: &Path) -> Option<String> {
    let value: serde_json::Value =
        serde_json::from_str(&fs::read_to_string(root.join("package.json")).ok()?).ok()?;
    value.get("version").and_then(|version| match version {
        serde_json::Value::Null => None,
        serde_json::Value::String(value) => Some(value.clone()),
        other => Some(js_string(other)),
    })
}

fn js_string(value: &serde_json::Value) -> String {
    match value {
        serde_json::Value::String(value) => value.clone(),
        serde_json::Value::Bool(value) => value.to_string(),
        serde_json::Value::Number(value) => value.to_string(),
        serde_json::Value::Null => "null".into(),
        serde_json::Value::Array(_) => value
            .as_array()
            .expect("array")
            .iter()
            .map(js_string)
            .collect::<Vec<_>>()
            .join(","),
        serde_json::Value::Object(_) => "[object Object]".into(),
    }
}

fn environment_table(envs: &[DeploymentEnv]) -> String {
    table(
        &["Name", "Type", "URL", "Created"],
        &envs
            .iter()
            .map(|env| {
                vec![
                    env.name.clone(),
                    env.env_type.clone(),
                    env.url.clone().unwrap_or_else(|| "-".into()),
                    env.created_at.clone(),
                ]
            })
            .collect::<Vec<_>>(),
    )
}

fn record_table(records: &[DeploymentRecord], description: bool) -> String {
    let mut headers = vec!["ID", "Env", "Version", "Status", "Time"];
    if description {
        headers.push("Description");
    }
    let rows = records
        .iter()
        .map(|record| {
            let mut row = vec![
                record.id.clone(),
                record.environment.clone(),
                record.version.clone(),
                record.status.clone(),
                record.timestamp.clone(),
            ];
            if description {
                row.push(record.description.clone().unwrap_or_else(|| "-".into()));
            }
            row
        })
        .collect::<Vec<_>>();
    table(&headers, &rows)
}

fn table(headers: &[&str], rows: &[Vec<String>]) -> String {
    let widths = headers
        .iter()
        .enumerate()
        .map(|(index, header)| {
            rows.iter()
                .filter_map(|row| row.get(index))
                .map(String::len)
                .max()
                .unwrap_or(0)
                .max(header.len())
        })
        .collect::<Vec<_>>();
    let border = format!(
        "+{}+",
        widths
            .iter()
            .map(|width| "-".repeat(width + 2))
            .collect::<Vec<_>>()
            .join("+")
    );
    let row = |values: Vec<String>| {
        format!(
            "|{}|",
            values
                .into_iter()
                .enumerate()
                .map(|(index, value)| format!(" {:<width$} ", value, width = widths[index]))
                .collect::<Vec<_>>()
                .join("|")
        )
    };
    let mut lines = vec![
        border.clone(),
        row(headers.iter().map(|header| (*header).into()).collect()),
        border.clone(),
    ];
    lines.extend(rows.iter().cloned().map(row));
    lines.push(border);
    lines.join("\n")
}

fn js_slice_zero<T>(mut values: Vec<T>, end: i64) -> Vec<T> {
    let len = values.len() as i64;
    let end = if end < 0 {
        (len + end).max(0)
    } else {
        end.min(len)
    };
    values.truncate(end as usize);
    values
}

fn generate_id() -> String {
    static COUNTER: AtomicU64 = AtomicU64::new(0);
    let millis = unix_millis();
    let seed = millis
        ^ (std::process::id() as u64).rotate_left(17)
        ^ COUNTER
            .fetch_add(1, Ordering::Relaxed)
            .wrapping_mul(0x9e37_79b9);
    let random = base36(seed.wrapping_mul(6_364_136_223_846_793_005))[..6].to_string();
    format!("dep-{}-{random}", base36(millis))
}

fn base36(mut value: u64) -> String {
    const DIGITS: &[u8; 36] = b"0123456789abcdefghijklmnopqrstuvwxyz";
    if value == 0 {
        return "000000".into();
    }
    let mut result = Vec::new();
    while value > 0 {
        result.push(DIGITS[(value % 36) as usize] as char);
        value /= 36;
    }
    while result.len() < 6 {
        result.push('0');
    }
    result.iter().rev().collect()
}

fn unix_millis() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_millis() as u64
}

fn now_iso8601() -> String {
    let millis = unix_millis() as i64;
    let seconds = millis.div_euclid(1000);
    let sub_millis = millis.rem_euclid(1000);
    let days = seconds.div_euclid(86_400);
    let seconds_of_day = seconds.rem_euclid(86_400);
    let (year, month, day) = civil_from_days(days);
    let hour = seconds_of_day / 3600;
    let minute = seconds_of_day % 3600 / 60;
    let second = seconds_of_day % 60;
    format!("{year:04}-{month:02}-{day:02}T{hour:02}:{minute:02}:{second:02}.{sub_millis:03}Z")
}

fn civil_from_days(days_since_epoch: i64) -> (i64, i64, i64) {
    let z = days_since_epoch + 719_468;
    let era = if z >= 0 { z } else { z - 146_096 } / 146_097;
    let day_of_era = z - era * 146_097;
    let year_of_era =
        (day_of_era - day_of_era / 1460 + day_of_era / 36_524 - day_of_era / 146_096) / 365;
    let mut year = year_of_era + era * 400;
    let day_of_year = day_of_era - (365 * year_of_era + year_of_era / 4 - year_of_era / 100);
    let month_prime = (5 * day_of_year + 2) / 153;
    let day = day_of_year - (153 * month_prime + 2) / 5 + 1;
    let month = month_prime + if month_prime < 10 { 3 } else { -9 };
    year += i64::from(month <= 2);
    (year, month, day)
}

const OVERVIEW: &str = "\nRuFlo Deployment\nMulti-environment deployment management\n\nSubcommands:\n  - deploy       - Deploy to target environment\n  - status       - Check deployment status\n  - rollback     - Rollback to previous version\n  - history      - View deployment history\n  - environments - Manage deployment environments\n  - logs         - View deployment logs\n  - release      - Create a new release\n\nFeatures:\n  - Zero-downtime rolling deployments\n  - Automatic rollback on failure\n  - Environment-specific configurations\n  - Deployment previews for PRs\n\nCreated with love by ruv.io\n";

fn help(subcommand: Option<&str>) -> &'static str {
    match subcommand {
        Some("deploy") => "\nruflo deployment deploy\nDeploy to target environment\n\nOPTIONS:\n  -e, --env <value>          Environment: dev, staging, prod [default: staging]\n  -v, --version <value>      Version to deploy\n  -d, --dry-run              Simulate deployment without changes\n      --description <value>  Deployment description\n",
        Some("status") => "\nruflo deployment status\nCheck deployment status across environments\n\nOPTIONS:\n  -e, --env <value>  Specific environment to check\n",
        Some("rollback") => "\nruflo deployment rollback\nRollback to previous deployment\n\nOPTIONS:\n  -e, --env <value>      Environment to rollback (required)\n  -v, --version <value>  Specific version to rollback to\n  -s, --steps <number>   Number of versions to rollback [default: 1]\n",
        Some("history") => "\nruflo deployment history\nView deployment history\n\nOPTIONS:\n  -e, --env <value>     Filter by environment\n  -l, --limit <number>  Number of entries [default: 10]\n",
        Some("environments" | "envs") => "\nruflo deployment environments\nManage deployment environments\n\nOPTIONS:\n  -a, --action <value>  Action: list, add, remove [default: list]\n  -n, --name <value>    Environment name\n  -t, --type <value>    Environment type: local, staging, production [default: local]\n  -u, --url <value>     Environment URL\n",
        Some("logs") => "\nruflo deployment logs\nView deployment logs\n\nOPTIONS:\n  -d, --deployment <value>  Deployment ID\n  -e, --env <value>         Environment\n  -n, --lines <number>      Number of lines [default: 50]\n",
        Some("release") => "\nruflo deployment release\nCreate a new release deployment\n\nOPTIONS:\n  -v, --version <value>      Release version\n  -e, --env <value>          Target environment [default: production]\n  -d, --description <value>  Release description\n",
        _ => "\nruflo deployment\nDeployment management, environments, rollbacks\n\nSUBCOMMANDS:\n  deploy        Deploy to target environment\n  status        Check deployment status across environments\n  rollback      Rollback to previous deployment\n  history       View deployment history\n  environments  Manage deployment environments (alias: envs)\n  logs          View deployment logs\n  release       Create a new release deployment\n",
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn timestamp_is_javascript_compatible_utc() {
        let value = now_iso8601();
        assert_eq!(value.len(), 24);
        assert_eq!(&value[4..5], "-");
        assert_eq!(&value[10..11], "T");
        assert!(value.ends_with('Z'));
    }

    #[test]
    fn save_uses_atomic_sibling_and_round_trips() {
        let project = tempfile::tempdir().unwrap();
        let mut state = DeploymentState::default();
        state.insert_environment(DeploymentEnv {
            name: "prod".into(),
            env_type: "production".into(),
            url: None,
            created_at: "2026-08-08T00:00:00.000Z".into(),
        });
        save(project.path(), &state).unwrap();
        assert!(!project
            .path()
            .join(".claude-flow/deployments.json.tmp")
            .exists());
        assert!(load(project.path()).environment("prod").is_some());
    }
}
