//! Native V3 `providers` command — AI provider management.
//!
//! Source: `v3/@claude-flow/cli/src/commands/providers.ts`. Subcommands:
//! list/configure/test/models/usage. Provider config in .claude-flow/providers.json.

use std::fs;
use std::path::{Path, PathBuf};

use serde_json::{json, Value};

struct Provider {
    name: &'static str,
    ptype: &'static str,
    models: &'static str,
    env_var: &'static str,
    config_name: &'static str,
}

const PROVIDERS: &[Provider] = &[
    Provider {
        name: "Anthropic",
        ptype: "LLM",
        models: "claude-3.5-sonnet, opus",
        env_var: "ANTHROPIC_API_KEY",
        config_name: "anthropic",
    },
    Provider {
        name: "OpenAI",
        ptype: "LLM",
        models: "gpt-4o, gpt-4-turbo",
        env_var: "OPENAI_API_KEY",
        config_name: "openai",
    },
    Provider {
        name: "OpenAI",
        ptype: "Embedding",
        models: "text-embedding-3-small/large",
        env_var: "OPENAI_API_KEY",
        config_name: "openai",
    },
    Provider {
        name: "Google",
        ptype: "LLM",
        models: "gemini-pro, gemini-ultra",
        env_var: "GOOGLE_API_KEY",
        config_name: "google",
    },
    Provider {
        name: "Ollama",
        ptype: "LLM",
        models: "gpt-oss:120b-cloud, llama3:70b-cloud, qwen2.5-coder:32b-cloud",
        env_var: "OLLAMA_API_KEY",
        config_name: "ollama",
    },
    Provider {
        name: "Transformers.js",
        ptype: "Embedding",
        models: "Xenova/all-MiniLM-L6-v2",
        env_var: "",
        config_name: "transformers",
    },
    Provider {
        name: "Agentic Flow",
        ptype: "Embedding",
        models: "ONNX optimized",
        env_var: "",
        config_name: "agentic-flow",
    },
    Provider {
        name: "Mock",
        ptype: "All",
        models: "mock-*",
        env_var: "",
        config_name: "mock",
    },
];

fn providers_file(root: &Path) -> PathBuf {
    root.join(".claude-flow/providers.json")
}

fn load_config(root: &Path) -> Value {
    fs::read_to_string(providers_file(root))
        .ok()
        .and_then(|r| serde_json::from_str(&r).ok())
        .unwrap_or_else(|| json!({}))
}

fn save_config(root: &Path, config: &Value) -> bool {
    let dir = root.join(".claude-flow");
    let _ = fs::create_dir_all(&dir);
    let path = providers_file(root);
    let tmp = path.with_extension("json.tmp");
    let Ok(bytes) = serde_json::to_vec_pretty(config) else {
        return false;
    };
    fs::write(&tmp, &bytes).is_ok() && fs::rename(&tmp, &path).is_ok()
}

fn is_active(p: &Provider, config: &Value) -> bool {
    if p.env_var.is_empty() {
        return false;
    }
    if std::env::var_os(p.env_var).is_some() {
        return true;
    }
    let key = config
        .get(p.config_name)
        .and_then(|v| v.get("apiKey"))
        .and_then(Value::as_str);
    key.is_some_and(|k| !k.is_empty())
}

