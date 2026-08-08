use std::collections::BTreeMap;
use std::env;
use std::fs;
use std::path::Path;
use std::process::ExitCode;

use serde::Serialize;

#[derive(Debug, Serialize)]
struct CliFixture {
    argv: Vec<String>,
    exit: i32,
    stdout: String,
    stderr: String,
    #[serde(skip_serializing_if = "BTreeMap::is_empty")]
    environment: BTreeMap<String, String>,
    platform: FixturePlatform,
    provenance: FixtureProvenance,
    recording: FixtureRecording,
}

#[derive(Debug, Serialize)]
struct FixturePlatform {
    family: &'static str,
}

#[derive(Debug, Serialize)]
struct FixtureProvenance {
    kind: &'static str,
    source: &'static str,
    source_command: Vec<String>,
    #[serde(skip_serializing_if = "Vec::is_empty")]
    source_paths: Vec<String>,
    #[serde(skip_serializing_if = "Vec::is_empty")]
    redactions: Vec<String>,
}

#[derive(Debug, Serialize)]
struct FixtureRecording {
    recorded_at: String,
    recorded_by: &'static str,
    harness: &'static str,
}

fn main() -> ExitCode {
    match run() {
        Ok(()) => ExitCode::SUCCESS,
        Err(error) => {
            eprintln!("{error}");
            ExitCode::from(1)
        }
    }
}

fn run() -> Result<(), String> {
    let mut args = env::args().skip(1).collect::<Vec<_>>();
    if args.len() < 5 {
        return Err(
            "usage: fixture-capture <fixture-path> <stdout-path> <stderr-path> <exit-code> <recorded-at> [argv ...]"
                .to_string(),
        );
    }

    let fixture_path = args.remove(0);
    let stdout_path = args.remove(0);
    let stderr_path = args.remove(0);
    let exit_code = args
        .remove(0)
        .parse::<i32>()
        .map_err(|error| format!("invalid exit code: {error}"))?;
    let recorded_at = args.remove(0);
    let argv = args;

    let metadata = metadata_for(&fixture_path, &argv)?;
    let stdout = fs::read_to_string(&stdout_path)
        .map_err(|error| format!("failed to read stdout capture `{stdout_path}`: {error}"))?;
    let stderr = fs::read_to_string(&stderr_path)
        .map_err(|error| format!("failed to read stderr capture `{stderr_path}`: {error}"))?;

    reject_if_sensitive(&stdout)?;
    reject_if_sensitive(&stderr)?;

    let fixture = CliFixture {
        argv,
        exit: exit_code,
        stdout,
        stderr,
        environment: BTreeMap::new(),
        platform: FixturePlatform { family: "portable" },
        provenance: metadata,
        recording: FixtureRecording {
            recorded_at,
            recorded_by: "scripts/capture-reference-contract.sh",
            harness: "fixture-capture",
        },
    };

    let json = serde_json::to_string_pretty(&fixture)
        .map_err(|error| format!("failed to encode fixture JSON: {error}"))?;
    fs::write(Path::new(&fixture_path), format!("{json}\n"))
        .map_err(|error| format!("failed to write fixture `{fixture_path}`: {error}"))?;
    Ok(())
}

fn metadata_for(fixture_path: &str, argv: &[String]) -> Result<FixtureProvenance, String> {
    match fixture_path {
        "tests/fixtures/cli/version.json" | "tests/fixtures/cli/help.json" => Ok(FixtureProvenance {
            kind: "source-oracle",
            source: "managed:ruflo-cli",
            source_command: argv.to_vec(),
            source_paths: Vec::new(),
            redactions: vec!["environment omitted".to_string()],
        }),
        "tests/fixtures/mcp/tools-list.json" => Err(
            "tools-list.json is a reduced-schema fixture and must be curated manually with provenance metadata"
                .to_string(),
        ),
        _ => {
            // Per-family source-oracle fixtures captured from the TS reference
            // CLI. Approve any tests/fixtures/cli/<family>/*.json and stamp the
            // owning TS source path so the fixture is source-proven.
            let ts_source = family_ts_source(fixture_path)
                .ok_or_else(|| format!("unapproved fixture path: {fixture_path}"))?;
            Ok(FixtureProvenance {
                kind: "source-oracle",
                source: "managed:ruflo-cli@3.34.0",
                source_command: argv.to_vec(),
                source_paths: vec![ts_source.to_string()],
                redactions: vec!["environment omitted".to_string()],
            })
        }
    }
}

