//! Native V3 `hooks` command — self-learning hooks system.
//!
//! Source: `v3/@claude-flow/cli/src/commands/hooks.ts` (5708 lines, ~35
//! subcommands). The hooks system is the integration layer Claude Code invokes
//! (PreToolUse / PostToolUse / etc. via `settings.json`) to record workflow
//! events and feed the SONA learning loop + model router.
//!
//! In the native build the CLI binary IS the hook target: when Claude Code
//! shells out to `ruflo hooks <event>`, the native binary records the event to
//! `.claude-flow/hooks-events.jsonl` and returns — exactly the side-effect a
//! hook produces. The SONA/EWC learning that consumes those events runs in the
//! Node daemon, so the *learning* step degrades but the *recording* is real and
//! queryable (`metrics`, `list`, `model-stats`). `route` does real keyword
//! routing (Q-learning deferred) and `statusline` renders from persisted state.

use std::fs;
use std::io::Write;
use std::os::unix::fs::OpenOptionsExt;
use std::path::{Path, PathBuf};
use std::time::{SystemTime, UNIX_EPOCH};

use serde_json::{json, Value};

fn now_ms() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_millis() as u64)
        .unwrap_or(0)
}

fn hooks_dir(root: &Path) -> PathBuf {
    root.join(".claude-flow")
}

fn events_file(root: &Path) -> PathBuf {
    hooks_dir(root).join("hooks-events.jsonl")
}

fn model_state_file(root: &Path) -> PathBuf {
    hooks_dir(root).join("model-routing.json")
}

fn decisions_file(root: &Path) -> PathBuf {
    hooks_dir(root).join("route-decisions.jsonl")
}

fn append_jsonl(path: &Path, v: &Value) {
    if let Some(dir) = path.parent() {
        let _ = fs::create_dir_all(dir);
    }
    if let Ok(mut f) = fs::OpenOptions::new()
        .create(true)
        .append(true)
        .mode(0o600)
        .open(path)
    {
        let _ = writeln!(f, "{}", v);
    }
}

fn read_jsonl(path: &Path) -> Vec<Value> {
    let mut out = Vec::new();
    if let Ok(raw) = fs::read_to_string(path) {
        for line in raw.lines() {
            if let Ok(v) = serde_json::from_str::<Value>(line) {
                out.push(v);
            }
        }
    }
    out
}

fn read_json(path: &Path) -> Value {
    fs::read_to_string(path)
        .ok()
        .and_then(|s| serde_json::from_str(&s).ok())
        .unwrap_or_else(|| json!({}))
}

fn write_json_atomic(path: &Path, v: &Value) -> bool {
    if let Some(dir) = path.parent() {
        let _ = fs::create_dir_all(dir);
    }
    let tmp = path.with_extension("json.tmp");
    let bytes = serde_json::to_vec_pretty(v).unwrap_or_default();
    if fs::write(&tmp, &bytes).is_err() {
        return false;
    }
    let ok = fs::rename(&tmp, path).is_ok();
    if !ok {
        let _ = fs::remove_file(&tmp);
    }
    ok
}

// Static hook catalog (matches the hooks the Node runtime registers).
fn hook_catalog() -> &'static [(&'static str, &'static str)] {
    &[
        ("pre-edit", "PreToolUse"),
        ("post-edit", "PostToolUse"),
        ("pre-command", "PreToolUse"),
        ("post-command", "PostToolUse"),
        ("pre-task", "PreToolUse"),
        ("post-task", "PostToolUse"),
        ("pre-bash", "PreToolUse"),
        ("post-bash", "PostToolUse"),
        ("session-start", "SessionStart"),
        ("session-end", "SessionEnd"),
        ("session-restore", "SessionStart"),
        ("route", "UserPromptSubmit"),
        ("model-route", "PreToolUse"),
        ("teammate-idle", "Stop"),
        ("task-completed", "Stop"),
        ("notify", "Notification"),
        ("statusline", "StatusLine"),
    ]
}

