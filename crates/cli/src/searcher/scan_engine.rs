use std::fs;

use memchr::memchr_iter;
use smallvec::SmallVec;

use crate::domain::{FriggResult, model::TextMatch};

use super::{
    BOUNDED_SEARCH_RESULT_LIMIT_THRESHOLD, SearchCandidateUniverse, SearchDiagnostic,
    SearchDiagnosticKind, SearchExecutionOutput, SearchLexicalBackend, SearchTextQuery,
    content_scrub::{scrub_search_content, should_scrub_leading_markdown_comment},
    ordering::BoundedTextMatches,
    sort_search_diagnostics_deterministically, text_match_candidate_order,
};

pub(super) type MatchColumnsBuffer = SmallVec<[usize; 8]>;

pub(super) fn search_with_streaming_lines_in_universe<F>(
    query: &SearchTextQuery,
    candidate_universe: &SearchCandidateUniverse,
    mut match_columns: F,
) -> FriggResult<SearchExecutionOutput>
where
    F: FnMut(&str, &mut MatchColumnsBuffer),
{
    if query.limit == 0 {
        return Ok(SearchExecutionOutput::default());
    }

    let use_bounded_retention = query.limit <= BOUNDED_SEARCH_RESULT_LIMIT_THRESHOLD;
    let mut matches = BoundedTextMatches::with_limit(query.limit, use_bounded_retention);
    let mut total_matches = 0usize;
    let mut diagnostics = candidate_universe.diagnostics.clone();
    let mut match_columns_buffer = MatchColumnsBuffer::new();

    for repository in &candidate_universe.repositories {
        for candidate in &repository.candidates {
            let repository_id = &repository.repository_id;
            let rel_path = &candidate.relative_path;
            let path = &candidate.absolute_path;
            if query
                .path_regex
                .as_ref()
                .is_some_and(|path_regex| !path_regex.is_match(rel_path))
            {
                continue;
            }

            let Some(bytes) =
                load_searchable_bytes(repository_id, rel_path, path, &mut diagnostics.entries)
            else {
                continue;
            };

            let outcome = collect_line_matches(
                repository_id,
                rel_path,
                &bytes,
                &mut matches,
                &mut total_matches,
                &mut match_columns_buffer,
                &mut match_columns,
                use_bounded_retention,
                false,
            );
            if let Err(err) = outcome {
                diagnostics.entries.push(SearchDiagnostic {
                    repository_id: repository_id.clone(),
                    path: Some(rel_path.clone()),
                    kind: SearchDiagnosticKind::Read,
                    message: err,
                });
            }
        }
    }

    sort_search_diagnostics_deterministically(&mut diagnostics.entries);
    let matches = matches.into_final_matches(query.limit);

    Ok(SearchExecutionOutput {
        total_matches,
        matches,
        diagnostics,
        lexical_backend: Some(SearchLexicalBackend::Native),
        lexical_backend_note: None,
    })
}

pub(super) fn search_with_streaming_lines_prefix_in_universe<F>(
    query: &SearchTextQuery,
    candidate_universe: &SearchCandidateUniverse,
    mut match_columns: F,
) -> FriggResult<SearchExecutionOutput>
where
    F: FnMut(&str, &mut MatchColumnsBuffer),
{
    if query.limit == 0 {
        return Ok(SearchExecutionOutput::default());
    }

    let mut matches = BoundedTextMatches::with_limit(query.limit, true);
    let mut total_matches = 0usize;
    let mut diagnostics = candidate_universe.diagnostics.clone();
    let mut match_columns_buffer = MatchColumnsBuffer::new();

    'repositories: for repository in &candidate_universe.repositories {
        for candidate in &repository.candidates {
            let repository_id = &repository.repository_id;
            let rel_path = &candidate.relative_path;
            let path = &candidate.absolute_path;
            if query
                .path_regex
                .as_ref()
                .is_some_and(|path_regex| !path_regex.is_match(rel_path))
            {
                continue;
            }

            let Some(bytes) =
                load_searchable_bytes(repository_id, rel_path, path, &mut diagnostics.entries)
            else {
                continue;
            };

            match collect_line_matches(
                repository_id,
                rel_path,
                &bytes,
                &mut matches,
                &mut total_matches,
                &mut match_columns_buffer,
                &mut match_columns,
                false,
                true,
            ) {
                Ok(true) => break 'repositories,
                Ok(false) => {}
                Err(err) => diagnostics.entries.push(SearchDiagnostic {
                    repository_id: repository_id.clone(),
                    path: Some(rel_path.clone()),
                    kind: SearchDiagnosticKind::Read,
                    message: err,
                }),
            }
        }
    }

    sort_search_diagnostics_deterministically(&mut diagnostics.entries);
    let matches = matches.into_final_matches(query.limit);

    Ok(SearchExecutionOutput {
        total_matches,
        matches,
        diagnostics,
        lexical_backend: Some(SearchLexicalBackend::Native),
        lexical_backend_note: None,
    })
}

