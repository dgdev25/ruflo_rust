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
            redactions: vec!["environment omitted".to_string()],
        }),
        "tests/fixtures/mcp/tools-list.json" => Err(
            "tools-list.json is a reduced-schema fixture and must be curated manually with provenance metadata"
                .to_string(),
        ),
        _ => Err(format!("unapproved fixture path: {fixture_path}")),
    }
}

fn reject_if_sensitive(text: &str) -> Result<(), String> {
    for marker in [
        "/home/",
        "/Users/",
        ":\\\\Users\\\\",
        "ghp_",
        "sk-",
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

    Ok(())
}