/// Map a per-family fixture path to its owning TS source file. Only families
/// with a real TS oracle are approved.
fn family_ts_source(fixture_path: &str) -> Option<&'static str> {
    let family = fixture_path
        .strip_prefix("tests/fixtures/cli/")?
        .split('/')
        .next()?;
    let sources: &[(&str, &str)] = &[
        ("cleanup", "v3/@claude-flow/cli/src/commands/cleanup.ts"),
        ("config", "v3/@claude-flow/cli/src/commands/config.ts"),
        ("deployment", "v3/@claude-flow/cli/src/commands/deployment.ts"),
        ("transport", "v3/@claude-flow/cli/src/commands/transport.ts"),
        ("migrate", "v3/@claude-flow/cli/src/commands/migrate.ts"),
        ("security", "v3/@claude-flow/cli/src/commands/security.ts"),
        ("analyze", "v3/@claude-flow/cli/src/commands/analyze.ts"),
        ("daemon", "v3/@claude-flow/cli/src/commands/daemon.ts"),
        ("embeddings", "v3/@claude-flow/cli/src/commands/embeddings.ts"),
        ("hive-mind", "v3/@claude-flow/cli/src/commands/hive-mind.ts"),
        ("neural", "v3/@claude-flow/cli/src/commands/neural.ts"),
        ("hooks", "v3/@claude-flow/cli/src/commands/hooks.ts"),
        ("claims", "v3/@claude-flow/cli/src/commands/claims.ts"),
        ("issues", "v3/@claude-flow/cli/src/commands/issues.ts"),
        ("completions", "v3/@claude-flow/cli/src/commands/completions.ts"),
        ("version", "v3/@claude-flow/cli/src/commands/version.ts"),
        ("init", "v3/@claude-flow/cli/src/commands/init.ts"),
        ("start", "v3/@claude-flow/cli/src/commands/start.ts"),
        ("status", "v3/@claude-flow/cli/src/commands/status.ts"),
        ("agent", "v3/@claude-flow/cli/src/commands/agent.ts"),
        ("swarm", "v3/@claude-flow/cli/src/commands/swarm.ts"),
        ("task", "v3/@claude-flow/cli/src/commands/task.ts"),
        ("session", "v3/@claude-flow/cli/src/commands/session.ts"),
        ("memory", "v3/@claude-flow/cli/src/commands/memory.ts"),
        ("mcp", "v3/@claude-flow/cli/src/commands/mcp.ts"),
        ("workflow", "v3/@claude-flow/cli/src/commands/workflow.ts"),
        ("process", "v3/@claude-flow/cli/src/commands/process.ts"),
        ("doctor", "v3/@claude-flow/cli/src/commands/doctor.ts"),
        ("performance", "v3/@claude-flow/cli/src/commands/performance.ts"),
        ("policy", "v3/@claude-flow/cli/src/commands/policy.ts"),
        ("verify", "v3/@claude-flow/cli/src/commands/verify.ts"),
        ("route", "v3/@claude-flow/cli/src/commands/route.ts"),
        ("progress", "v3/@claude-flow/cli/src/commands/progress.ts"),
        ("providers", "v3/@claude-flow/cli/src/commands/providers.ts"),
        ("plugins", "v3/@claude-flow/cli/src/commands/plugins.ts"),
        ("update", "v3/@claude-flow/cli/src/commands/update.ts"),
        ("ruvector", "v3/@claude-flow/cli/src/commands/ruvector.ts"),
        ("guidance", "v3/@claude-flow/cli/src/commands/guidance.ts"),
        ("appliance", "v3/@claude-flow/cli/src/commands/appliance.ts"),
        ("appliance-advanced", "v3/@claude-flow/cli/src/commands/appliance-advanced.ts"),
        ("transfer-store", "v3/@claude-flow/cli/src/commands/transfer-store.ts"),
        ("autopilot", "v3/@claude-flow/cli/src/commands/autopilot.ts"),
        ("benchmark", "v3/@claude-flow/cli/src/commands/benchmark.ts"),
        ("gaia-bench", "v3/@claude-flow/cli/src/commands/gaia-bench.ts"),
        ("metaharness", "v3/@claude-flow/cli/src/commands/metaharness.ts"),
        ("eject", "v3/@claude-flow/cli/src/commands/eject.ts"),
        ("funnel", "v3/@claude-flow/cli/src/commands/funnel.ts"),
        ("settings", "v3/@claude-flow/cli/src/commands/settings.ts"),
        ("auth", "v3/@claude-flow/cli/src/commands/auth.ts"),
        ("proxy", "v3/@claude-flow/cli/src/commands/proxy.ts"),
        ("advisor", "v3/@claude-flow/cli/src/commands/advisor.ts"),
        ("spinner", "v3/@claude-flow/cli/src/commands/spinner.ts"),
        ("announcements", "v3/@claude-flow/cli/src/commands/announcements.ts"),
    ];
    sources
        .iter()
        .find(|(f, _)| *f == family)
        .map(|(_, src)| *src)
}

fn reject_if_sensitive(text: &str) -> Result<(), String> {
    for marker in [
        "/home/",
        "/Users/",
        ":\\\\Users\\\\",
        "ghp_",
        "AIza",
        "PRIVATE KEY-----",
        "xoxb-",
        "xoxp-",
        "xoxa-",
        "xoxr-",
        "xoxs-",
    ] {
        if text.contains(marker) {
            return Err(format!(
                "captured output contains a forbidden path or secret marker: {marker}"
            ));
        }
    }
    // `sk-` is checked with a word boundary so legitimate words like
    // "task-completed" (which contain the letters s-k-) don't trip it. A real
    // Stripe/OpenAI key appears as a standalone token: quote/space/start + sk-.
    for (idx, _) in text.match_indices("sk-") {
        let prev = text[..idx].chars().next_back();
        let boundary = match prev {
            None => true,
            Some(c) => !c.is_alphanumeric(),
        };
        if boundary {
            return Err(
                "captured output contains a forbidden path or secret marker: sk-".to_string(),
            );
        }
    }
    Ok(())
}
