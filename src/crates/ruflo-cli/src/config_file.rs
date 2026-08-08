use std::env;
use std::fs;
use std::io;
use std::path::{Path, PathBuf};

use serde_json::{json, Map, Value};

const PRIMARY_CONFIG: &str = "claude-flow.config.json";
const SECONDARY_CONFIG: &str = ".claude-flow/config.json";

pub fn defaults() -> Value {
    json!({
        "version": "3.5",
        "agents": {
            "defaultType": "coder", "autoSpawn": false, "maxConcurrent": 8,
            "timeout": 300000, "providers": []
        },
        "swarm": {
            "topology": "hierarchical", "maxAgents": 8, "autoScale": false,
            "coordinationStrategy": "leader", "healthCheckInterval": 30000
        },
        "memory": {
            "backend": "hybrid", "persistPath": "./data/memory", "cacheSize": 1000,
            "enableHNSW": true, "vectorDimension": 384
        },
        "mcp": {
            "serverHost": "localhost", "serverPort": 3000, "autoStart": false,
            "transportType": "stdio", "tools": []
        },
        "cli": {
            "colorOutput": true, "interactive": true, "verbosity": "normal",
            "outputFormat": "text", "progressStyle": "spinner"
        },
        "hooks": { "enabled": true, "autoExecute": true, "hooks": [] }
    })
}

pub fn find(root: &Path) -> Option<PathBuf> {
    for relative in [PRIMARY_CONFIG, SECONDARY_CONFIG] {
        let candidate = root.join(relative);
        if candidate.exists() {
            return Some(candidate);
        }
    }
    env::var_os("CLAUDE_FLOW_CONFIG")
        .map(PathBuf::from)
        .filter(|path| path.exists())
}

pub fn load(root: &Path) -> io::Result<Value> {
    let Some(path) = find(root) else {
        return Ok(defaults());
    };
    let Ok(contents) = fs::read_to_string(&path) else {
        return Ok(defaults());
    };
    Ok(serde_json::from_str::<Value>(&contents).unwrap_or_else(|_| defaults()))
}

pub fn create(root: &Path, force: bool) -> io::Result<PathBuf> {
    let path = root.join(PRIMARY_CONFIG);
    if path.exists() && !force {
        return Err(io::Error::new(
            io::ErrorKind::AlreadyExists,
            format!(
                "Config file already exists: {}. Use --force to overwrite.",
                path.display()
            ),
        ));
    }
    write_atomic(&path, &defaults())?;
    Ok(path)
}

pub fn get<'a>(config: &'a Value, key: &str) -> Option<&'a Value> {
    let mut current = config;
    for part in key.split('.') {
        current = match current {
            Value::Object(object) => object.get(part)?,
            Value::Array(array) => array.get(part.parse::<usize>().ok()?)?,
            _ => return None,
        };
    }
    Some(current)
}

pub fn set(root: &Path, key: &str, value: Value) -> io::Result<PathBuf> {
    if key.is_empty() || key.split('.').any(str::is_empty) {
        return Err(io::Error::new(
            io::ErrorKind::InvalidInput,
            "configuration key is invalid",
        ));
    }
    let target = find(root).unwrap_or_else(|| root.join(PRIMARY_CONFIG));
    let mut config = load(root)?;
    let parts = key.split('.').collect::<Vec<_>>();
    set_nested(&mut config, &parts, value)?;
    write_atomic(&target, &config)?;
    Ok(target)
}

pub fn reset(root: &Path, section: Option<&str>) -> io::Result<PathBuf> {
    // V3 currently accepts --section but its owning ConfigFileManager resets
    // the complete document. Preserve that observable behavior.
    let _ = section;
    let config = defaults();
    let path = root.join(PRIMARY_CONFIG);
    write_atomic(&path, &config)?;
    Ok(path)
}

pub fn export(root: &Path, output: &Path) -> io::Result<PathBuf> {
    let path = resolve(root, output);
    write_atomic(&path, &load(root)?)?;
    Ok(path)
}

