//! VS Code Claude Code extension synchronization
//!
//! Syncs environment variables from Claude provider config to VS Code's
//! `claudeCode.environmentVariables` setting in settings.json.

use std::path::PathBuf;

use serde_json::{json, Value};

use crate::config::atomic_write;
use crate::error::AppError;

/// Get the default VS Code settings.json path based on the current platform.
fn get_default_vscode_settings_path() -> Result<PathBuf, AppError> {
    #[cfg(target_os = "windows")]
    {
        if let Ok(appdata) = std::env::var("APPDATA") {
            return Ok(PathBuf::from(appdata)
                .join("Code")
                .join("User")
                .join("settings.json"));
        }
        Err(AppError::Config(
            "无法获取 APPDATA 环境变量".to_string(),
        ))
    }

    #[cfg(target_os = "macos")]
    {
        let home = dirs::home_dir()
            .ok_or_else(|| AppError::Config("无法获取用户主目录".to_string()))?;
        Ok(home
            .join("Library")
            .join("Application Support")
            .join("Code")
            .join("User")
            .join("settings.json"))
    }

    #[cfg(target_os = "linux")]
    {
        let home = dirs::home_dir()
            .ok_or_else(|| AppError::Config("无法获取用户主目录".to_string()))?;
        Ok(home
            .join(".config")
            .join("Code")
            .join("User")
            .join("settings.json"))
    }
}

/// Get the VS Code settings.json path, preferring user override if set.
pub fn get_vscode_settings_path() -> Result<PathBuf, AppError> {
    let settings = crate::settings::get_settings();
    if let Some(ref custom_path) = settings.vscode_settings_path {
        let trimmed = custom_path.trim();
        if !trimmed.is_empty() {
            return Ok(crate::settings::resolve_override_path_pub(trimmed));
        }
    }
    get_default_vscode_settings_path()
}

/// Convert a flat env object `{"KEY": "VALUE", ...}` into the VS Code
/// `claudeCode.environmentVariables` array format:
/// `[{"name": "KEY", "value": "VALUE"}, ...]`
fn env_to_vscode_array(env: &Value) -> Value {
    let obj = match env.as_object() {
        Some(obj) => obj,
        None => return json!([]),
    };

    let arr: Vec<Value> = obj
        .iter()
        .map(|(k, v)| {
            json!({
                "name": k,
                "value": v.as_str().unwrap_or("")
            })
        })
        .collect();

    Value::Array(arr)
}

/// Sync environment variables to VS Code's settings.json.
///
/// Reads the existing file, updates only the `claudeCode.environmentVariables`
/// key, and writes it back atomically. All other settings are preserved.
pub fn sync_env_to_vscode(env: &Value) -> Result<(), AppError> {
    let path = get_vscode_settings_path()?;

    // Read existing VS Code settings (or start with empty object)
    let mut settings = if path.exists() {
        let content =
            std::fs::read_to_string(&path).map_err(|e| AppError::io(&path, e))?;
        // VS Code settings.json may contain comments (JSONC) — try standard parse first
        serde_json::from_str::<Value>(&content).unwrap_or_else(|_| {
            log::warn!(
                "VS Code settings.json 解析失败（可能包含注释），将尝试清除注释后重新解析"
            );
            // Strip single-line comments (//) and try again
            let stripped = strip_jsonc_comments(&content);
            serde_json::from_str::<Value>(&stripped).unwrap_or_else(|_| json!({}))
        })
    } else {
        json!({})
    };

    let vscode_array = env_to_vscode_array(env);

    // Update the claudeCode.environmentVariables field
    if let Some(obj) = settings.as_object_mut() {
        obj.insert(
            "claudeCode.environmentVariables".to_string(),
            vscode_array,
        );
    }

    // Ensure parent directory exists
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent).map_err(|e| AppError::io(parent, e))?;
    }

    let json_str = serde_json::to_string_pretty(&settings)
        .map_err(|e| AppError::JsonSerialize { source: e })?;

    atomic_write(&path, json_str.as_bytes())?;

    log::info!(
        "VS Code Claude 插件环境变量已同步到 {}",
        path.display()
    );
    Ok(())
}

