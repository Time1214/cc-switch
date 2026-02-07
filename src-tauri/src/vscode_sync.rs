//! VS Code Claude Code extension synchronization
//!
//! Syncs environment variables from Claude provider config to VS Code's
//! `claudeCode.environmentVariables` setting in settings.json.
//!
//! Uses text-level editing (regex + string manipulation) to preserve
//! JSONC comments and original formatting in settings.json.

use std::path::PathBuf;

use regex::Regex;
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

/// Format the VS Code array value as a pretty-printed JSON string with indent.
fn format_vscode_array_value(arr: &Value, indent: &str) -> String {
    let items = match arr.as_array() {
        Some(items) if !items.is_empty() => items,
        _ => return "[]".to_string(),
    };

    let inner_indent = format!("{}    ", indent);
    let mut parts = Vec::new();
    for item in items {
        let name = item.get("name").and_then(|v| v.as_str()).unwrap_or("");
        let value = item.get("value").and_then(|v| v.as_str()).unwrap_or("");
        parts.push(format!(
            r#"{}{{"name": "{}", "value": "{}"}}"#,
            inner_indent, name, value
        ));
    }

    format!("[\n{}\n{}]", parts.join(",\n"), indent)
}

/// Find the range of `"claudeCode.environmentVariables": <value>` in JSONC text.
///
/// Returns `Some((start, end))` where start is the beginning of the key string
/// and end is after the value (including the array).
/// Returns `None` if not found.
fn find_claude_env_range(content: &str) -> Option<(usize, usize)> {
    // Match the key "claudeCode.environmentVariables" followed by : and a JSON array value.
    // We need to handle the full array value which may span multiple lines.
    let key_pattern = r#""claudeCode\.environmentVariables"\s*:"#;
    let re = Regex::new(key_pattern).ok()?;
    let mat = re.find(content)?;

    let key_start = mat.start();
    let after_colon = mat.end();

    // Skip whitespace after the colon
    let remaining = &content[after_colon..];
    let trimmed_offset = remaining.len() - remaining.trim_start().len();
    let value_start = after_colon + trimmed_offset;

    // Now we need to find the end of the JSON value starting at value_start.
    // The value should be a JSON array [...].
    let value_str = &content[value_start..];
    if !value_str.starts_with('[') {
        // Value is not an array — skip it (could be some other type).
        // Try to find the end: scan for the next , or } at the same nesting level.
        let end = find_value_end(content, value_start)?;
        return Some((key_start, end));
    }

    let end = find_bracket_end(content, value_start)?;
    Some((key_start, end))
}

/// Find the end of a bracket-delimited value (array or object) starting at `start`.
/// `content[start]` must be `[` or `{`.
fn find_bracket_end(content: &str, start: usize) -> Option<usize> {
    let bytes = content.as_bytes();
    let open = bytes[start];
    let close = match open {
        b'[' => b']',
        b'{' => b'}',
        _ => return None,
    };

    let mut depth = 0i32;
    let mut in_string = false;
    let mut escape_next = false;

    for (i, &b) in bytes[start..].iter().enumerate() {
        if escape_next {
            escape_next = false;
            continue;
        }
        if in_string {
            if b == b'\\' {
                escape_next = true;
            } else if b == b'"' {
                in_string = false;
            }
            continue;
        }
        match b {
            b'"' => in_string = true,
            b if b == open => depth += 1,
            b if b == close => {
                depth -= 1;
                if depth == 0 {
                    return Some(start + i + 1);
                }
            }
            _ => {}
        }
    }
    None
}

/// Find the end of a generic JSON value (string, number, bool, null, array, object).
fn find_value_end(content: &str, start: usize) -> Option<usize> {
    let bytes = content.as_bytes();
    if start >= bytes.len() {
        return None;
    }

    match bytes[start] {
        b'[' | b'{' => find_bracket_end(content, start),
        b'"' => {
            // String value
            let mut escape_next = false;
            for (i, &b) in bytes[start + 1..].iter().enumerate() {
                if escape_next {
                    escape_next = false;
                    continue;
                }
                if b == b'\\' {
                    escape_next = true;
                } else if b == b'"' {
                    return Some(start + 1 + i + 1);
                }
            }
            None
        }
        _ => {
            // Number, bool, null — scan until delimiter
            for (i, &b) in bytes[start..].iter().enumerate() {
                if b == b',' || b == b'}' || b == b']' || b == b'\n' || b == b'\r' {
                    return Some(start + i);
                }
            }
            Some(bytes.len())
        }
    }
}

/// Detect the indentation used in the file (based on the first indented line).
fn detect_indent(content: &str) -> String {
    for line in content.lines() {
        let stripped = line.trim_start();
        if !stripped.is_empty() && line.len() != stripped.len() {
            let indent = &line[..line.len() - stripped.len()];
            return indent.to_string();
        }
    }
    "    ".to_string() // Default to 4 spaces
}