pub fn import(root: &Path, input: &Path, merge: bool) -> io::Result<PathBuf> {
    let input = resolve(root, input);
    if !input.is_file() {
        return Err(io::Error::new(
            io::ErrorKind::NotFound,
            format!("Import file not found: {}", input.display()),
        ));
    }
    let contents = fs::read_to_string(&input)?;
    let imported: Value = serde_json::from_str(&contents).map_err(|_| {
        io::Error::new(
            io::ErrorKind::InvalidData,
            format!("Invalid JSON in import file: {}", input.display()),
        )
    })?;
    if !imported.is_object() {
        return Err(io::Error::new(
            io::ErrorKind::InvalidData,
            "Import file must contain a JSON object",
        ));
    }
    // V3 exposes --merge but ConfigFileManager.importFrom replaces the
    // document. Keep the flag accepted while matching the source effect.
    let _ = merge;
    let config = imported;
    let path = root.join(PRIMARY_CONFIG);
    write_atomic(&path, &config)?;
    Ok(path)
}

pub fn resolve(root: &Path, path: &Path) -> PathBuf {
    use std::path::Component;

    let joined = if path.is_absolute() {
        path.to_path_buf()
    } else {
        root.join(path)
    };
    let mut resolved = PathBuf::new();
    for component in joined.components() {
        match component {
            Component::CurDir => {}
            Component::ParentDir => {
                resolved.pop();
            }
            other => resolved.push(other.as_os_str()),
        }
    }
    resolved
}

pub fn parse_value(raw: &str) -> Value {
    if raw.eq_ignore_ascii_case("true") {
        return Value::Bool(true);
    }
    if raw.eq_ignore_ascii_case("false") {
        return Value::Bool(false);
    }
    let trimmed = raw.trim();
    if !trimmed.is_empty() {
        let radix_number = [
            ("0x", 16),
            ("0X", 16),
            ("0b", 2),
            ("0B", 2),
            ("0o", 8),
            ("0O", 8),
        ]
        .into_iter()
        .find_map(|(prefix, radix)| {
            trimmed
                .strip_prefix(prefix)
                .and_then(|digits| u64::from_str_radix(digits, radix).ok())
        });
        if let Some(value) = radix_number {
            return Value::from(value);
        }
        if let Ok(value) = trimmed.parse::<f64>() {
            if value.is_finite() {
                if value.fract() == 0.0 && value >= i64::MIN as f64 && value <= i64::MAX as f64 {
                    return Value::from(value as i64);
                }
                if let Some(number) = serde_json::Number::from_f64(value) {
                    return Value::Number(number);
                }
            } else {
                return Value::Null;
            }
        }
    }
    match serde_json::from_str::<Value>(raw) {
        Ok(value @ (Value::Object(_) | Value::Array(_) | Value::Null)) => value,
        _ => Value::String(raw.to_string()),
    }
}

fn set_nested(current: &mut Value, parts: &[&str], value: Value) -> io::Result<()> {
    let Some((part, rest)) = parts.split_first() else {
        return Ok(());
    };
    if rest.is_empty() {
        match current {
            Value::Object(object) => {
                object.insert((*part).to_string(), value);
            }
            Value::Array(array) => {
                let index = part.parse::<usize>().map_err(|_| {
                    io::Error::new(io::ErrorKind::InvalidInput, "array key must be an index")
                })?;
                if array.len() <= index {
                    array.resize(index + 1, Value::Null);
                }
                array[index] = value;
            }
            _ => {
                *current = Value::Object(Map::from_iter([((*part).to_string(), value)]));
            }
        }
        return Ok(());
    }

    let child = match current {
        Value::Object(object) => object
            .entry((*part).to_string())
            .or_insert_with(|| Value::Object(Map::new())),
        Value::Array(array) => {
            let index = part.parse::<usize>().map_err(|_| {
                io::Error::new(io::ErrorKind::InvalidInput, "array key must be an index")
            })?;
            if array.len() <= index {
                array.resize(index + 1, Value::Null);
            }
            &mut array[index]
        }
        _ => {
            *current = Value::Object(Map::new());
            current
                .as_object_mut()
                .expect("object assigned above")
                .entry((*part).to_string())
                .or_insert_with(|| Value::Object(Map::new()))
        }
    };
    if !child.is_object() && !child.is_array() {
        *child = Value::Object(Map::new());
    }
    set_nested(child, rest, value)
}

