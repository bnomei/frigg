//! Hidden Claude Code hook handlers.
//!
//! Hidden Claude Code hook entrypoints that proxy into adopt and workspace workflows with stable
//! event output.

use std::io::{self, Read, Write};

use frigg::agent_directive::HOOK_NUDGE;
use serde_json::{Value, json};

use crate::cli_args::HookMode;

/// Handles the hidden Claude Code PreToolUse hook and may append Frigg navigation nudges to stdout.
pub(crate) fn run_pretooluse_hook_command<R: Read, W: Write>(
    mut stdin: R,
    mut stdout: W,
    mode: HookMode,
) -> io::Result<()> {
    let mut input = String::new();
    stdin.read_to_string(&mut input)?;
    if let Some(output) = render_pretooluse_hook_output(&input, mode) {
        stdout.write_all(output.as_bytes())?;
        stdout.write_all(b"\n")?;
    }
    Ok(())
}

fn render_pretooluse_hook_output(input: &str, mode: HookMode) -> Option<String> {
    let value = serde_json::from_str::<Value>(input).ok()?;
    should_nudge_pretooluse(&value).then(|| {
        match mode {
            // Advisory only: attach context and let the call through. Never blocks, never
            // auto-approves, so a wrong guess costs the agent nothing but a few tokens.
            HookMode::Nudge => json!({
                "hookSpecificOutput": {
                    "hookEventName": "PreToolUse",
                    "additionalContext": HOOK_NUDGE,
                }
            }),
            // Opt-in enforcement: turn the call into a permission prompt. Still `ask` rather
            // than `deny`, because Frigg cannot serve every case the matcher catches and the
            // user has to stay able to say yes.
            HookMode::Ask => json!({
                "hookSpecificOutput": {
                    "hookEventName": "PreToolUse",
                    "permissionDecision": "ask",
                    "permissionDecisionReason": HOOK_NUDGE,
                    "additionalContext": HOOK_NUDGE,
                }
            }),
        }
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
        // `Glob` is the host's file-finding tool, the case `list_files` replaces.
        Some("Grep" | "Glob") => true,
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
    command_segments(command)
        .iter()
        .any(|segment| segment_is_frigg_replaceable(segment))
}

/// Splits a shell command into per-command word lists on `;`, `|`, `&&`, and newlines.
fn command_segments(command: &str) -> Vec<Vec<&str>> {
    let mut segments = Vec::new();
    let mut current: Vec<&str> = Vec::new();

    for token in ShellTokens::new(command) {
        match token {
            ShellToken::Separator => {
                if !current.is_empty() {
                    segments.push(std::mem::take(&mut current));
                }
            }
            ShellToken::Word(word) => current.push(word),
        }
    }
    if !current.is_empty() {
        segments.push(current);
    }

    segments
}

/// True when one shell command has a Frigg equivalent the agent should have reached for first.
fn segment_is_frigg_replaceable(segment: &[&str]) -> bool {
    // Step past leading `FOO=bar` assignments and wrappers such as `sudo` or `env`.
    let mut index = 0;
    while let Some(word) = segment.get(index) {
        if is_shell_assignment(word) || is_command_wrapper(command_name(word)) {
            index += 1;
            continue;
        }
        break;
    }

    let Some(head) = segment.get(index) else {
        return false;
    };
    let name = command_name(head);
    let arguments = segment.get(index + 1..).unwrap_or(&[]);

    if name == "git" {
        return git_subcommand_is_grep(arguments);
    }
    if is_discovery_command_name(name) {
        return true;
    }
    if is_file_read_command_name(name) {
        return read_command_targets_code(name, arguments);
    }
    false
}

/// True when a `git` invocation is `git grep` (allowing leading global flags).
fn git_subcommand_is_grep(arguments: &[&str]) -> bool {
    for argument in arguments {
        if *argument == "grep" {
            return true;
        }
        if !argument.starts_with('-') {
            return false;
        }
    }
    false
}

fn command_name(word: &str) -> &str {
    word.rsplit('/').next().unwrap_or(word)
}

/// Discovery commands: whole-repo search and file finding, replaced by `search_text`/`list_files`.
fn is_discovery_command_name(command_name: &str) -> bool {
    matches!(
        command_name,
        "grep" | "egrep" | "fgrep" | "rg" | "ripgrep" | "find" | "fd" | "fdfind"
    )
}

/// Read commands, replaced by `read_file`. Only nudged when they target a source path.
fn is_file_read_command_name(command_name: &str) -> bool {
    matches!(command_name, "cat" | "head" | "tail" | "sed")
}

/// True when a read-shaped command names a source file.
///
/// Requiring a code path keeps `cat package.json` and `tail -f server.log` quiet; those are not
/// reads Frigg replaces. In-place `sed` is an edit, and Frigg has no write tool, so it never nudges.
fn read_command_targets_code(command_name: &str, arguments: &[&str]) -> bool {
    if command_name == "sed"
        && arguments
            .iter()
            .any(|argument| is_sed_in_place_flag(argument))
    {
        return false;
    }

    arguments
        .iter()
        .take_while(|argument| !is_redirection_token(argument))
        .any(|argument| {
            let argument = strip_shell_quotes(argument);
            !argument.starts_with('-') && is_code_path(argument)
        })
}

/// True for `>`, `>>`, `<`, and their fused forms such as `>out.rs`.
///
/// Everything from the first redirection onward names a stream target, not a file being read.
/// `cat > src/lib.rs <<'EOF'` writes a file, and Frigg has no write tool, so it must not nudge.
fn is_redirection_token(argument: &str) -> bool {
    argument.starts_with('>') || argument.starts_with('<')
}

/// True for in-place `sed`: `-i`, `-i.bak`, bundled shorts such as `-Ei`, and `--in-place[=SUFFIX]`.
fn is_sed_in_place_flag(argument: &str) -> bool {
    let argument = strip_shell_quotes(argument);
    if argument == "--in-place" || argument.starts_with("--in-place=") {
        return true;
    }
    if argument.starts_with("--") || !argument.starts_with('-') {
        return false;
    }
    // Bundled short options: everything up to an optional suffix is a flag letter, so `-Ei` and
    // `-ni` are in-place just as much as a bare `-i`.
    argument
        .trim_start_matches('-')
        .split('.')
        .next()
        .unwrap_or_default()
        .contains('i')
}

/// Strips one layer of surrounding shell quotes left on a token by `ShellTokens`.
fn strip_shell_quotes(word: &str) -> &str {
    for quote in ['\'', '"'] {
        if let Some(inner) = word.strip_prefix(quote).and_then(|w| w.strip_suffix(quote)) {
            return inner;
        }
    }
    word
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

    use super::{HookMode, render_pretooluse_hook_output, run_pretooluse_hook_command};

    #[test]
    fn pretooluse_grep_emits_hook_specific_nudge() {
        let output = render_pretooluse_hook_output(
            r#"{"hook_event_name":"PreToolUse","tool_name":"Grep","tool_input":{"pattern":"needle"}}"#,
            HookMode::Nudge,
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
            value.get("permissionDecision").is_none(),
            "top-level hook JSON must not include permissionDecision (soft only)"
        );
        let context = hook_output
            .get("additionalContext")
            .and_then(Value::as_str)
            .expect("additionalContext should exist");
        assert_eq!(context, HOOK_NUDGE);
        assert!(
            context.contains("search_text") && context.contains("search_batch"),
            "nudge should teach preferred Frigg tools, not only a slogan"
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
                render_pretooluse_hook_output(&input, HookMode::Nudge).is_some(),
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

        assert!(render_pretooluse_hook_output(&input, HookMode::Nudge).is_some());
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
            assert_eq!(
                render_pretooluse_hook_output(&input.to_string(), HookMode::Nudge),
                None
            );
        }
    }

    /// `Glob` is the host's file-finding tool and the case `list_files` replaces, so it must nudge.
    #[test]
    fn pretooluse_glob_emits_nudge() {
        let input = json!({
            "hook_event_name": "PreToolUse",
            "tool_name": "Glob",
            "tool_input": { "pattern": "**/*.rs" },
        })
        .to_string();

        assert!(render_pretooluse_hook_output(&input, HookMode::Nudge).is_some());
    }

    /// The skill claims `find`/`fd` and the read family, not just the grep family.
    #[test]
    fn pretooluse_bash_file_discovery_and_code_reads_emit_nudge() {
        for command in [
            "find . -name '*.rs'",
            "fd --extension rs",
            "fdfind foo",
            "cat crates/cli/src/main.rs",
            "head -20 crates/cli/src/main.rs",
            "tail -n 5 src/lib.rs",
            "sed -n '10,20p' src/lib.rs",
            "cat \"crates/cli/src/main.rs\"",
            "sudo find /srv -name '*.go'",
            "cargo build && cat src/lib.rs",
            // Reads a source file before redirecting; the read is still the replaceable part.
            "cat src/lib.rs > /tmp/copy.txt",
            // `-n`/`-e` are not in-place, so these stay nudged.
            "sed -e 's/a/b/' src/lib.rs",
        ] {
            let input = json!({
                "hook_event_name": "PreToolUse",
                "tool_name": "Bash",
                "tool_input": { "command": command },
            })
            .to_string();

            assert!(
                render_pretooluse_hook_output(&input, HookMode::Nudge).is_some(),
                "expected nudge for command: {command}"
            );
        }
    }

    /// Read-shaped commands only nudge on source paths, and in-place `sed` is an edit Frigg
    /// cannot serve at all, so neither should produce advisory context.
    #[test]
    fn pretooluse_bash_non_code_reads_and_in_place_edits_are_silent() {
        for command in [
            "cat package.json",
            "tail -f /var/log/server.log",
            "head -3 data.csv",
            "sed -i 's/a/b/' src/lib.rs",
            "sed -i.bak 's/a/b/' src/lib.rs",
            "sed --in-place 's/a/b/' src/lib.rs",
            "cat",
            // Bundled short flags are in-place too.
            "sed -Ei 's/a/b/' src/lib.rs",
            "sed -ni 's/a/b/' src/lib.rs",
            "sed -i'.bak' 's/a/b/' src/lib.rs",
            // Redirection writes a file; Frigg has no write tool.
            "cat > src/lib.rs",
            "cat >> src/lib.rs",
            "cat >src/lib.rs",
        ] {
            let input = json!({
                "hook_event_name": "PreToolUse",
                "tool_name": "Bash",
                "tool_input": { "command": command },
            })
            .to_string();

            assert_eq!(
                render_pretooluse_hook_output(&input, HookMode::Nudge),
                None,
                "expected silence for command: {command}"
            );
        }
    }

    #[test]
    fn pretooluse_invalid_or_empty_input_returns_success_without_stdout() {
        for input in ["", "not json", r#"{"tool_name":12}"#] {
            let mut stdout = Vec::new();

            run_pretooluse_hook_command(input.as_bytes(), &mut stdout, HookMode::Nudge)
                .expect("invalid hook input should not fail");

            assert!(stdout.is_empty(), "stdout should be empty for {input:?}");
        }
    }

    /// `ask` turns the call into a permission prompt. Still `ask`, never `deny`: Frigg cannot
    /// serve every case the matcher catches, so the user has to stay able to say yes.
    #[test]
    fn ask_mode_requests_permission_and_keeps_the_reason() {
        let input = json!({
            "hook_event_name": "PreToolUse",
            "tool_name": "Grep",
            "tool_input": { "pattern": "needle" },
        })
        .to_string();

        let output = render_pretooluse_hook_output(&input, HookMode::Ask).expect("ask output");
        let value = serde_json::from_str::<Value>(&output).expect("hook output should be json");
        let hook_output = value["hookSpecificOutput"].clone();

        assert_eq!(
            hook_output["permissionDecision"].as_str(),
            Some("ask"),
            "ask mode must prompt, not decide for the user"
        );
        assert_ne!(
            hook_output["permissionDecision"].as_str(),
            Some("deny"),
            "frigg must never block a tool call outright"
        );
        assert_eq!(
            hook_output["permissionDecisionReason"].as_str(),
            Some(HOOK_NUDGE),
            "the prompt should say which Frigg tool replaces the call"
        );
    }

    /// The default stays advisory, so an install that never opts in cannot start prompting.
    #[test]
    fn nudge_mode_never_emits_a_permission_decision() {
        let input = json!({
            "hook_event_name": "PreToolUse",
            "tool_name": "Glob",
            "tool_input": { "pattern": "**/*.rs" },
        })
        .to_string();

        let output = render_pretooluse_hook_output(&input, HookMode::Nudge).expect("nudge output");
        let value = serde_json::from_str::<Value>(&output).expect("hook output should be json");

        assert!(
            value["hookSpecificOutput"]["permissionDecision"].is_null(),
            "nudge must stay advisory: {output}"
        );
    }

    /// A call Frigg cannot serve is silent in every mode.
    #[test]
    fn ask_mode_stays_silent_for_calls_frigg_does_not_replace() {
        let input = json!({
            "hook_event_name": "PreToolUse",
            "tool_name": "Bash",
            "tool_input": { "command": "cargo test" },
        })
        .to_string();

        assert_eq!(render_pretooluse_hook_output(&input, HookMode::Ask), None);
    }
}