pub(super) fn search_with_matcher_in_universe<F, P>(
    query: &SearchTextQuery,
    candidate_universe: &SearchCandidateUniverse,
    mut file_may_match: P,
    mut match_columns: F,
) -> FriggResult<SearchExecutionOutput>
where
    P: FnMut(&str) -> bool,
    F: FnMut(&str, &mut MatchColumnsBuffer),
{
    let use_bounded_retention = query.limit <= BOUNDED_SEARCH_RESULT_LIMIT_THRESHOLD;
    let mut matches = BoundedTextMatches::with_limit(query.limit, use_bounded_retention);
    let mut total_matches = 0usize;
    let mut diagnostics = candidate_universe.diagnostics.clone();
    let mut match_columns_buffer = MatchColumnsBuffer::new();
    for repository in &candidate_universe.repositories {
        for candidate in &repository.candidates {
            let repository_id = &repository.repository_id;
            let rel_path = &candidate.relative_path;
            let path = &candidate.absolute_path;
            if query
                .path_regex
                .as_ref()
                .is_some_and(|path_regex| !path_regex.is_match(rel_path))
            {
                continue;
            }
            let content = match fs::read_to_string(path) {
                Ok(content) => content,
                Err(err) => {
                    diagnostics.entries.push(SearchDiagnostic {
                        repository_id: repository_id.clone(),
                        path: Some(rel_path.clone()),
                        kind: SearchDiagnosticKind::Read,
                        message: err.to_string(),
                    });
                    continue;
                }
            };
            let content = scrub_search_content(rel_path, &content);
            if !file_may_match(content.as_ref()) {
                continue;
            }

            let outcome = collect_line_matches(
                repository_id,
                rel_path,
                content.as_bytes(),
                &mut matches,
                &mut total_matches,
                &mut match_columns_buffer,
                &mut match_columns,
                use_bounded_retention,
                false,
            );
            if let Err(err) = outcome {
                diagnostics.entries.push(SearchDiagnostic {
                    repository_id: repository_id.clone(),
                    path: Some(rel_path.clone()),
                    kind: SearchDiagnosticKind::Read,
                    message: err,
                });
            }
        }
    }

    sort_search_diagnostics_deterministically(&mut diagnostics.entries);
    let matches = matches.into_final_matches(query.limit);

    Ok(SearchExecutionOutput {
        total_matches,
        matches,
        diagnostics,
        lexical_backend: Some(SearchLexicalBackend::Native),
        lexical_backend_note: None,
    })
}

fn load_searchable_bytes(
    repository_id: &str,
    rel_path: &str,
    path: &std::path::Path,
    diagnostics: &mut Vec<SearchDiagnostic>,
) -> Option<Vec<u8>> {
    if should_scrub_leading_markdown_comment(rel_path) {
        let content = match fs::read_to_string(path) {
            Ok(content) => content,
            Err(err) => {
                diagnostics.push(SearchDiagnostic {
                    repository_id: repository_id.to_owned(),
                    path: Some(rel_path.to_owned()),
                    kind: SearchDiagnosticKind::Read,
                    message: err.to_string(),
                });
                return None;
            }
        };
        return Some(
            scrub_search_content(rel_path, &content)
                .into_owned()
                .into_bytes(),
        );
    }

    match fs::read(path) {
        Ok(bytes) => Some(bytes),
        Err(err) => {
            diagnostics.push(SearchDiagnostic {
                repository_id: repository_id.to_owned(),
                path: Some(rel_path.to_owned()),
                kind: SearchDiagnosticKind::Read,
                message: err.to_string(),
            });
            None
        }
    }
}

#[allow(clippy::too_many_arguments)]
fn collect_line_matches<F>(
    repository_id: &str,
    rel_path: &str,
    bytes: &[u8],
    matches: &mut BoundedTextMatches,
    total_matches: &mut usize,
    match_columns_buffer: &mut MatchColumnsBuffer,
    match_columns: &mut F,
    use_bounded_retention: bool,
    stop_on_non_improving_prefix: bool,
) -> Result<bool, String>
where
    F: FnMut(&str, &mut MatchColumnsBuffer),
{
    let mut should_stop = false;
    for_each_utf8_line(bytes, |line_number, line| {
        match_columns(line, match_columns_buffer);
        if match_columns_buffer.is_empty() {
            return true;
        }

        let mut excerpt_for_line: Option<String> = None;
        for &column in match_columns_buffer.iter() {
            *total_matches = total_matches.saturating_add(1);
            let is_non_improving = matches.is_full()
                && matches.worst().is_some_and(|worst| {
                    !text_match_candidate_order(
                        repository_id,
                        rel_path,
                        line_number,
                        column,
                        line,
                        worst,
                    )
                    .is_lt()
                });
            if is_non_improving {
                if stop_on_non_improving_prefix {
                    should_stop = true;
                    break;
                }
                if use_bounded_retention {
                    continue;
                }
            }

            let candidate = TextMatch {
                repository_id: repository_id.to_owned(),
                path: rel_path.to_owned(),
                line: line_number,
                column,
                excerpt: excerpt_for_line
                    .get_or_insert_with(|| line.to_owned())
                    .clone(),
                witness_score_hint_millis: None,
                witness_provenance_ids: None,
            };
            matches.push(candidate);
        }

        !should_stop
    })?;
    Ok(should_stop)
}

fn for_each_utf8_line<F>(bytes: &[u8], mut visit: F) -> Result<(), String>
where
    F: FnMut(usize, &str) -> bool,
{
    let mut line_start = 0usize;
    let mut line_number = 0usize;

    for newline_index in memchr_iter(b'\n', bytes) {
        line_number = line_number.saturating_add(1);
        let mut line_bytes = &bytes[line_start..newline_index];
        if line_bytes.ends_with(b"\r") {
            line_bytes = &line_bytes[..line_bytes.len().saturating_sub(1)];
        }
        let line = std::str::from_utf8(line_bytes).map_err(|err| err.to_string())?;
        if !visit(line_number, line) {
            return Ok(());
        }
        line_start = newline_index.saturating_add(1);
    }

    if line_start < bytes.len() {
        line_number = line_number.saturating_add(1);
        let line = std::str::from_utf8(&bytes[line_start..]).map_err(|err| err.to_string())?;
        let _ = visit(line_number, line);
    }

    Ok(())
}