pub fn flattened(config: &Value) -> Map<String, Value> {
    fn visit(value: &Value, prefix: &str, output: &mut Map<String, Value>) {
        if let Value::Object(object) = value {
            for (key, value) in object {
                let key = if prefix.is_empty() {
                    key.clone()
                } else {
                    format!("{prefix}.{key}")
                };
                if value.is_object() {
                    visit(value, &key, output);
                } else {
                    output.insert(key, value.clone());
                }
            }
        }
    }
    let mut output = Map::new();
    visit(config, "", &mut output);
    output
}

pub fn providers(
    root: &Path,
    add: Option<&str>,
    remove: Option<&str>,
    enable: Option<&str>,
    disable: Option<&str>,
) -> io::Result<ProviderResult> {
    let mutation = add.is_some() || remove.is_some() || enable.is_some() || disable.is_some();
    let configured = load(root)?.get("providers").cloned();
    let mut providers = configured
        .and_then(|value| value.as_array().cloned())
        .unwrap_or_default();

    if mutation {
        let mut messages = Vec::new();
        if let Some(name) = add {
            if providers
                .iter()
                .any(|provider| provider.get("name").and_then(Value::as_str) == Some(name))
            {
                return Ok(ProviderResult::failed(
                    providers,
                    messages,
                    format!("Provider '{name}' already exists"),
                ));
            }
            providers.push(json!({"name": name, "enabled": true, "priority": providers.len() + 1}));
            messages.push(format!("Added provider: {name}"));
        }
        if let Some(name) = remove {
            let before = providers.len();
            providers.retain(|provider| provider.get("name").and_then(Value::as_str) != Some(name));
            if providers.len() == before {
                return Ok(ProviderResult::failed(
                    providers,
                    messages,
                    format!("Provider '{name}' not found"),
                ));
            }
            messages.push(format!("Removed provider: {name}"));
        }
        for (name, enabled) in [(enable, true), (disable, false)] {
            if let Some(name) = name {
                let Some(provider) = providers
                    .iter_mut()
                    .find(|provider| provider.get("name").and_then(Value::as_str) == Some(name))
                else {
                    return Ok(ProviderResult::failed(
                        providers,
                        messages,
                        format!("Provider '{name}' not found"),
                    ));
                };
                provider
                    .as_object_mut()
                    .expect("provider is object")
                    .insert("enabled".into(), Value::Bool(enabled));
                messages.push(format!(
                    "{} provider: {name}",
                    if enabled { "Enabled" } else { "Disabled" }
                ));
            }
        }
        if let Err(error) = set(root, "providers", Value::Array(providers.clone())) {
            return Ok(ProviderResult::failed(
                providers,
                messages,
                format!("Failed to save providers: {error}"),
            ));
        }
        return Ok(ProviderResult {
            providers: Value::Array(providers),
            messages,
            failure: None,
        });
    }

    if providers.is_empty() {
        providers = vec![
            json!({"name":"anthropic","model":"claude-3-5-sonnet-20241022","priority":1,"enabled":true,"status":"Active"}),
            json!({"name":"openrouter","model":"claude-3.5-sonnet","priority":2,"enabled":false,"status":"Disabled"}),
            json!({"name":"ollama","model":"llama3.2","priority":3,"enabled":false,"status":"Disabled"}),
            json!({"name":"gemini","model":"gemini-2.0-flash","priority":4,"enabled":false,"status":"Disabled"}),
        ];
    } else {
        for (index, provider) in providers.iter_mut().enumerate() {
            let original = provider.as_object().ok_or_else(|| {
                io::Error::new(io::ErrorKind::InvalidData, "provider must be a JSON object")
            })?;
            let name = original
                .get("name")
                .filter(|value| js_truthy(value))
                .map(js_string)
                .unwrap_or_default();
            let model = original
                .get("model")
                .filter(|value| js_truthy(value))
                .map(js_string)
                .unwrap_or_default();
            let priority = original
                .get("priority")
                .filter(|value| js_truthy(value))
                .and_then(js_number)
                .unwrap_or((index + 1) as f64);
            let enabled = original
                .get("enabled")
                .and_then(Value::as_bool)
                .unwrap_or(true);
            *provider = json!({
                "name": name,
                "model": model,
                "priority": if priority.fract() == 0.0 { Value::from(priority as i64) } else { Value::from(priority) },
                "enabled": enabled,
                "status": if enabled { "Active" } else { "Disabled" },
            });
        }
    }
    Ok(ProviderResult {
        providers: Value::Array(providers),
        messages: Vec::new(),
        failure: None,
    })
}

