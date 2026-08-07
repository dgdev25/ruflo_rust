use std::collections::BTreeMap;
use std::fs;
use std::path::Path;

use serde::{Deserialize, Serialize};
use serde_json::Value;

pub type Fixture = CliFixture;

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct CliFixture {
    pub argv: Vec<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub stdin: Option<String>,
    pub exit: i32,
    pub stdout: String,
    pub stderr: String,
    #[serde(default, skip_serializing_if = "BTreeMap::is_empty")]
    pub environment: BTreeMap<String, String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub platform: Option<FixturePlatform>,
    pub provenance: FixtureProvenance,
    pub recording: FixtureRecording,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct FixturePlatform {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub os: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub arch: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub family: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct JsonRpcFixture {
    pub request: Value,
    pub response: Value,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub platform: Option<FixturePlatform>,
    pub provenance: FixtureProvenance,
    pub recording: FixtureRecording,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum FixtureKind {
    SourceOracle,
    ReducedSchema,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct FixtureProvenance {
    pub kind: FixtureKind,
    pub source: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub source_command: Option<Vec<String>>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub reduction: Option<String>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub redactions: Vec<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct FixtureRecording {
    pub recorded_at: String,
    pub recorded_by: String,
    pub harness: String,
}

#[derive(Debug)]
pub enum FixtureParseError {
    Json(serde_json::Error),
    Validation(String),
}

impl CliFixture {
    pub fn parse(json: &str) -> Result<Self, FixtureParseError> {
        let fixture: Self = serde_json::from_str(json).map_err(FixtureParseError::Json)?;
        fixture.validate()?;
        Ok(fixture)
    }

    pub fn load(path: impl AsRef<Path>) -> Result<Self, FixtureLoadError> {
        let path = path.as_ref();
        let raw = fs::read_to_string(path).map_err(|source| FixtureLoadError::Io {
            path: path.display().to_string(),
            source,
        })?;
        Self::parse(&raw).map_err(|source| FixtureLoadError::Parse {
            path: path.display().to_string(),
            source,
        })
    }

    fn validate(&self) -> Result<(), FixtureParseError> {
        self.provenance.validate(&self.argv)?;
        self.recording.validate()
    }
}

impl JsonRpcFixture {
    pub fn parse(json: &str) -> Result<Self, FixtureParseError> {
        let fixture: Self = serde_json::from_str(json).map_err(FixtureParseError::Json)?;
        fixture.validate()?;
        Ok(fixture)
    }

    pub fn load(path: impl AsRef<Path>) -> Result<Self, FixtureLoadError> {
        let path = path.as_ref();
        let raw = fs::read_to_string(path).map_err(|source| FixtureLoadError::Io {
            path: path.display().to_string(),
            source,
        })?;
        Self::parse(&raw).map_err(|source| FixtureLoadError::Parse {
            path: path.display().to_string(),
            source,
        })
    }

    fn validate(&self) -> Result<(), FixtureParseError> {
        self.provenance.validate(&[])?;
        self.recording.validate()
    }
}

#[derive(Debug)]
pub enum FixtureLoadError {
    Io {
        path: String,
        source: std::io::Error,
    },
    Parse {
        path: String,
        source: FixtureParseError,
    },
}

impl std::fmt::Display for FixtureLoadError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Io { path, source } => write!(f, "failed to read fixture `{path}`: {source}"),
            Self::Parse { path, source } => {
                write!(f, "failed to parse fixture `{path}`: {source}")
            }
        }
    }
}

impl std::error::Error for FixtureLoadError {}

impl FixtureProvenance {
    fn validate(&self, argv: &[String]) -> Result<(), FixtureParseError> {
        if self.source.trim().is_empty() {
            return Err(FixtureParseError::Validation(
                "fixture provenance.source must not be empty".to_string(),
            ));
        }

        match self.kind {
            FixtureKind::SourceOracle => {
                let source_command = self.source_command.as_ref().ok_or_else(|| {
                    FixtureParseError::Validation(
                        "source-oracle fixtures must declare provenance.source_command".to_string(),
                    )
                })?;
                if argv != source_command {
                    return Err(FixtureParseError::Validation(
                        "source-oracle fixture argv must match provenance.source_command"
                            .to_string(),
                    ));
                }
                if self.reduction.is_some() {
                    return Err(FixtureParseError::Validation(
                        "source-oracle fixtures must not declare provenance.reduction".to_string(),
                    ));
                }
            }
            FixtureKind::ReducedSchema => {
                if self.source_command.is_some() {
                    return Err(FixtureParseError::Validation(
                        "reduced-schema fixtures must not declare provenance.source_command"
                            .to_string(),
                    ));
                }
                let reduction = self.reduction.as_ref().ok_or_else(|| {
                    FixtureParseError::Validation(
                        "reduced-schema fixtures must explain provenance.reduction".to_string(),
                    )
                })?;
                if reduction.trim().is_empty() {
                    return Err(FixtureParseError::Validation(
                        "reduced-schema fixture provenance.reduction must not be empty".to_string(),
                    ));
                }
            }
        }

        Ok(())
    }
}

impl FixtureRecording {
    fn validate(&self) -> Result<(), FixtureParseError> {
        for (name, value) in [
            ("recorded_at", &self.recorded_at),
            ("recorded_by", &self.recorded_by),
            ("harness", &self.harness),
        ] {
            if value.trim().is_empty() {
                return Err(FixtureParseError::Validation(format!(
                    "fixture recording.{name} must not be empty"
                )));
            }
        }

        Ok(())
    }
}

impl std::fmt::Display for FixtureParseError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Json(source) => write!(f, "{source}"),
            Self::Validation(message) => write!(f, "{message}"),
        }
    }
}