/// Sync environment variables to VS Code's settings.json.
///
/// Uses text-level editing to preserve JSONC comments and original formatting.
/// Only modifies the `claudeCode.environmentVariables` key-value pair.
pub fn sync_env_to_vscode(env: &Value) -> Result<(), AppError> {
    let path = get_vscode_settings_path()?;

    let vscode_array = env_to_vscode_array(env);

    let content = if path.exists() {
        std::fs::read_to_string(&path).map_err(|e| AppError::io(&path, e))?
    } else {
        String::new()
    };

    let new_content = if content.trim().is_empty() {
        // File doesn't exist or is empty — create a minimal settings.json
        let arr_str = serde_json::to_string_pretty(&vscode_array)
            .map_err(|e| AppError::JsonSerialize { source: e })?;
        format!("{{\n    \"claudeCode.environmentVariables\": {}\n}}\n", arr_str)
    } else if let Some((start, end)) = find_claude_env_range(&content) {
        // Key exists — replace only the value portion
        // Find where the value starts (after the colon + whitespace)
        let key_and_colon = r#""claudeCode.environmentVariables""#;
        let key_pos = content[start..].find(key_and_colon).unwrap_or(0) + start;
        let after_key = key_pos + key_and_colon.len();
        // Find the colon
        let colon_offset = content[after_key..].find(':').unwrap_or(0);
        let after_colon = after_key + colon_offset + 1;
        // Skip whitespace between colon and value
        let remaining = &content[after_colon..end];
        let ws_len = remaining.len() - remaining.trim_start().len();
        let value_start = after_colon + ws_len;

        let indent = detect_indent(&content);
        let new_value = format_vscode_array_value(&vscode_array, &indent);

        format!("{}{}{}", &content[..value_start], new_value, &content[end..])
    } else {
        // Key doesn't exist — insert before the last closing brace
        let indent = detect_indent(&content);
        let new_value = format_vscode_array_value(&vscode_array, &indent);
        let new_entry = format!(
            "{}\"claudeCode.environmentVariables\": {}",
            indent, new_value
        );

        // Find the last '}' in the file
        if let Some(last_brace) = content.rfind('}') {
            // Check if there are existing properties (need a comma)
            let before_brace = content[..last_brace].trim_end();
            let needs_comma = !before_brace.ends_with('{') && !before_brace.ends_with(',');
            let comma = if needs_comma { "," } else { "" };

            format!(
                "{}{}\n{}\n{}",
                &content[..last_brace].trim_end(),
                comma,
                new_entry,
                &content[last_brace..]
            )
        } else {
            // Malformed file — wrap in braces
            format!("{{\n{}\n}}\n", new_entry)
        }
    };

    // Ensure parent directory exists
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent).map_err(|e| AppError::io(parent, e))?;
    }

    atomic_write(&path, new_content.as_bytes())?;

    log::info!(
        "VS Code Claude 插件环境变量已同步到 {}",
        path.display()
    );
    Ok(())
}

