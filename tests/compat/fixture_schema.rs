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
}

#[test]
fn checked_in_json_rpc_fixture_parses() {
    let tools = JsonRpcFixture::load("tests/fixtures/mcp/tools-list.json").unwrap();
    assert_eq!(tools.request["method"], "tools/list");
    assert!(tools.response["result"]["tools"].is_array());
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
