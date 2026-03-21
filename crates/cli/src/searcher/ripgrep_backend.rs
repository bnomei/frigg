use std::collections::{BTreeMap, BTreeSet};
use std::ffi::OsString;
use std::io::{BufRead, BufReader};
use std::path::PathBuf;
use std::process::{Command, Stdio};
use std::sync::{OnceLock, RwLock};

use serde_json::Value;

use crate::domain::{FriggError, FriggResult, model::TextMatch};
use crate::settings::{LexicalBackendMode, LexicalRuntimeConfig};

use super::{
    SearchCandidateUniverse, SearchDiagnostic, SearchDiagnosticKind, SearchExecutionDiagnostics,
    SearchExecutionOutput, SearchLexicalBackend, SearchTextQuery, sort_matches_deterministically,
    sort_search_diagnostics_deterministically,
};

const RIPGREP_BATCH_ARG_BYTES_LIMIT: usize = 96 * 1024;
const RIPGREP_BATCH_FILE_LIMIT: usize = 512;

static RIPGREP_AVAILABILITY_CACHE: OnceLock<RwLock<BTreeMap<String, RipgrepAvailability>>> =
    OnceLock::new();

#[derive(Debug, Clone)]
pub(super) struct RipgrepExecutable {
    program: OsString,
    display: String,
    version: String,
}

#[derive(Debug, Clone)]
enum RipgrepAvailability {
    Available(RipgrepExecutable),
    Unavailable(String),
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) enum RipgrepPatternMode {
    Literal,
    Regex,
}

impl RipgrepPatternMode {
    fn as_note(self) -> &'static str {
        match self {
            Self::Literal => "literal",
            Self::Regex => "regex",
        }
    }
}

pub(super) fn resolve_ripgrep_executable(
    lexical_runtime: &LexicalRuntimeConfig,
) -> Result<Option<RipgrepExecutable>, String> {
    if lexical_runtime.backend == LexicalBackendMode::Native {
        return Ok(None);
    }

    let program = lexical_runtime
        .ripgrep_executable
        .clone()
        .map(OsString::from)
        .unwrap_or_else(|| OsString::from("rg"));
    let key = lexical_runtime
        .ripgrep_executable
        .as_ref()
        .map(|path| format!("path:{}", path.display()))
        .unwrap_or_else(|| "path:rg".to_owned());
    let cache = RIPGREP_AVAILABILITY_CACHE.get_or_init(|| RwLock::new(BTreeMap::new()));

    if let Some(cached) = cache
        .read()
        .unwrap_or_else(|poisoned| poisoned.into_inner())
        .get(&key)
        .cloned()
    {
        return match cached {
            RipgrepAvailability::Available(executable) => Ok(Some(executable)),
            RipgrepAvailability::Unavailable(reason) => Err(reason),
        };
    }

    let output = Command::new(&program)
        .arg("--version")
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .output()
        .map_err(|err| err.to_string())?;
    if !output.status.success() {
        let reason = String::from_utf8_lossy(&output.stderr).trim().to_owned();
        let reason = if reason.is_empty() {
            format!(
                "ripgrep version probe failed for {}",
                program.to_string_lossy()
            )
        } else {
            reason
        };
        cache
            .write()
            .unwrap_or_else(|poisoned| poisoned.into_inner())
            .insert(key, RipgrepAvailability::Unavailable(reason.clone()));
        return Err(reason);
    }

    let version = String::from_utf8_lossy(&output.stdout)
        .lines()
        .next()
        .unwrap_or("ripgrep")
        .trim()
        .to_owned();
    let executable = RipgrepExecutable {
        display: program.to_string_lossy().into_owned(),
        program,
        version,
    };
    cache
        .write()
        .unwrap_or_else(|poisoned| poisoned.into_inner())
        .insert(key, RipgrepAvailability::Available(executable.clone()));
    Ok(Some(executable))
}

#[cfg(test)]
pub(crate) fn clear_ripgrep_availability_cache() {
    if let Some(cache) = RIPGREP_AVAILABILITY_CACHE.get() {
        cache
            .write()
            .unwrap_or_else(|poisoned| poisoned.into_inner())
            .clear();
    }
}

pub(super) fn search_with_ripgrep_in_universe(
    executable: &RipgrepExecutable,
    query: &SearchTextQuery,
    candidate_universe: &SearchCandidateUniverse,
    mode: RipgrepPatternMode,
) -> FriggResult<SearchExecutionOutput> {
    let mut matches = Vec::new();
    let mut diagnostics = candidate_universe.diagnostics.clone();
    let mut total_matches = 0usize;

    for repository in &candidate_universe.repositories {
        let filtered_paths = repository
            .candidates
            .iter()
            .filter(|candidate| {
                query
                    .path_regex
                    .as_ref()
                    .is_none_or(|path_regex| path_regex.is_match(&candidate.relative_path))
            })
            .map(|candidate| candidate.relative_path.clone())
            .collect::<Vec<_>>();
        if filtered_paths.is_empty() {
            continue;
        }

        for batch in batch_candidate_paths(&filtered_paths) {
            let mut output =
                run_ripgrep_batch(executable, &repository.root, &query.query, batch, mode)
                    .map_err(|err| {
                        FriggError::Internal(format!(
                            "ripgrep {} execution failed for {}: {err}",
                            mode.as_note(),
                            repository.root.display()
                        ))
                    })?;
            for matched in &mut output.matches {
                matched.repository_id = repository.repository_id.clone();
            }
            total_matches = total_matches.saturating_add(output.total_matches);
            matches.extend(output.matches);
            diagnostics.entries.extend(output.diagnostics.entries);
        }
    }

    sort_matches_deterministically(&mut matches);
    matches.truncate(query.limit);
    sort_search_diagnostics_deterministically(&mut diagnostics.entries);
    diagnostics.entries.dedup();

    Ok(SearchExecutionOutput {
        total_matches,
        matches,
        diagnostics,
        lexical_backend: Some(SearchLexicalBackend::Ripgrep),
        lexical_backend_note: Some(format!(
            "ripgrep accelerator active ({}) via {}",
            executable.version, executable.display
        )),
    })
}