#[derive(Debug, Clone, PartialEq)]
pub struct HooksCommand {
    pub operation: String,
    pub task: Option<String>,
    pub description: Option<String>,
    pub file_path: Option<String>,
    pub command: Option<String>,
    pub agent: Option<String>,
    pub task_id: Option<String>,
    pub model: Option<String>,
    pub outcome: Option<String>,
    pub enabled: bool,
    pub hook_type: Option<String>,
    pub json: bool,
    pub verbose: bool,
    pub positional: Vec<String>,
}

pub fn run(root: &Path, command: HooksCommand) -> u8 {
    let op = command.operation.as_str();
    match op {
        "" => overview(&command),
        "list" | "ls" => list(root, &command),
        "metrics" => metrics(root, &command),
        "route" | "route-task" => route_cmd(root, &command),
        "explain" => explain(root, &command),
        "model-route" => model_route(root, &command),
        "model-stats" => model_stats(root, &command),
        "model-outcome" => model_outcome(root, &command),
        "worker-list" | "worker-status" => worker_list(&command),
        "worker-dispatch" | "worker-detect" | "worker-cancel" => worker_op(op, &command),
        "intelligence" => intelligence(root, &command),
        "statusline" => statusline(root, &command),
        "notify" => notify(&command),
        "build-agents" => build_agents(&command),
        "pretrain" | "transfer" | "transfer-from-project" | "coverage-route"
        | "coverage-suggest" | "coverage-gaps" | "token-optimize" | "refresh-funnel"
        | "refresh-advisor" | "progress" => degrade(op, &command),
        // Hook-event ops: record the event + return success.
        "pre-edit" | "post-edit" | "pre-command" | "post-command" | "pre-task"
        | "post-task" | "pre-bash" | "post-bash" | "session-start" | "session-end"
        | "session-restore" | "teammate-idle" | "task-completed" => record_event(root, op, &command),
        _ => {
            eprintln!("[ERROR] Unknown hooks op: {op}");
            1
        }
    }
}

fn overview(_command: &HooksCommand) -> u8 {
    println!("\nHooks");
    println!("Self-learning hooks system for workflow automation.\n");
    println!("Subcommands:");
    println!("  pre-edit / post-edit        Edit lifecycle hooks");
    println!("  pre-command / post-command  Command lifecycle hooks");
    println!("  pre-task / post-task        Task lifecycle hooks");
    println!("  session-start / end / restore  Session hooks");
    println!("  route / route-task          Route a task to an agent");
    println!("  explain                     Explain the last routing decision");
    println!("  model-route / model-stats / model-outcome  Model routing");
    println!("  list                        List registered hooks");
    println!("  metrics                     Show hook metrics");
    println!("  worker-list / worker-status List background workers");
    println!("  intelligence                Neural intelligence stats");
    println!("  statusline                  Render statusline");
    println!("  notify                      Send a notification");
    println!("\nHook-event ops record to .claude-flow/hooks-events.jsonl.");
    0
}

// ---- event recording --------------------------------------------------------

fn record_event(root: &Path, event: &str, command: &HooksCommand) -> u8 {
    let rec = json!({
        "event": event,
        "at": now_ms(),
        "filePath": command.file_path,
        "command": command.command,
        "taskId": command.task_id,
        "agent": command.agent,
        "description": command.description,
    });
    append_jsonl(&events_file(root), &rec);
    if command.json {
        println!("{}", rec);
    }
    // Hooks return success unless they intend to block; native records only.
    0
}

// ---- list -------------------------------------------------------------------

fn list(root: &Path, command: &HooksCommand) -> u8 {
    let events = read_jsonl(&events_file(root));
    let mut catalog: Vec<(&'static str, &'static str, u64)> = hook_catalog()
        .iter()
        .map(|(name, ty)| {
            let count = events.iter().filter(|e| e["event"].as_str() == Some(*name)).count() as u64;
            (*name, *ty, count)
        })
        .collect();
    if command.enabled {
        // All catalog hooks are "enabled" in the native build (no registry to toggle).
    }
    if let Some(t) = &command.hook_type {
        catalog.retain(|(_, ty, _)| *ty == t.as_str());
    }
    if command.json {
        let out = json!({
            "hooks": catalog.iter().map(|(n, t, c)| json!({
                "name": n, "type": t, "enabled": true, "executionCount": c,
            })).collect::<Vec<_>>(),
            "total": catalog.len(),
        });
        println!("{}", serde_json::to_string_pretty(&out).unwrap_or_default());
        return 0;
    }
    println!("\nRegistered Hooks ({})", catalog.len());
    println!("{}", "\u{2500}".repeat(50));
    println!("  {:<18} {:<16} {:>10}", "Name", "Type", "Executions");
    println!("  {} {} {}", "\u{2500}".repeat(18), "\u{2500}".repeat(16), "\u{2500}".repeat(10));
    for (n, t, c) in &catalog {
        println!("  {:<18} {:<16} {c:>10}", n, t);
    }
    0
}