fn redact_key(key: &str) -> String {
    if key.len() <= 8 {
        return "***".into();
    }
    format!("{}...{}", &key[..4], &key[key.len() - 4..])
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ProvidersCommand {
    pub operation: String,
    pub provider: Option<String>,
    pub key: Option<String>,
    pub model: Option<String>,
    pub base_url: Option<String>,
    pub filter_type: Option<String>,
    pub active_only: bool,
    pub json: bool,
}

pub fn run(root: &Path, command: ProvidersCommand) -> u8 {
    match command.operation.as_str() {
        "list" | "" => list(root, &command),
        "configure" => configure(root, &command),
        "test" => test_providers(root, &command),
        "models" => models(&command),
        "usage" => usage(root, &command),
        _ => {
            eprintln!(
                "[ERROR] Unknown: {} (list|configure|test|models|usage)",
                command.operation
            );
            1
        }
    }
}

fn list(root: &Path, command: &ProvidersCommand) -> u8 {
    let config = load_config(root);
    let filtered: Vec<&Provider> = PROVIDERS
        .iter()
        .filter(|p| {
            if command.active_only && !is_active(p, &config) {
                return false;
            }
            if let Some(ref t) = command.filter_type {
                return p.ptype.to_lowercase() == t.to_lowercase() || t == "all";
            }
            true
        })
        .collect();
    if command.json {
        let arr: Vec<Value> = filtered.iter().map(|p| json!({
            "name": p.name, "type": p.ptype, "models": p.models,
            "active": is_active(p, &config),
            "key": config.get(p.config_name).and_then(|v| v.get("apiKey")).and_then(Value::as_str).map(redact_key),
        })).collect();
        println!(
            "{}",
            serde_json::to_string_pretty(&json!(arr)).unwrap_or_default()
        );
    } else {
        println!("\nProviders");
        println!("{}", "\u{2500}".repeat(60));
        if filtered.is_empty() {
            println!("  No providers match the current filter.");
        }
        for p in &filtered {
            let active = is_active(p, &config);
            let icon = if active { "\u{2714}" } else { "\u{2718}" };
            println!("  {icon} {} ({}) \u{2014} {}", p.name, p.ptype, p.models);
            if active {
                if let Some(key) = config
                    .get(p.config_name)
                    .and_then(|v| v.get("apiKey"))
                    .and_then(Value::as_str)
                {
                    println!("     key: {}", redact_key(key));
                }
            }
        }
        println!("\nTip: Use \"providers configure -p <name> -k <key>\" to set API keys.");
    }
    0
}

fn configure(root: &Path, command: &ProvidersCommand) -> u8 {
    let Some(provider) = &command.provider else {
        eprintln!("[ERROR] --provider is required");
        return 1;
    };
    let mut config = load_config(root);
    if !config.is_object() {
        config = json!({});
    }
    if let Some(obj) = config.as_object_mut() {
        let mut entry = obj.get(provider.as_str()).cloned().unwrap_or(json!({}));
        if let Some(entry_obj) = entry.as_object_mut() {
            if let Some(key) = &command.key {
                entry_obj.insert("apiKey".into(), json!(key));
            }
            if let Some(model) = &command.model {
                entry_obj.insert("model".into(), json!(model));
            }
            if let Some(url) = &command.base_url {
                entry_obj.insert("baseUrl".into(), json!(url));
            }
        }
        obj.insert(provider.clone(), entry);
    }
    if !save_config(root, &config) {
        eprintln!("[ERROR] Failed to save providers config");
        return 1;
    }
    println!("Configured provider: {provider}");
    0
}

fn test_providers(root: &Path, command: &ProvidersCommand) -> u8 {
    let config = load_config(root);
    let to_test: Vec<&Provider> = if let Some(ref name) = command.provider {
        PROVIDERS
            .iter()
            .filter(|p| p.name == name.as_str() || p.config_name == name.as_str())
            .collect()
    } else {
        PROVIDERS.iter().filter(|p| is_active(p, &config)).collect()
    };
    if to_test.is_empty() {
        println!("No providers to test. Use \"providers configure\" to add providers.");
        return 0;
    }
    println!("\nProvider Connectivity Test");
    println!("{}", "\u{2500}".repeat(50));
    let mut pass_count = 0;
    for p in &to_test {
        let active = is_active(p, &config);
        let has_key = !p.env_var.is_empty()
            && (std::env::var_os(p.env_var).is_some()
                || config
                    .get(p.config_name)
                    .and_then(|v| v.get("apiKey"))
                    .is_some());
        // Actual connectivity test deferred (no HTTP client); check key presence.
        let no_key_msg = format!("No API key ({})", p.env_var);
        let (icon, msg): (&str, &str) = if has_key {
            pass_count += 1;
            ("\u{2714}", "API key present")
        } else if !p.env_var.is_empty() {
            ("\u{2718}", &no_key_msg)
        } else {
            ("\u{2714}", "Local provider (no key needed)")
        };
        let _ = active;
        println!("  {icon}  {}: {msg}", p.name);
    }
    println!("\n  {pass_count}/{} provider(s) passed.", to_test.len());
    0
}

fn models(command: &ProvidersCommand) -> u8 {
    println!("\nAvailable Models");
    println!("{}", "\u{2500}".repeat(70));
    let filtered: Vec<&Provider> = if let Some(ref name) = command.provider {
        PROVIDERS
            .iter()
            .filter(|p| p.name == name.as_str() || p.config_name == name.as_str())
            .collect()
    } else {
        PROVIDERS.iter().collect()
    };
    for p in &filtered {
        println!("\n  {} ({})", p.name, p.ptype);
        for model in p.models.split(", ") {
            println!("    \u{2022} {model}");
        }
    }
    0
}

fn usage(root: &Path, command: &ProvidersCommand) -> u8 {
    let config = load_config(root);
    let active: Vec<&Provider> = PROVIDERS.iter().filter(|p| is_active(p, &config)).collect();
    if command.json {
        let arr: Vec<Value> = active
            .iter()
            .map(|p| {
                json!({
                    "name": p.name, "type": p.ptype, "usage": null,
                    "note": "Usage tracking not available in native build."
                })
            })
            .collect();
        println!(
            "{}",
            serde_json::to_string_pretty(&json!(arr)).unwrap_or_default()
        );
    } else {
        println!("\nProvider Usage");
        println!("{}", "\u{2500}".repeat(50));
        if active.is_empty() {
            println!("  No active providers.");
        }
        for p in &active {
            println!("  {} ({}): usage tracking not available", p.name, p.ptype);
        }
    }
    0
}
