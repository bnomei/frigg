//! Hidden Claude Code hook handlers.

use std::io::{self, Read, Write};

use frigg::agent_directive::FRIGG_FIRST_DIRECTIVE;
use serde_json::{Value, json};

pub(crate) fn run_pretooluse_hook_command<R: Read, W: Write>(
    mut stdin: R,
    mut stdout: W,
) -> io::Result<()> {
    let mut input = String::new();
    stdin.read_to_string(&mut input)?;
    if let Some(output) = render_pretooluse_hook_output(&input) {
        stdout.write_all(output.as_bytes())?;
        stdout.write_all(b"\n")?;
    }
    Ok(())
}

fn render_pretooluse_hook_output(input: &str) -> Option<String> {
    let value = serde_json::from_str::<Value>(input).ok()?;
    should_nudge_pretooluse(&value).then(|| {
        json!({
            "hookSpecificOutput": {
                "hookEventName": "PreToolUse",
                "additionalContext": FRIGG_FIRST_DIRECTIVE.trim(),
            }
        })
        .to_string()
    })
}

fn should_nudge_pretooluse(value: &Value) -> bool {
    if value
        .get("hook_event_name")
        .and_then(Value::as_str)
        .is_some_and(|event_name| event_name != "PreToolUse")
    {
        return false;
    }

    match value.get("tool_name").and_then(Value::as_str) {
        Some("Grep") => true,
        Some("Bash") => value
            .get("tool_input")
            .and_then(|input| input.get("command"))
            .and_then(Value::as_str)
            .is_some_and(contains_grep_like_command),
        Some("Read") => is_whole_file_code_read(value.get("tool_input")),
        _ => false,
    }
}

fn contains_grep_like_command(command: &str) -> bool {
    command
        .split(|character: char| {
            !(character.is_ascii_alphanumeric() || matches!(character, '_' | '-' | '/' | '.'))
        })
        .filter(|token| !token.is_empty())
        .any(|token| {
            let command_name = token.rsplit('/').next().unwrap_or(token);
            matches!(command_name, "grep" | "egrep" | "fgrep" | "rg" | "ripgrep")
        })
}

fn is_whole_file_code_read(tool_input: Option<&Value>) -> bool {
    let Some(tool_input) = tool_input.and_then(Value::as_object) else {
        return false;
    };
    if value_present(tool_input.get("offset")) || value_present(tool_input.get("limit")) {
        return false;
    }
    let Some(path) = tool_input
        .get("file_path")
        .or_else(|| tool_input.get("path"))
        .and_then(Value::as_str)
    else {
        return false;
    };
    is_code_path(path)
}

fn value_present(value: Option<&Value>) -> bool {
    value.is_some_and(|value| !value.is_null())
}

fn is_code_path(path: &str) -> bool {
    let lower = path.to_ascii_lowercase();
    let extension = lower.rsplit_once('.').map(|(_, extension)| extension);
    matches!(
        extension,
        Some(
            "astro"
                | "bash"
                | "c"
                | "cc"
                | "cljs"
                | "clj"
                | "cpp"
                | "cs"
                | "css"
                | "cts"
                | "cxx"
                | "dart"
                | "erl"
                | "ex"
                | "exs"
                | "fish"
                | "fs"
                | "fsi"
                | "fsx"
                | "go"
                | "h"
                | "hpp"
                | "hrl"
                | "hs"
                | "html"
                | "java"
                | "js"
                | "jsx"
                | "kt"
                | "kts"
                | "lua"
                | "mjs"
                | "ml"
                | "mli"
                | "mts"
                | "nim"
                | "php"
                | "pl"
                | "pm"
                | "ps1"
                | "py"
                | "r"
                | "rb"
                | "rs"
                | "scala"
                | "sh"
                | "sql"
                | "svelte"
                | "swift"
                | "ts"
                | "tsx"
                | "vue"
                | "zig"
        )
    )
}

#[cfg(test)]
mod tests {
    use serde_json::{Value, json};

    use super::{render_pretooluse_hook_output, run_pretooluse_hook_command};

    #[test]
    fn pretooluse_grep_emits_hook_specific_nudge() {
        let output = render_pretooluse_hook_output(
            r#"{"hook_event_name":"PreToolUse","tool_name":"Grep","tool_input":{"pattern":"needle"}}"#,
        )
        .expect("grep should produce output");
        let value = serde_json::from_str::<Value>(&output).expect("hook output should be json");

        let hook_output = value
            .get("hookSpecificOutput")
            .expect("hookSpecificOutput should exist");
        assert_eq!(
            hook_output.get("hookEventName").and_then(Value::as_str),
            Some("PreToolUse")
        );
        assert!(
            hook_output.get("permissionDecision").is_none(),
            "hook must not allow or deny tool execution"
        );
        assert!(
            hook_output
                .get("additionalContext")
                .and_then(Value::as_str)
                .is_some_and(|context| context.contains("Frigg"))
        );
    }

    #[test]
    fn pretooluse_bash_grep_like_commands_emit_nudge() {
        for command in [
            "grep -R needle crates/cli/src",
            "rg needle crates/cli/src",
            "git grep needle",
            "/usr/bin/ripgrep needle",
        ] {
            let input = json!({
                "hook_event_name": "PreToolUse",
                "tool_name": "Bash",
                "tool_input": { "command": command },
            })
            .to_string();

            assert!(
                render_pretooluse_hook_output(&input).is_some(),
                "expected nudge for command: {command}"
            );
        }
    }

    #[test]
    fn pretooluse_whole_file_code_read_emits_nudge() {
        let input = json!({
            "hook_event_name": "PreToolUse",
            "tool_name": "Read",
            "tool_input": { "file_path": "crates/cli/src/main.rs" },
        })
        .to_string();

        assert!(render_pretooluse_hook_output(&input).is_some());
    }

    #[test]
    fn pretooluse_ranged_read_non_code_and_unrelated_tools_are_silent() {
        let cases = [
            json!({
                "hook_event_name": "PreToolUse",
                "tool_name": "Read",
                "tool_input": { "file_path": "crates/cli/src/main.rs", "offset": 12, "limit": 20 },
            }),
            json!({
                "hook_event_name": "PreToolUse",
                "tool_name": "Read",
                "tool_input": { "file_path": "README.md" },
            }),
            json!({
                "hook_event_name": "PreToolUse",
                "tool_name": "Bash",
                "tool_input": { "command": "cargo test -p frigg hook" },
            }),
            json!({
                "hook_event_name": "PreToolUse",
                "tool_name": "Write",
                "tool_input": { "file_path": "src/lib.rs" },
            }),
        ];

        for input in cases {
            assert_eq!(render_pretooluse_hook_output(&input.to_string()), None);
        }
    }

    #[test]
    fn pretooluse_invalid_or_empty_input_returns_success_without_stdout() {
        for input in ["", "not json", r#"{"tool_name":12}"#] {
            let mut stdout = Vec::new();

            run_pretooluse_hook_command(input.as_bytes(), &mut stdout)
                .expect("invalid hook input should not fail");

            assert!(stdout.is_empty(), "stdout should be empty for {input:?}");
        }
    }
}