impl std::error::Error for FixtureParseError {}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ContractMatrix {
    rows: Vec<ContractMatrixRow>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ContractMatrixRow {
    pub priority: String,
    pub consumer: String,
    pub invocation: String,
    pub contract: String,
    pub fixture: Option<String>,
    pub blocker: Option<String>,
    pub wave: String,
    pub status: String,
    pub owner: String,
    pub evidence: Vec<String>,
}

#[derive(Debug)]
pub enum ContractMatrixLoadError {
    Io {
        path: String,
        source: std::io::Error,
    },
    Validation(String),
}

impl ContractMatrix {
    pub fn load(path: impl AsRef<Path>) -> Result<Self, ContractMatrixLoadError> {
        let path = path.as_ref();
        let raw = fs::read_to_string(path).map_err(|source| ContractMatrixLoadError::Io {
            path: path.display().to_string(),
            source,
        })?;

        let mut headers: Option<Vec<String>> = None;
        let mut rows = Vec::new();

        for line in raw.lines() {
            let trimmed = line.trim();
            if !trimmed.starts_with('|') {
                if headers.is_some() && !rows.is_empty() {
                    break;
                }
                continue;
            }

            let cells = parse_markdown_row(trimmed);
            if cells.is_empty() {
                continue;
            }

            if headers.is_none() {
                if cells
                    .iter()
                    .any(|cell| cell.eq_ignore_ascii_case("priority"))
                {
                    headers = Some(cells);
                }
                continue;
            }

            if is_separator_row(&cells) {
                continue;
            }

            let header = headers.as_ref().expect("header checked above");
            if cells.len() != header.len() {
                return Err(ContractMatrixLoadError::Validation(format!(
                    "matrix row has {} columns but header has {}",
                    cells.len(),
                    header.len()
                )));
            }

            rows.push(ContractMatrixRow::from_cells(header, &cells)?);
        }

        if rows.is_empty() {
            return Err(ContractMatrixLoadError::Validation(
                "no contract matrix rows found".to_string(),
            ));
        }

        Ok(Self { rows })
    }

