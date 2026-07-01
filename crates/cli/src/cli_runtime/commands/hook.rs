//! Hidden Claude Code hook handlers.

use std::io::{self, Read, Write};

use frigg::agent_directive::HOOK_NUDGE;
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
                "additionalContext": HOOK_NUDGE,
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
    let mut tokens = ShellTokens::new(command);
    let mut command_position = true;
    let mut git_subcommand_position = false;

    while let Some(token) = tokens.next() {
        match token {
            ShellToken::Separator => {
                command_position = true;
                git_subcommand_position = false;
            }
            ShellToken::Word(word) => {
                let command_name = command_name(word);

                if git_subcommand_position {
                    if command_name == "grep" {
                        return true;
                    }
                    if !word.starts_with('-') {
                        git_subcommand_position = false;
                    }
                    command_position = false;
                    continue;
                }

                if !command_position {
                    continue;
                }

                if is_shell_assignment(word) {
                    continue;
                }
                if is_grep_like_command_name(command_name) {
                    return true;
                }
                if command_name == "git" {
                    git_subcommand_position = true;
                    command_position = false;
                    continue;
                }
                command_position = is_command_wrapper(command_name);
            }
        }
    }

    false
}

fn command_name(word: &str) -> &str {
    word.rsplit('/').next().unwrap_or(word)
}

fn is_grep_like_command_name(command_name: &str) -> bool {
    matches!(command_name, "grep" | "egrep" | "fgrep" | "rg" | "ripgrep")
}

fn is_command_wrapper(command_name: &str) -> bool {
    matches!(
        command_name,
        "command" | "exec" | "env" | "noglob" | "sudo" | "time"
    )
}

fn is_shell_assignment(word: &str) -> bool {
    let Some((name, _)) = word.split_once('=') else {
        return false;
    };
    let mut characters = name.chars();
    characters
        .next()
        .is_some_and(|character| character == '_' || character.is_ascii_alphabetic())
        && characters.all(|character| character == '_' || character.is_ascii_alphanumeric())
}

enum ShellToken<'a> {
    Word(&'a str),
    Separator,
}

struct ShellTokens<'a> {
    command: &'a str,
    index: usize,
}

impl<'a> ShellTokens<'a> {
    fn new(command: &'a str) -> Self {
        Self { command, index: 0 }
    }
}

impl<'a> Iterator for ShellTokens<'a> {
    type Item = ShellToken<'a>;

    fn next(&mut self) -> Option<Self::Item> {
        let bytes = self.command.as_bytes();
        while self.index < bytes.len() && bytes[self.index].is_ascii_whitespace() {
            if bytes[self.index] == b'\n' {
                self.index += 1;
                return Some(ShellToken::Separator);
            }
            self.index += 1;
        }
        if self.index >= bytes.len() {
            return None;
        }

        let start = self.index;
        let separator_width = match bytes[self.index] {
            b';' | b'|' => Some(1),
            b'&' if self.index + 1 < bytes.len() && bytes[self.index + 1] == b'&' => Some(2),
            _ => None,
        };
        if let Some(width) = separator_width {
            self.index += width;
            return Some(ShellToken::Separator);
        }

        let mut quote = None;
        while self.index < bytes.len() {
            let byte = bytes[self.index];
            if let Some(quote_byte) = quote {
                self.index += 1;
                if byte == quote_byte {
                    quote = None;
                }
                continue;
            }

            match byte {
                b'\'' | b'"' => {
                    quote = Some(byte);
                    self.index += 1;
                }
                b'\\' => {
                    self.index = (self.index + 2).min(bytes.len());
                }
                b'\n' | b';' | b'|' => break,
                b'&' if self.index + 1 < bytes.len() && bytes[self.index + 1] == b'&' => break,
                byte if byte.is_ascii_whitespace() => break,
                _ => self.index += 1,
            }
        }

        Some(ShellToken::Word(&self.command[start..self.index]))
    }
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
    use frigg::agent_directive::HOOK_NUDGE;
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
                .is_some_and(|context| context == HOOK_NUDGE)
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
                "tool_name": "Bash",
                "tool_input": { "command": "echo rg" },
            }),
            json!({
                "hook_event_name": "PreToolUse",
                "tool_name": "Bash",
                "tool_input": { "command": "printf 'grep\\n'" },
            }),
            json!({
                "hook_event_name": "PreToolUse",
                "tool_name": "Bash",
                "tool_input": { "command": "cargo run -- rg needle" },
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
