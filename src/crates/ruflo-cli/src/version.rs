//! Native V3 `version` command — ANV (Agent-Native Versioning) Phase 1.
//!
//! Source of truth: `v3/@claude-flow/cli/src/commands/version.ts`. `ruflo --version`
//! stays bare semver; the `version` subcommand adds `--explain` (ANV catalog
//! breakdown) and `--require-catalog-gte` (script capability gating). The advisory
//! suffix is build metadata and never load-bearing for npm/semver precedence.

use std::path::Path;

use serde::Deserialize;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct VersionCommand {
    pub explain: bool,
    pub require_catalog_gte: Option<u64>,
}

#[derive(Debug, Clone, Deserialize)]
pub struct CatalogManifest {
    #[serde(default, rename = "schemaVersion")]
    #[allow(dead_code)]
    pub schema_version: Option<u64>,
    pub generation: u64,
    #[serde(default, rename = "generatedAt")]
    #[allow(dead_code)]
    pub generated_at: Option<String>,
    #[serde(default, rename = "gitSha")]
    pub git_sha: String,
    pub catalog: CatalogCounts,
    pub benchmark: Option<Benchmark>,
}

#[derive(Debug, Clone, Deserialize)]
pub struct CatalogCounts {
    #[serde(default)]
    pub agents: u64,
    #[serde(default)]
    pub tools: u64,
    #[serde(default)]
    pub skills: u64,
}

#[derive(Debug, Clone, Deserialize)]
pub struct Benchmark {
    pub tier: u64,
    #[serde(default, rename = "verifiedAt")]
    #[allow(dead_code)]
    pub verified_at: String,
    #[serde(default)]
    #[allow(dead_code)]
    pub signature: Option<String>,
}

const VERSION: &str = env!("CARGO_PKG_VERSION");

pub fn run(command: VersionCommand) -> u8 {
    let manifest = find_catalog_manifest();
    if let Some(required) = command.require_catalog_gte {
        let generation = manifest.as_ref().map(|m| m.generation).unwrap_or(0);
        if generation >= required {
            println!("OK (installed catalog is {generation})");
            return 0;
        }
        eprintln!("[ERROR] Installed catalog generation {generation} is below required {required}");
        return 1;
    }

    if !command.explain {
        println!("{VERSION}");
        return 0;
    }

    let Some(manifest) = manifest else {
        println!("Installed: ruflo@{VERSION}");
        println!(
            "\x1b[2m  (no catalog-manifest.json — plain semver, pre-ANV or dev checkout)\x1b[0m"
        );
        return 0;
    };

    let suffix = build_advisory_suffix(&manifest, 1);
    println!("Installed: \x1b[1mruflo@{VERSION}{suffix}\x1b[0m");
    println!();
    println!("Era:       AD (Agent Descent) — 1st generation");
    println!(
        "Catalog:   generation {} (agents: {} types, tools: {} MCP, skills: {})",
        manifest.generation,
        manifest.catalog.agents,
        manifest.catalog.tools,
        manifest.catalog.skills
    );
    if let Some(benchmark) = &manifest.benchmark {
        let verified_at = benchmark.verified_at.chars().take(10).collect::<String>();
        println!(
            "Benchmark: GAIA tier {} (verified {verified_at}, signed)",
            benchmark.tier
        );
    } else {
        println!(
            "\x1b[2mBenchmark: not yet submitted (no verified GAIA/HAL score for this catalog generation)\x1b[0m"
        );
    }
    0
}

/// `+ad.<release>.g<gitSha>.cat<generation>[.hal<tier>]` (semver §10 build metadata).
pub fn build_advisory_suffix(manifest: &CatalogManifest, release_sequence: u64) -> String {
    let mut parts = vec![
        format!("ad.{release_sequence}"),
        format!("g{}", manifest.git_sha),
        format!("cat{}", manifest.generation),
    ];
    if let Some(benchmark) = &manifest.benchmark {
        parts.push(format!("hal{}", benchmark.tier));
    }
    format!("+{}", parts.join("."))
}