// ---- metrics ----------------------------------------------------------------

fn metrics(root: &Path, command: &HooksCommand) -> u8 {
    let events = read_jsonl(&events_file(root));
    let total = events.len();
    let mut by_event: std::collections::BTreeMap<String, u64> = std::collections::BTreeMap::new();
    for e in &events {
        if let Some(ev) = e["event"].as_str() {
            *by_event.entry(ev.to_string()).or_insert(0) += 1;
        }
    }
    if command.json {
        let out = json!({"totalEvents": total, "byEvent": by_event});
        println!("{}", serde_json::to_string_pretty(&out).unwrap_or_default());
        return 0;
    }
    println!("\nHook Metrics");
    println!("{}", "\u{2500}".repeat(50));
    println!("  Total events recorded: {total}");
    if by_event.is_empty() {
        println!("  (no events yet — hooks record as Claude Code fires them)");
    } else {
        println!("\n  {:<20} {}", "Event", "Count");
        println!("  {} {}", "\u{2500}".repeat(20), "\u{2500}".repeat(8));
        for (e, c) in &by_event {
            println!("  {:<20} {c}", e);
        }
    }
    0
}

// ---- route ------------------------------------------------------------------

const ROUTE_AGENTS: &[(&str, &[&str])] = &[
    ("coder", &["coding", "implementation", "refactor", "fix", "build"]),
    ("tester", &["testing", "validation", "quality", "test"]),
    ("reviewer", &["review", "security", "best-practices", "audit"]),
    ("architect", &["design", "architecture", "planning", "plan"]),
    ("researcher", &["research", "analysis", "documentation", "investigate"]),
    ("optimizer", &["optimization", "performance", "profiling", "optimize"]),
    ("debugger", &["debugging", "troubleshooting", "bug", "debug"]),
    ("documenter", &["documentation", "writing", "docs"]),
];

fn route_cmd(root: &Path, command: &HooksCommand) -> u8 {
    let Some(task) = &command.task else {
        eprintln!("[ERROR] Task description required: hooks route -t \"<description>\"");
        return 1;
    };
    let lower = task.to_lowercase();
    let mut best = "coder";
    let mut best_score = 0i32;
    for (agent, caps) in ROUTE_AGENTS {
        let score = caps.iter().map(|c| if lower.contains(c) { 2 } else { 0 }).sum::<i32>();
        if score > best_score {
            best_score = score;
            best = agent;
        }
    }
    let tier = if best_score >= 4 { 3 } else { 2 };
    let decision = json!({
        "task": task, "agent": best, "tier": tier, "score": best_score,
        "model": tier_label(tier), "at": now_ms(),
    });
    append_jsonl(&decisions_file(root), &decision);
    if command.json {
        println!("{}", serde_json::to_string_pretty(&decision).unwrap_or_default());
        return 0;
    }
    println!("\nRouting Decision");
    println!("  Task:  {task}");
    println!("  Agent: {best}");
    println!("  Tier:  {tier} ({})", tier_label(tier));
    println!("  Score: {best_score}");
    0
}

fn tier_label(tier: u8) -> &'static str {
    match tier {
        1 => "haiku",
        2 => "sonnet",
        _ => "opus",
    }
}