pub struct ProviderResult {
    pub providers: Value,
    pub messages: Vec<String>,
    pub failure: Option<String>,
}

impl ProviderResult {
    fn failed(providers: Vec<Value>, messages: Vec<String>, failure: String) -> Self {
        Self {
            providers: Value::Array(providers),
            messages,
            failure: Some(failure),
        }
    }
}

fn write_atomic(path: &Path, value: &Value) -> io::Result<()> {
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent)?;
    }
    let temporary = PathBuf::from(format!("{}.tmp", path.display()));
    let mut contents = serde_json::to_string_pretty(value)
        .map_err(|error| io::Error::new(io::ErrorKind::InvalidData, error))?;
    contents.push('\n');
    fs::write(&temporary, contents)?;
    fs::rename(&temporary, path)
}

fn js_string(value: &Value) -> String {
    match value {
        Value::Null => String::new(),
        Value::String(value) => value.clone(),
        Value::Bool(value) => value.to_string(),
        Value::Number(value) => value.to_string(),
        Value::Array(values) => values.iter().map(js_string).collect::<Vec<_>>().join(","),
        Value::Object(_) => "[object Object]".into(),
    }
}

fn js_number(value: &Value) -> Option<f64> {
    match value {
        Value::Number(value) => value.as_f64(),
        Value::String(value) => value.parse().ok(),
        Value::Bool(true) => Some(1.0),
        Value::Bool(false) | Value::Null => Some(0.0),
        _ => None,
    }
}