    pub fn p0_rows(&self) -> impl Iterator<Item = &ContractMatrixRow> {
        self.rows.iter().filter(|row| row.priority == "P0")
    }
}

impl ContractMatrixRow {
    fn from_cells(headers: &[String], cells: &[String]) -> Result<Self, ContractMatrixLoadError> {
        let get = |name: &str| -> Result<String, ContractMatrixLoadError> {
            let index = headers
                .iter()
                .position(|header| header.eq_ignore_ascii_case(name))
                .ok_or_else(|| {
                    ContractMatrixLoadError::Validation(format!(
                        "missing required matrix column `{name}`"
                    ))
                })?;
            Ok(cells[index].clone())
        };

        let priority = get("priority")?;
        let consumer = get("consumer")?;
        let invocation = get("invocation")?;
        let contract = get("contract")?;
        let fixture = normalize_cell(&get("fixture")?);
        let blocker = normalize_cell(&get("blocker")?);
        let wave = get("wave")?;
        let status = get("status")?;
        let owner = get("owner")?;
        let evidence = normalize_cell(&get("evidence")?)
            .map(|value| {
                value
                    .split(';')
                    .map(str::trim)
                    .filter(|path| !path.is_empty())
                    .map(ToOwned::to_owned)
                    .collect::<Vec<_>>()
            })
            .unwrap_or_default();

        for (field, value) in [
            ("priority", &priority),
            ("consumer", &consumer),
            ("invocation", &invocation),
            ("contract", &contract),
            ("wave", &wave),
            ("status", &status),
            ("owner", &owner),
        ] {
            if value.trim().is_empty() || value.trim() == "-" {
                return Err(ContractMatrixLoadError::Validation(format!(
                    "matrix row field `{field}` must not be empty"
                )));
            }
        }

        if evidence.is_empty() {
            return Err(ContractMatrixLoadError::Validation(
                "matrix row must declare at least one evidence path".to_string(),
            ));
        }

        Ok(Self {
            priority,
            consumer,
            invocation,
            contract,
            fixture,
            blocker,
            wave,
            status,
            owner,
            evidence,
        })
    }
}

impl std::fmt::Display for ContractMatrixLoadError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Io { path, source } => write!(f, "failed to read matrix `{path}`: {source}"),
            Self::Validation(message) => write!(f, "{message}"),
        }
    }
}

impl std::error::Error for ContractMatrixLoadError {}

fn parse_markdown_row(line: &str) -> Vec<String> {
    line.trim_matches('|')
        .split('|')
        .map(|cell| cell.trim().to_string())
        .collect()
}

fn is_separator_row(cells: &[String]) -> bool {
    cells.iter().all(|cell| {
        let normalized = cell.replace(['-', ':'], "");
        normalized.trim().is_empty()
    })
}

fn normalize_cell(cell: &str) -> Option<String> {
    let trimmed = cell.trim();
    if trimmed.is_empty() || trimmed == "-" {
        None
    } else {
        Some(trimmed.to_string())
    }
}

#[test]
fn cli_fixture_requires_exit_stdout_and_stderr() {
    let parsed = Fixture::parse(
        r#"{
          "argv":["--version"],
          "exit":0,
          "stdout":"ruflo vX\n",
          "stderr":"",
          "environment":{},
          "platform":{"family":"portable"},
          "provenance":{"kind":"source-oracle","source":"managed:ruflo-cli","source_command":["--version"]},
          "recording":{"recorded_at":"2026-08-07T00:00:00Z","recorded_by":"test","harness":"fixture-schema"}
        }"#,
    )
    .unwrap();
    assert_eq!(parsed.exit, 0);
    assert_eq!(parsed.stdout, "ruflo vX\n");
    assert_eq!(parsed.stderr, "");
}

#[test]
fn checked_in_cli_fixtures_parse() {
    let version = Fixture::load("tests/fixtures/cli/version.json").unwrap();
    assert_eq!(version.argv, vec!["--version"]);
    assert_eq!(version.exit, 0);

    let help = Fixture::load("tests/fixtures/cli/help.json").unwrap();
    assert_eq!(help.argv, vec!["--quiet", "--help"]);
    assert!(help
        .stdout
        .contains("Ruflo - AI Agent Orchestration Platform"));

    for path in [
        "tests/fixtures/codex/version.json",
        "tests/fixtures/codex/dual-templates.json",
        "tests/fixtures/codex/dual-run-empty.json",
        "tests/fixtures/codex/dual-run-help.json",
    ] {
        Fixture::load(path).unwrap();
    }
}