/// Clear `claudeCode.environmentVariables` from VS Code settings.json.
pub fn clear_vscode_env() -> Result<(), AppError> {
    let path = get_vscode_settings_path()?;

    if !path.exists() {
        return Ok(());
    }

    let content =
        std::fs::read_to_string(&path).map_err(|e| AppError::io(&path, e))?;
    let mut settings = serde_json::from_str::<Value>(&content).unwrap_or_else(|_| {
        let stripped = strip_jsonc_comments(&content);
        serde_json::from_str::<Value>(&stripped).unwrap_or_else(|_| json!({}))
    });

    if let Some(obj) = settings.as_object_mut() {
        if obj.remove("claudeCode.environmentVariables").is_none() {
            return Ok(()); // Key didn't exist, nothing to do
        }
    } else {
        return Ok(());
    }

    let json_str = serde_json::to_string_pretty(&settings)
        .map_err(|e| AppError::JsonSerialize { source: e })?;

    atomic_write(&path, json_str.as_bytes())?;

    log::info!(
        "VS Code Claude 插件环境变量已从 {} 中移除",
        path.display()
    );
    Ok(())
}

/// Strip single-line comments (//) from JSONC content.
/// This is a best-effort approach for VS Code's JSONC format.
fn strip_jsonc_comments(input: &str) -> String {
    let mut result = String::with_capacity(input.len());
    let mut in_string = false;
    let mut escape_next = false;
    let mut chars = input.chars().peekable();

    while let Some(ch) = chars.next() {
        if escape_next {
            result.push(ch);
            escape_next = false;
            continue;
        }

        if in_string {
            result.push(ch);
            if ch == '\\' {
                escape_next = true;
            } else if ch == '"' {
                in_string = false;
            }
            continue;
        }

        match ch {
            '"' => {
                in_string = true;
                result.push(ch);
            }
            '/' => {
                if chars.peek() == Some(&'/') {
                    // Single-line comment — skip until end of line
                    chars.next(); // consume second '/'
                    for c in chars.by_ref() {
                        if c == '\n' {
                            result.push('\n');
                            break;
                        }
                    }
                } else if chars.peek() == Some(&'*') {
                    // Block comment — skip until */
                    chars.next(); // consume '*'
                    let mut prev = ' ';
                    for c in chars.by_ref() {
                        if prev == '*' && c == '/' {
                            break;
                        }
                        if c == '\n' {
                            result.push('\n');
                        }
                        prev = c;
                    }
                } else {
                    result.push(ch);
                }
            }
            _ => result.push(ch),
        }
    }

    result
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn env_to_vscode_array_converts_flat_map() {
        let env = json!({
            "ANTHROPIC_BASE_URL": "http://localhost:3000",
            "ANTHROPIC_AUTH_TOKEN": "test-token"
        });

        let result = env_to_vscode_array(&env);
        let arr = result.as_array().expect("should be array");
        assert_eq!(arr.len(), 2);

        // Check that each element has "name" and "value"
        for item in arr {
            assert!(item.get("name").is_some());
            assert!(item.get("value").is_some());
        }
    }

    #[test]
    fn env_to_vscode_array_handles_empty() {
        let env = json!({});
        let result = env_to_vscode_array(&env);
        assert_eq!(result, json!([]));
    }

    #[test]
    fn env_to_vscode_array_handles_non_object() {
        let env = json!("not an object");
        let result = env_to_vscode_array(&env);
        assert_eq!(result, json!([]));
    }

    #[test]
    fn strip_jsonc_comments_removes_single_line() {
        let input = r#"{
    // This is a comment
    "key": "value"
}"#;
        let stripped = strip_jsonc_comments(input);
        let parsed: Value = serde_json::from_str(&stripped).expect("should parse");
        assert_eq!(parsed.get("key").and_then(|v| v.as_str()), Some("value"));
    }

    #[test]
    fn strip_jsonc_comments_removes_block_comment() {
        let input = r#"{
    /* block comment */
    "key": "value"
}"#;
        let stripped = strip_jsonc_comments(input);
        let parsed: Value = serde_json::from_str(&stripped).expect("should parse");
        assert_eq!(parsed.get("key").and_then(|v| v.as_str()), Some("value"));
    }

    #[test]
    fn strip_jsonc_comments_preserves_url_in_string() {
        let input = r#"{"url": "https://example.com"}"#;
        let stripped = strip_jsonc_comments(input);
        let parsed: Value = serde_json::from_str(&stripped).expect("should parse");
        assert_eq!(
            parsed.get("url").and_then(|v| v.as_str()),
            Some("https://example.com")
        );
    }
}