/// Locate `catalog-manifest.json`. The TS source resolves it next to the installed
/// package root, with `__dirname`-relative dev fallbacks. The native binary has no
/// package-root resolver, so we walk the executable's parent chain (installed
/// layout) and finally the process working directory (dev checkout parity). A
/// corrupt manifest is treated as absent, matching version.ts:51.
pub fn find_catalog_manifest() -> Option<CatalogManifest> {
    let mut candidates = Vec::new();
    if let Ok(exe) = std::env::current_exe() {
        if let Some(mut dir) = exe.parent().map(Path::to_path_buf) {
            candidates.push(dir.join("catalog-manifest.json"));
            for _ in 0..4 {
                if let Some(parent) = dir.parent() {
                    dir = parent.to_path_buf();
                    candidates.push(dir.join("catalog-manifest.json"));
                }
            }
        }
    }
    if let Ok(cwd) = std::env::current_dir() {
        candidates.push(cwd.join("catalog-manifest.json"));
    }

    for candidate in candidates {
        if !candidate.is_file() {
            continue;
        }
        let Ok(contents) = std::fs::read_to_string(&candidate) else {
            continue;
        };
        if let Ok(manifest) = serde_json::from_str::<CatalogManifest>(&contents) {
            return Some(manifest);
        }
        // corrupt manifest — treat as absent (version.ts:51).
    }
    None
}

#[cfg(test)]
mod tests {
    use super::*;

    fn sample_manifest(benchmark: bool) -> CatalogManifest {
        CatalogManifest {
            schema_version: Some(1),
            generation: 42,
            generated_at: Some("2026-08-01T00:00:00Z".into()),
            git_sha: "abc1234".into(),
            catalog: CatalogCounts {
                agents: 12,
                tools: 34,
                skills: 56,
            },
            benchmark: benchmark.then(|| Benchmark {
                tier: 3,
                verified_at: "2026-08-02T00:00:00Z".into(),
                signature: Some("sig".into()),
            }),
        }
    }

    #[test]
    fn suffix_includes_benchmark_tier_only_when_present() {
        let with_bench = sample_manifest(true);
        assert_eq!(
            build_advisory_suffix(&with_bench, 1),
            "+ad.1.gabc1234.cat42.hal3"
        );
        let no_bench = sample_manifest(false);
        assert_eq!(build_advisory_suffix(&no_bench, 1), "+ad.1.gabc1234.cat42");
    }

    #[test]
    fn require_catalog_gte_passes_when_generation_meets() {
        // No manifest in test cwd → generation 0. Require 0 passes.
        let code = run(VersionCommand {
            explain: false,
            require_catalog_gte: Some(0),
        });
        assert_eq!(code, 0);
    }

    #[test]
    fn require_catalog_gte_fails_when_generation_below() {
        let code = run(VersionCommand {
            explain: false,
            require_catalog_gte: Some(99),
        });
        assert_eq!(code, 1);
    }

    #[test]
    fn bare_version_prints_plain_semver() {
        // No explain, no gate → bare version string only.
        // (Output asserted via E2E; here just ensure exit 0.)
        let code = run(VersionCommand {
            explain: false,
            require_catalog_gte: None,
        });
        assert_eq!(code, 0);
    }

    #[test]
    fn manifest_parse_requires_generation_and_catalog() {
        // Missing generation → not a valid manifest → None.
        let bad = r#"{"gitSha":"x","catalog":{"agents":1}}"#;
        assert!(serde_json::from_str::<CatalogManifest>(bad).is_err());
        let good = r#"{"generation":5,"gitSha":"x","catalog":{"agents":1,"tools":2,"skills":3}}"#;
        assert!(serde_json::from_str::<CatalogManifest>(good).is_ok());
    }
}