/// Clear `claudeCode.environmentVariables` from VS Code settings.json.
///
/// Uses text-level editing to preserve JSONC comments and original formatting.
/// Removes the entire key-value pair including any trailing comma.
pub fn clear_vscode_env() -> Result<(), AppError> {
    let path = get_vscode_settings_path()?;

    if !path.exists() {
        return Ok(());
    }

    let content =
        std::fs::read_to_string(&path).map_err(|e| AppError::io(&path, e))?;

    let (start, end) = match find_claude_env_range(&content) {
        Some(range) => range,
        None => return Ok(()), // Key doesn't exist, nothing to do
    };

    // Expand the removal range to include:
    // 1. Any trailing comma after the value
    // 2. The trailing newline
    // 3. Any leading whitespace on the line containing the key
    let mut remove_end = end;
    let after = &content[end..];
    // Skip whitespace and a possible trailing comma
    for (i, ch) in after.char_indices() {
        if ch == ',' {
            remove_end = end + i + 1;
            break;
        } else if ch == '\n' || ch == '}' || ch == ']' {
            break;
        } else if !ch.is_whitespace() {
            break;
        }
    }
    // Also consume trailing newline
    if remove_end < content.len() && content.as_bytes()[remove_end] == b'\n' {
        remove_end += 1;
    } else if remove_end + 1 < content.len()
        && &content[remove_end..remove_end + 2] == "\r\n"
    {
        remove_end += 2;
    }

    // Expand start backwards to consume leading whitespace on the same line
    let mut remove_start = start;
    let before = &content[..start];
    for ch in before.chars().rev() {
        if ch == '\n' {
            break;
        }
        if ch.is_whitespace() {
            remove_start -= ch.len_utf8();
        } else {
            break;
        }
    }

    // Check if removal leaves a trailing comma before '}'.
    // E.g., `"foo": 1,\n  <removed>\n}` → need to remove that trailing comma
    let before_removed = content[..remove_start].trim_end();
    let after_removed = content[remove_end..].trim_start();
    let new_content = if before_removed.ends_with(',')
        && (after_removed.starts_with('}') || after_removed.starts_with(']'))
    {
        // Remove the dangling comma
        let comma_pos = content[..remove_start]
            .rfind(',')
            .unwrap_or(remove_start);
        format!("{}{}", &content[..comma_pos], &content[remove_end..])
    } else {
        format!("{}{}", &content[..remove_start], &content[remove_end..])
    };

    atomic_write(&path, new_content.as_bytes())?;

    log::info!(
        "VS Code Claude 插件环境变量已从 {} 中移除",
        path.display()
    );
    Ok(())
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
    fn find_claude_env_range_existing_key() {
        let content = r#"{
    "editor.fontSize": 14,
    "claudeCode.environmentVariables": [
        {"name": "FOO", "value": "bar"}
    ],
    "terminal.integrated.shell": "/bin/bash"
}"#;
        let (start, end) = find_claude_env_range(content).expect("should find range");
        let extracted = &content[start..end];
        assert!(extracted.contains("claudeCode.environmentVariables"));
        assert!(extracted.contains("FOO"));
    }

    #[test]
    fn find_claude_env_range_not_found() {
        let content = r#"{
    "editor.fontSize": 14
}"#;
        assert!(find_claude_env_range(content).is_none());
    }

    #[test]
    fn sync_preserves_comments_in_text() {
        // Simulate a settings.json with comments
        let content = r#"{
    // Editor settings
    "editor.fontSize": 14,
    "claudeCode.environmentVariables": [],
    /* Terminal config */
    "terminal.integrated.shell": "/bin/bash"
}"#;

        // Verify the key is found
        let range = find_claude_env_range(content);
        assert!(range.is_some());

        let (start, end) = range.unwrap();
        let indent = detect_indent(content);
        let new_arr = json!([{"name": "KEY", "value": "VAL"}]);
        let new_value = format_vscode_array_value(&new_arr, &indent);

        // Find value start
        let key_str = "\"claudeCode.environmentVariables\"";
        let key_pos = content[start..].find(key_str).unwrap() + start;
        let after_key = key_pos + key_str.len();
        let colon_offset = content[after_key..].find(':').unwrap();
        let after_colon = after_key + colon_offset + 1;
        let remaining = &content[after_colon..end];
        let ws_len = remaining.len() - remaining.trim_start().len();
        let value_start = after_colon + ws_len;

        let result = format!("{}{}{}", &content[..value_start], new_value, &content[end..]);

        // Comments should be preserved
        assert!(result.contains("// Editor settings"));
        assert!(result.contains("/* Terminal config */"));
        // Other settings should be preserved
        assert!(result.contains("\"editor.fontSize\": 14"));
        assert!(result.contains("\"terminal.integrated.shell\""));
        // New value should be there
        assert!(result.contains("KEY"));
        assert!(result.contains("VAL"));
    }

    #[test]
    fn clear_removes_key_and_preserves_rest() {
        let content = r#"{
    // Editor settings
    "editor.fontSize": 14,
    "claudeCode.environmentVariables": [
        {"name": "FOO", "value": "bar"}
    ],
    "terminal.integrated.shell": "/bin/bash"
}"#;
        let (start, end) = find_claude_env_range(content).unwrap();

        // Expand range for removal (simplified version of clear logic)
        let mut remove_end = end;
        let after = &content[end..];
        for (i, ch) in after.char_indices() {
            if ch == ',' {
                remove_end = end + i + 1;
                break;
            } else if ch == '\n' || ch == '}' {
                break;
            }
        }
        if remove_end < content.len() && content.as_bytes()[remove_end] == b'\n' {
            remove_end += 1;
        }
        let mut remove_start = start;
        let before = &content[..start];
        for ch in before.chars().rev() {
            if ch == '\n' {
                break;
            }
            if ch.is_whitespace() {
                remove_start -= ch.len_utf8();
            } else {
                break;
            }
        }
        let result = format!("{}{}", &content[..remove_start], &content[remove_end..]);

        assert!(result.contains("// Editor settings"));
        assert!(result.contains("editor.fontSize"));
        assert!(result.contains("terminal.integrated.shell"));
        assert!(!result.contains("claudeCode.environmentVariables"));
    }

    #[test]
    fn insert_to_existing_file_without_key() {
        let content = r#"{
    // My settings
    "editor.fontSize": 14
}"#;
        let indent = detect_indent(content);
        let new_arr = json!([{"name": "A", "value": "B"}]);
        let new_value = format_vscode_array_value(&new_arr, &indent);
        let new_entry = format!(
            "{}\"claudeCode.environmentVariables\": {}",
            indent, new_value
        );

        let last_brace = content.rfind('}').unwrap();
        let before_brace = content[..last_brace].trim_end();
        let needs_comma = !before_brace.ends_with('{') && !before_brace.ends_with(',');
        let comma = if needs_comma { "," } else { "" };

        let result = format!(
            "{}{}\n{}\n{}",
            before_brace,
            comma,
            new_entry,
            &content[last_brace..]
        );

        assert!(result.contains("// My settings"));
        assert!(result.contains("editor.fontSize"));
        assert!(result.contains("claudeCode.environmentVariables"));
        assert!(result.contains("\"A\""));
    }
}