fn explain(root: &Path, command: &HooksCommand) -> u8 {
    let decisions = read_jsonl(&decisions_file(root));
    let Some(last) = decisions.last() else {
        eprintln!("[ERROR] No routing decisions recorded.");
        return 1;
    };
    if command.json {
        println!("{}", serde_json::to_string_pretty(last).unwrap_or_default());
        return 0;
    }
    println!("\nLast Routing Decision");
    println!("{}", "\u{2500}".repeat(50));
    println!("  Task:  {}", last["task"].as_str().unwrap_or("?"));
    println!("  Agent: {}", last["agent"].as_str().unwrap_or("?"));
    println!("  Tier:  {} ({})", last["tier"], last["model"].as_str().unwrap_or("?"));
    println!("  Score: {}", last["score"]);
    println!("  Reason: keyword-match routing (Q-learning deferred to Node daemon)");
    0
}

// ---- model routing ----------------------------------------------------------

fn model_route(root: &Path, command: &HooksCommand) -> u8 {
    let Some(task) = &command.task else {
        eprintln!("[ERROR] Task description required: hooks model-route -t \"<description>\"");
        return 1;
    };
    let lower = task.to_lowercase();
    let (model, tier) = if lower.contains("security") || lower.contains("architect") {
        ("opus", 3)
    } else if lower.contains("fix") || lower.contains("test") || lower.contains("doc") {
        ("haiku", 1)
    } else {
        ("sonnet", 2)
    };
    let rec = json!({"task": task, "model": model, "tier": tier, "at": now_ms()});
    append_jsonl(&model_state_file(root).with_extension("jsonl"), &rec);
    if command.json {
        println!("{}", serde_json::to_string_pretty(&rec).unwrap_or_default());
        return 0;
    }
    println!("model-route: {model} (tier {tier}) for \"{task}\"");
    0
}

fn model_stats(root: &Path, command: &HooksCommand) -> u8 {
    let path = model_state_file(root).with_extension("jsonl");
    let events = read_jsonl(&path);
    let state = read_json(&model_state_file(root));
    let mut by_model: std::collections::BTreeMap<String, u64> = std::collections::BTreeMap::new();
    for e in &events {
        if let Some(m) = e["model"].as_str() {
            *by_model.entry(m.to_string()).or_insert(0) += 1;
        }
    }
    if command.json {
        let out = json!({"decisions": events.len(), "byModel": by_model, "learned": state});
        println!("{}", serde_json::to_string_pretty(&out).unwrap_or_default());
        return 0;
    }
    println!("\nModel Routing Stats");
    println!("{}", "\u{2500}".repeat(50));
    println!("  Decisions recorded: {}", events.len());
    if by_model.is_empty() {
        println!("  (none yet)");
    } else {
        for (m, c) in &by_model {
            println!("    {m}: {c}");
        }
    }
    0
}

fn model_outcome(root: &Path, command: &HooksCommand) -> u8 {
    let Some(model) = &command.model else {
        eprintln!("[ERROR] --model is required");
        return 1;
    };
    let outcome = command.outcome.clone().unwrap_or_else(|| "success".into());
    if !matches!(outcome.as_str(), "success" | "failure" | "escalated") {
        eprintln!("[ERROR] --outcome must be success|failure|escalated");
        return 1;
    }
    let mut state = read_json(&model_state_file(root));
    let key = format!("{model}.{outcome}");
    let cur = state[key.as_str()].as_u64().unwrap_or(0);
    state[key.as_str()] = json!(cur + 1);
    if !write_json_atomic(&model_state_file(root), &state) {
        eprintln!("[ERROR] Failed to persist model-routing state.");
        return 1;
    }
    println!("Recorded outcome '{outcome}' for model '{model}'.");
    0
}

// ---- workers ----------------------------------------------------------------

fn worker_catalog() -> &'static [(&'static str, &'static str, &'static str)] {
    &[
        ("ultralearn", "Deep multi-repo learning scan", "idle"),
        ("optimize", "Performance optimization analysis", "idle"),
        ("consolidate", "Memory consolidation + compaction", "idle"),
        ("predict", "Predictive task pre-staging", "idle"),
        ("audit", "Security audit sweep", "idle"),
        ("map", "Codebase cartography", "idle"),
        ("preload", "Warm cache for likely-next tasks", "idle"),
        ("deepdive", "Deep dependency analysis", "idle"),
        ("document", "Doc generation", "idle"),
        ("refactor", "Refactor opportunity scan", "idle"),
        ("benchmark", "Performance benchmark runner", "idle"),
        ("testgaps", "Test-coverage gap finder", "idle"),
    ]
}