#[test]
fn checked_in_json_rpc_fixture_parses() {
    let tools = JsonRpcFixture::load("tests/fixtures/mcp/tools-list.json").unwrap();
    assert_eq!(tools.request["method"], "tools/list");
    assert!(tools.response["result"]["tools"].is_array());

    let memory_search = JsonRpcFixture::load("tests/fixtures/mcp/memory-search-call.json").unwrap();
    assert_eq!(memory_search.request["method"], "tools/call");
    assert_eq!(
        memory_search.response["result"]["structuredContent"]["query"],
        "auth"
    );

    let denied = JsonRpcFixture::load("tests/fixtures/mcp/memory-search-denied.json").unwrap();
    assert_eq!(denied.response["error"]["code"], -32001);
}

#[test]
fn fixture_environment_values_must_be_redacted_when_present() {
    let version = Fixture::load("tests/fixtures/cli/version.json").unwrap();
    assert!(version
        .environment
        .values()
        .all(|value| value == "<redacted>"));
}

#[test]
fn checked_in_fixtures_require_provenance_and_recording_metadata() {
    let version = Fixture::load("tests/fixtures/cli/version.json").unwrap();
    assert_eq!(version.provenance.kind, FixtureKind::SourceOracle);
    assert_eq!(
        version.provenance.source_command.as_deref(),
        Some(version.argv.as_slice())
    );
    assert_eq!(version.recording.harness, "fixture-capture");

    let help = Fixture::load("tests/fixtures/cli/help.json").unwrap();
    assert_eq!(help.provenance.kind, FixtureKind::SourceOracle);
    assert_eq!(
        help.recording.recorded_by,
        "scripts/capture-reference-contract.sh"
    );

    let tools = JsonRpcFixture::load("tests/fixtures/mcp/tools-list.json").unwrap();
    assert_eq!(tools.provenance.kind, FixtureKind::ReducedSchema);
    assert!(tools
        .provenance
        .reduction
        .as_deref()
        .unwrap()
        .contains("minimal synthetic"));
}

#[test]
fn reduced_schema_fixture_must_explain_its_reduction() {
    let error = JsonRpcFixture::parse(
        r#"{
          "request":{"jsonrpc":"2.0","id":"1","method":"tools/list","params":{}},
          "response":{"jsonrpc":"2.0","id":"1","result":{"tools":[]}},
          "provenance":{"kind":"reduced-schema","source":"manual"},
          "recording":{"recorded_at":"2026-08-07T00:00:00Z","recorded_by":"test","harness":"fixture-schema"}
        }"#,
    )
    .unwrap_err();

    assert!(error
        .to_string()
        .contains("reduced-schema fixtures must explain provenance.reduction"));
}

#[test]
fn checked_in_contract_matrix_parses() {
    let matrix = ContractMatrix::load("docs/compatibility/contract-matrix.md").unwrap();
    assert!(matrix.p0_rows().count() >= 8);
}

#[test]
fn every_p0_contract_has_a_consumer_fixture_or_explicit_blocker() {
    let matrix = ContractMatrix::load("docs/compatibility/contract-matrix.md").unwrap();
    assert!(matrix
        .p0_rows()
        .all(|row| row.fixture.is_some() || row.blocker.is_some()));
}

#[test]
fn every_p0_contract_declares_wave_status_owner_and_evidence() {
    let matrix = ContractMatrix::load("docs/compatibility/contract-matrix.md").unwrap();
    assert!(matrix.p0_rows().all(|row| {
        !row.wave.is_empty()
            && !row.status.is_empty()
            && !row.owner.is_empty()
            && !row.evidence.is_empty()
    }));
}