fn batch_candidate_paths(paths: &[String]) -> Vec<Vec<String>> {
    let mut batches = Vec::new();
    let mut current = Vec::new();
    let mut current_bytes = 0usize;

    for path in paths {
        let next_bytes = current_bytes.saturating_add(path.len()).saturating_add(1);
        if !current.is_empty()
            && (current.len() >= RIPGREP_BATCH_FILE_LIMIT
                || next_bytes > RIPGREP_BATCH_ARG_BYTES_LIMIT)
        {
            batches.push(current);
            current = Vec::new();
            current_bytes = 0;
        }
        current_bytes = current_bytes.saturating_add(path.len()).saturating_add(1);
        current.push(path.clone());
    }

    if !current.is_empty() {
        batches.push(current);
    }

    batches
}

fn run_ripgrep_batch(
    executable: &RipgrepExecutable,
    root: &PathBuf,
    query: &str,
    candidate_paths: Vec<String>,
    mode: RipgrepPatternMode,
) -> Result<SearchExecutionOutput, String> {
    let allowed_paths = candidate_paths.iter().cloned().collect::<BTreeSet<_>>();
    let mut command = Command::new(&executable.program);
    command
        .current_dir(root)
        .arg("--json")
        .arg("--color")
        .arg("never")
        .arg("--line-number");
    if mode == RipgrepPatternMode::Literal {
        command.arg("--fixed-strings");
    }
    command.arg("-e").arg(query).arg("--");
    for path in &candidate_paths {
        command.arg(path);
    }
    command.stdout(Stdio::piped()).stderr(Stdio::piped());

    let mut child = command.spawn().map_err(|err| err.to_string())?;
    let stdout = child
        .stdout
        .take()
        .ok_or_else(|| "ripgrep did not expose stdout".to_owned())?;
    let reader = BufReader::new(stdout);
    let mut matches = Vec::new();
    let mut diagnostics = SearchExecutionDiagnostics::default();

    for line in reader.lines() {
        let line = line.map_err(|err| err.to_string())?;
        if line.trim().is_empty() {
            continue;
        }
        parse_ripgrep_event_line(&line, &mut matches)?;
    }

    let output = child.wait_with_output().map_err(|err| err.to_string())?;
    match output.status.code() {
        Some(0 | 1) => {}
        Some(code) => {
            let stderr = String::from_utf8_lossy(&output.stderr).trim().to_owned();
            let message = if stderr.is_empty() {
                format!("ripgrep exited with status {code}")
            } else {
                stderr
            };
            diagnostics.entries.push(SearchDiagnostic {
                repository_id: String::new(),
                path: None,
                kind: SearchDiagnosticKind::Read,
                message: message.clone(),
            });
            return Err(message);
        }
        None => return Err("ripgrep terminated by signal".to_owned()),
    }

    matches.retain(|matched| allowed_paths.contains(&matched.path));
    let total_matches = matches.len();

    Ok(SearchExecutionOutput {
        total_matches,
        matches,
        diagnostics,
        lexical_backend: Some(SearchLexicalBackend::Ripgrep),
        lexical_backend_note: Some(format!(
            "ripgrep accelerator active ({}) via {}",
            executable.version, executable.display
        )),
    })
}

fn parse_ripgrep_event_line(line: &str, matches: &mut Vec<TextMatch>) -> Result<(), String> {
    let value: Value = serde_json::from_str(line).map_err(|err| err.to_string())?;
    if value.get("type").and_then(Value::as_str) != Some("match") {
        return Ok(());
    }

    let data = value
        .get("data")
        .and_then(Value::as_object)
        .ok_or_else(|| "ripgrep match event missing data".to_owned())?;
    let path = data
        .get("path")
        .and_then(extract_ripgrep_text)
        .ok_or_else(|| "ripgrep match event missing path".to_owned())?;
    let line_number =
        data.get("line_number")
            .and_then(Value::as_u64)
            .ok_or_else(|| "ripgrep match event missing line_number".to_owned())? as usize;
    let excerpt = data
        .get("lines")
        .and_then(extract_ripgrep_text)
        .unwrap_or_default()
        .trim_end_matches(['\r', '\n'])
        .to_owned();
    let submatches = data
        .get("submatches")
        .and_then(Value::as_array)
        .ok_or_else(|| "ripgrep match event missing submatches".to_owned())?;

    for submatch in submatches {
        let start = submatch
            .get("start")
            .and_then(Value::as_u64)
            .ok_or_else(|| "ripgrep submatch missing start".to_owned())?
            as usize;
        matches.push(TextMatch {
            match_id: None,
            repository_id: String::new(),
            path: normalize_ripgrep_path(&path),
            line: line_number,
            column: start.saturating_add(1),
            excerpt: excerpt.clone(),
            witness_score_hint_millis: None,
            witness_provenance_ids: None,
        });
    }

    Ok(())
}

fn extract_ripgrep_text(value: &Value) -> Option<String> {
    value
        .get("text")
        .and_then(Value::as_str)
        .map(ToOwned::to_owned)
        .or_else(|| {
            value
                .get("bytes")
                .and_then(Value::as_str)
                .map(ToOwned::to_owned)
        })
}

fn normalize_ripgrep_path(raw: &str) -> String {
    raw.strip_prefix("./").unwrap_or(raw).to_owned()
}