fn worker_list(command: &HooksCommand) -> u8 {
    let cat = worker_catalog();
    if command.json {
        let out = json!({"workers": cat.iter().map(|(t, d, s)| json!({"type": t, "description": d, "status": s})).collect::<Vec<_>>()});
        println!("{}", serde_json::to_string_pretty(&out).unwrap_or_default());
        return 0;
    }
    println!("\nBackground Workers ({})", cat.len());
    println!("{}", "\u{2500}".repeat(50));
    println!("  {:<14} {:<10} {}", "Type", "Status", "Description");
    println!("  {} {} {}", "\u{2500}".repeat(14), "\u{2500}".repeat(10), "\u{2500}".repeat(36));
    for (t, d, s) in cat {
        println!("  {:<14} {:<10} {d}", t, s);
    }
    0
}

fn worker_op(op: &str, command: &HooksCommand) -> u8 {
    let trigger = command.task.as_deref().or(command.positional.first().map(|s| s.as_str())).unwrap_or("audit");
    let valid = worker_catalog().iter().any(|(t, _, _)| *t == trigger);
    if !valid {
        eprintln!("[ERROR] Unknown worker trigger: {trigger}");
        return 1;
    }
    if op == "worker-cancel" {
        println!("Cancel requested for worker '{trigger}' (no live worker in native build).");
        return 0;
    }
    eprintln!("[WARN] {op} for '{trigger}' requires the background daemon (Node). Native build");
    eprintln!("       records intent only. Run: npx ruflo hooks {op} -t {trigger}");
    1
}

// ---- intelligence -----------------------------------------------------------

fn intelligence(root: &Path, command: &HooksCommand) -> u8 {
    let stats_path = root.join(".claude-flow/neural/stats.json");
    let stats = read_json(&stats_path);
    let patterns = stats["trainingRuns"].as_array().map(|a| a.len()).unwrap_or(0);
    if command.json {
        let out = json!({
            "patternsLearned": patterns,
            "modelLoaded": false,
            "sonaEnabled": false,
            "hnswIndex": "not built",
        });
        println!("{}", serde_json::to_string_pretty(&out).unwrap_or_default());
        return 0;
    }
    println!("\nNeural Intelligence Stats");
    println!("{}", "\u{2500}".repeat(50));
    println!("  Patterns learned (recorded): {patterns}");
    println!("  SONA: inactive (needs WASM)");
    println!("  HNSW: not built (needs ONNX store)");
    println!("  Intelligence learning runs in the Node daemon; native reports recorded state.");
    0
}

// ---- statusline -------------------------------------------------------------

fn statusline(root: &Path, command: &HooksCommand) -> u8 {
    // Render a minimal statusline from persisted state. Claude Code pipes session
    // JSON on stdin for the real statusline; native renders a static line from
    // repo state.
    let hive = read_json(&root.join(".claude-flow/hive-mind.json"));
    let workers = hive["workers"].as_array().map(|w| w.len()).unwrap_or(0);
    let budget_paused = read_json(&home_budget())
        ["pausedUntil"]
        .as_u64()
        .map(|t| t > now_ms())
        .unwrap_or(false);
    let line = format!(
        "ruflo \u{2502} hive:{} \u{2502} budget:{}",
        if workers > 0 { workers.to_string() } else { "-".into() },
        if budget_paused { "paused" } else { "ok" }
    );
    if command.json {
        println!("{}", json!({"statusline": line}));
    } else {
        println!("{line}");
    }
    0
}

fn home_budget() -> PathBuf {
    if let Ok(d) = std::env::var("RUFLO_AI_BUDGET_DIR") {
        return PathBuf::from(d).join("ai-budget.json");
    }
    std::env::var("HOME")
        .map(|h| PathBuf::from(h).join(".claude-flow/ai-budget.json"))
        .unwrap_or_else(|_| PathBuf::from(".claude-flow/ai-budget.json"))
}

// ---- notify -----------------------------------------------------------------

