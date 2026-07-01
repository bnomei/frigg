//! Read-only cache fingerprint command for CI/install workflows.

use std::env;
use std::fs::OpenOptions;
use std::io::{self, Write};
use std::path::Path;

use frigg::searcher::stable_cache_fingerprint_hex;

const HASH_OUTPUT_KEY: &str = "frigg-hash";

pub(crate) fn run_hash_command() -> io::Result<()> {
    let github_output = env::var_os("GITHUB_OUTPUT");
    run_hash_command_with_output(github_output.as_deref().map(Path::new), io::stdout())
}

pub(crate) fn run_hash_command_with_output<W: Write>(
    github_output: Option<&Path>,
    mut stdout: W,
) -> io::Result<()> {
    let line = hash_output_line();
    writeln!(stdout, "{line}")?;
    if let Some(path) = github_output {
        let mut output = OpenOptions::new().create(true).append(true).open(path)?;
        writeln!(output, "{line}")?;
    }
    Ok(())
}

fn hash_output_line() -> String {
    format!("{HASH_OUTPUT_KEY}={}", stable_cache_fingerprint_hex())
}

#[cfg(test)]
mod tests {
    use std::fs;
    use std::path::PathBuf;
    use std::time::{SystemTime, UNIX_EPOCH};

    use clap::Parser;

    use super::run_hash_command_with_output;
    use crate::cli_runtime::resolve_command_config;
    use crate::{Cli, Command};

    #[test]
    fn hash_command_stdout_contains_exactly_one_machine_line() {
        let mut stdout = Vec::new();

        run_hash_command_with_output(None, &mut stdout).expect("hash command should run");

        let output = String::from_utf8(stdout).expect("stdout should be utf-8");
        let lines = output.lines().collect::<Vec<_>>();
        assert_eq!(lines.len(), 1, "stdout must contain exactly one line");
        assert!(
            lines[0].starts_with("frigg-hash="),
            "unexpected hash output: {output:?}"
        );
        assert!(
            lines[0]["frigg-hash=".len()..]
                .chars()
                .all(|character| character.is_ascii_hexdigit()),
            "hash value must be hex: {output:?}"
        );
    }

    #[test]
    fn hash_command_writes_same_line_to_github_output() {
        let output_path = temp_output_path("frigg-hash-github-output");
        fs::write(&output_path, "existing=value\n").expect("seed output file");
        let mut stdout = Vec::new();

        run_hash_command_with_output(Some(&output_path), &mut stdout)
            .expect("hash command should append github output");

        let stdout = String::from_utf8(stdout).expect("stdout should be utf-8");
        let output_file = fs::read_to_string(&output_path).expect("read github output file");
        let appended = output_file
            .lines()
            .last()
            .expect("github output should contain appended line");
        assert_eq!(stdout.trim_end(), appended);
        assert_eq!(
            output_file
                .lines()
                .filter(|line| line.starts_with("frigg-hash="))
                .count(),
            1
        );

        fs::remove_file(output_path).expect("remove temp output file");
    }

    #[test]
    fn hash_command_config_resolution_allows_empty_workspace_roots() {
        let cli = Cli::try_parse_from(["frigg", "hash"]).expect("hash command should parse");
        assert!(cli.workspace_roots.is_empty());

        let config = resolve_command_config(&cli, Command::Hash)
            .expect("hash command config should not require workspace roots");

        assert!(config.workspace_roots.is_empty());
    }

    fn temp_output_path(stem: &str) -> PathBuf {
        let unique = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .expect("system time should be after epoch")
            .as_nanos();
        std::env::temp_dir().join(format!("{stem}-{unique}.txt"))
    }
}