fn js_truthy(value: &Value) -> bool {
    match value {
        Value::Null => false,
        Value::Bool(value) => *value,
        Value::Number(value) => value.as_f64().is_some_and(|value| value != 0.0),
        Value::String(value) => !value.is_empty(),
        Value::Array(_) | Value::Object(_) => true,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn v3_config_round_trip_covers_create_set_reset_export_and_import() {
        let temp = tempfile::tempdir().unwrap();
        let path = create(temp.path(), false).unwrap();
        assert_eq!(path.file_name().unwrap(), PRIMARY_CONFIG);
        assert_eq!(
            get(&load(temp.path()).unwrap(), "swarm.maxAgents"),
            Some(&Value::from(8))
        );

        set(temp.path(), "swarm.maxAgents", Value::from(20)).unwrap();
        assert_eq!(
            get(&load(temp.path()).unwrap(), "swarm.maxAgents"),
            Some(&Value::from(20))
        );

        export(temp.path(), Path::new("backup.json")).unwrap();
        set(temp.path(), "swarm.maxAgents", Value::from(30)).unwrap();
        import(temp.path(), Path::new("backup.json"), false).unwrap();
        assert_eq!(
            get(&load(temp.path()).unwrap(), "swarm.maxAgents"),
            Some(&Value::from(20))
        );

        reset(temp.path(), Some("swarm")).unwrap();
        assert_eq!(
            get(&load(temp.path()).unwrap(), "swarm.maxAgents"),
            Some(&Value::from(8))
        );
    }

    #[test]
    fn parses_v3_scalar_and_structured_values() {
        assert_eq!(parse_value("true"), Value::Bool(true));
        assert_eq!(parse_value("20"), Value::from(20));
        assert_eq!(parse_value("1.5"), Value::from(1.5));
        assert!(parse_value(r#"["a","b"]"#).is_array());
        assert_eq!(parse_value("hybrid"), Value::String("hybrid".into()));
        assert_eq!(parse_value("TRUE"), Value::Bool(true));
        assert_eq!(parse_value("-1"), Value::from(-1));
        assert_eq!(parse_value("1e3"), Value::from(1000));
        assert_eq!(parse_value("0x10"), Value::from(16));
        assert_eq!(parse_value("1.0"), Value::from(1));
        assert!(parse_value("184467440737095516160000").is_number());
    }

    #[test]
    fn nested_get_and_set_traverse_provider_arrays() {
        let temp = tempfile::tempdir().unwrap();
        set(temp.path(), "agents.providers", json!([{"name":"first"}])).unwrap();
        assert_eq!(
            get(&load(temp.path()).unwrap(), "agents.providers.0.name"),
            Some(&Value::String("first".into()))
        );
        set(
            temp.path(),
            "agents.providers.0.name",
            Value::String("updated".into()),
        )
        .unwrap();
        assert_eq!(
            get(&load(temp.path()).unwrap(), "agents.providers.0.name"),
            Some(&Value::String("updated".into()))
        );
    }

    #[test]
    fn path_resolution_is_lexically_normalized_without_requiring_existence() {
        assert_eq!(
            resolve(
                Path::new("/project/work"),
                Path::new("nested/../backup.json")
            ),
            PathBuf::from("/project/work/backup.json")
        );
    }

    #[test]
    fn set_reuses_secondary_but_reset_and_import_start_at_primary() {
        let temp = tempfile::tempdir().unwrap();
        fs::create_dir_all(temp.path().join(".claude-flow")).unwrap();
        fs::write(
            temp.path().join(SECONDARY_CONFIG),
            "{\"swarm\":{\"maxAgents\":4}}\n",
        )
        .unwrap();

        let set_path = set(temp.path(), "swarm.maxAgents", Value::from(9)).unwrap();
        assert_eq!(set_path, temp.path().join(SECONDARY_CONFIG));
        assert!(!temp.path().join(PRIMARY_CONFIG).exists());

        let reset_path = reset(temp.path(), Some("swarm")).unwrap();
        assert_eq!(reset_path, temp.path().join(PRIMARY_CONFIG));

        fs::remove_file(temp.path().join(PRIMARY_CONFIG)).unwrap();
        fs::write(temp.path().join("incoming.json"), "{\"imported\":true}\n").unwrap();
        let import_path = import(temp.path(), Path::new("incoming.json"), false).unwrap();
        assert_eq!(import_path, temp.path().join(PRIMARY_CONFIG));
        assert_eq!(load(temp.path()).unwrap()["imported"], true);
    }

    #[test]
    fn invalid_or_unreadable_existing_config_falls_back_to_defaults() {
        let temp = tempfile::tempdir().unwrap();
        fs::write(temp.path().join(PRIMARY_CONFIG), "not json").unwrap();
        assert_eq!(load(temp.path()).unwrap()["version"], "3.5");

        fs::remove_file(temp.path().join(PRIMARY_CONFIG)).unwrap();
        fs::create_dir(temp.path().join(PRIMARY_CONFIG)).unwrap();
        assert_eq!(load(temp.path()).unwrap()["version"], "3.5");
    }

    #[test]
    fn provider_listing_matches_javascript_falsy_coercion() {
        let temp = tempfile::tempdir().unwrap();
        fs::write(
            temp.path().join(PRIMARY_CONFIG),
            r#"{"providers":[{"name":0,"model":false,"priority":0,"enabled":null}]}"#,
        )
        .unwrap();

        let result = providers(temp.path(), None, None, None, None).unwrap();
        assert_eq!(
            result.providers,
            json!([{"name":"","model":"","priority":1,"enabled":true,"status":"Active"}])
        );
    }
}