fn notify(command: &HooksCommand) -> u8 {
    let msg = command.description.clone().unwrap_or_else(|| "notification".into());
    let target = command.agent.clone().unwrap_or_else(|| "all".into());
    let rec = json!({"event": "notify", "target": target, "message": msg, "at": now_ms()});
    if command.json {
        println!("{}", rec);
    } else {
        println!("notify [{target}]: {msg}");
    }
    0
}

// ---- build-agents -----------------------------------------------------------

fn build_agents(command: &HooksCommand) -> u8 {
    let focus = command.task.clone().unwrap_or_else(|| "all".into());
    eprintln!("[WARN] build-agents generates agent configs via the Node runtime (pretrained model).");
    eprintln!("       Focus: {focus}. Run: npx ruflo hooks build-agents -t {focus}");
    1
}

// ---- degrade ---------------------------------------------------------------

fn degrade(op: &str, command: &HooksCommand) -> u8 {
    let _ = command;
    eprintln!("[WARN] hooks {op} requires the SONA/EWC learning runtime (Node daemon).");
    eprintln!("       Native build cannot run it. Use: npx ruflo hooks {op}");
    1
}

#[cfg(test)]
mod tests {
    use super::*;

    fn tmp() -> PathBuf {
        tempfile::tempdir().unwrap().into_path()
    }

    fn base(op: &str) -> HooksCommand {
        HooksCommand {
            operation: op.into(), task: None, description: None, file_path: None,
            command: None, agent: None, task_id: None, model: None, outcome: None,
            enabled: false, hook_type: None, json: false, verbose: false, positional: vec![],
        }
    }

    #[test]
    fn event_recording_persists() {
        let root = tmp();
        let mut e = base("pre-edit");
        e.file_path = Some("src/x.rs".into());
        assert_eq!(run(&root, e), 0);
        let events = read_jsonl(&events_file(&root));
        assert_eq!(events.len(), 1);
        assert_eq!(events[0]["event"], "pre-edit");
        assert_eq!(events[0]["filePath"], "src/x.rs");
    }

    #[test]
    fn metrics_aggregates_events() {
        let root = tmp();
        run(&root, base("post-edit"));
        run(&root, base("post-edit"));
        run(&root, base("pre-command"));
        // metrics returns 0; verify via direct read.
        let by = read_jsonl(&events_file(&root));
        let post_edit = by.iter().filter(|e| e["event"] == "post-edit").count();
        assert_eq!(post_edit, 2);
    }

    #[test]
    fn route_picks_agent_by_keyword() {
        let root = tmp();
        let mut r = base("route");
        r.task = Some("write a unit test for auth".into());
        assert_eq!(run(&root, r), 0);
        let dec = read_jsonl(&decisions_file(&root));
        assert_eq!(dec[0]["agent"], "tester");
    }

    #[test]
    fn route_requires_task() {
        let root = tmp();
        assert_eq!(run(&root, base("route")), 1);
    }

    #[test]
    fn model_outcome_validated_and_counted() {
        let root = tmp();
        let mut bad = base("model-outcome");
        bad.model = Some("sonnet".into());
        bad.outcome = Some("bogus".into());
        assert_eq!(run(&root, bad), 1);
        let mut good = base("model-outcome");
        good.model = Some("sonnet".into());
        good.outcome = Some("success".into());
        assert_eq!(run(&root, good.clone()), 0);
        assert_eq!(run(&root, good), 0);
        let st = read_json(&model_state_file(&root));
        assert_eq!(st["sonnet.success"], 2);
    }

    #[test]
    fn worker_op_validates_trigger() {
        let root = tmp();
        let mut w = base("worker-dispatch");
        w.task = Some("bogus".into());
        assert_eq!(run(&root, w), 1);
        let mut ok = base("worker-dispatch");
        ok.task = Some("audit".into());
        assert_eq!(run(&root, ok), 1); // degrades (no daemon) but exit 1
    }

    #[test]
    fn list_filters_by_type() {
        let root = tmp();
        let mut l = base("list");
        l.hook_type = Some("SessionStart".into());
        assert_eq!(run(&root, l), 0);
    }
}
