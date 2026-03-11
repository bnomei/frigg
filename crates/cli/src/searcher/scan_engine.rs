use std::fs;
use std::io::{BufRead, BufReader};

use crate::domain::{FriggResult, model::TextMatch};

use super::{
    BOUNDED_SEARCH_RESULT_LIMIT_THRESHOLD, SearchCandidateUniverse, SearchDiagnostic,
    SearchDiagnosticKind, SearchExecutionOutput, SearchTextQuery, retain_bounded_match,
    scrub_search_content, should_scrub_leading_markdown_comment, sort_matches_deterministically,
    sort_search_diagnostics_deterministically, text_match_candidate_order,
};

pub(super) fn search_with_streaming_lines_in_universe<F>(
    query: &SearchTextQuery,
    candidate_universe: &SearchCandidateUniverse,
    mut match_columns: F,
) -> FriggResult<SearchExecutionOutput>
where
    F: FnMut(&str, &mut Vec<usize>),
{
    let use_bounded_retention = query.limit <= BOUNDED_SEARCH_RESULT_LIMIT_THRESHOLD;
    let mut matches = if use_bounded_retention {
        Vec::with_capacity(query.limit)
    } else {
        Vec::new()
    };
    let mut total_matches = 0usize;
    let mut diagnostics = candidate_universe.diagnostics.clone();
    let mut match_columns_buffer = Vec::new();
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
            if should_scrub_leading_markdown_comment(&rel_path) {
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

                for (line_idx, line) in content.lines().enumerate() {
                    match_columns(line, &mut match_columns_buffer);
                    if match_columns_buffer.is_empty() {
                        continue;
                    }

                    let line_number = line_idx + 1;
                    let mut excerpt_for_line: Option<String> = None;

                    for &column in &match_columns_buffer {
                        total_matches = total_matches.saturating_add(1);
                        if use_bounded_retention
                            && matches.len() == query.limit
                            && matches.last().is_some_and(|worst| {
                                !text_match_candidate_order(
                                    &repository_id,
                                    &rel_path,
                                    line_number,
                                    column,
                                    line,
                                    worst,
                                )
                                .is_lt()
                            })
                        {
                            continue;
                        }

                        let candidate = TextMatch {
                            repository_id: repository_id.clone(),
                            path: rel_path.clone(),
                            line: line_number,
                            column,
                            excerpt: excerpt_for_line
                                .get_or_insert_with(|| line.to_owned())
                                .clone(),
                        };

                        if use_bounded_retention {
                            retain_bounded_match(&mut matches, query.limit, candidate);
                        } else {
                            matches.push(candidate);
                        }
                    }
                }
                continue;
            }

            let file = match fs::File::open(path) {
                Ok(file) => file,
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
            let mut reader = BufReader::new(file);
            let mut line = String::new();
            let mut line_number = 0usize;

            loop {
                line.clear();
                match reader.read_line(&mut line) {
                    Ok(0) => break,
                    Ok(_) => {
                        line_number = line_number.saturating_add(1);
                    }
                    Err(err) => {
                        diagnostics.entries.push(SearchDiagnostic {
                            repository_id: repository_id.clone(),
                            path: Some(rel_path.clone()),
                            kind: SearchDiagnosticKind::Read,
                            message: err.to_string(),
                        });
                        break;
                    }
                }

                trim_trailing_newline(&mut line);
                match_columns(&line, &mut match_columns_buffer);
                if match_columns_buffer.is_empty() {
                    continue;
                }

                let mut excerpt_for_line: Option<String> = None;
                for &column in &match_columns_buffer {
                    total_matches = total_matches.saturating_add(1);
                    if use_bounded_retention
                        && matches.len() == query.limit
                        && matches.last().is_some_and(|worst| {
                            !text_match_candidate_order(
                                &repository_id,
                                &rel_path,
                                line_number,
                                column,
                                &line,
                                worst,
                            )
                            .is_lt()
                        })
                    {
                        continue;
                    }

                    let candidate = TextMatch {
                        repository_id: repository_id.clone(),
                        path: rel_path.clone(),
                        line: line_number,
                        column,
                        excerpt: excerpt_for_line.get_or_insert_with(|| line.clone()).clone(),
                    };

                    if use_bounded_retention {
                        retain_bounded_match(&mut matches, query.limit, candidate);
                    } else {
                        matches.push(candidate);
                    }
                }
            }
        }
    }

    sort_search_diagnostics_deterministically(&mut diagnostics.entries);

    if use_bounded_retention {
        return Ok(SearchExecutionOutput {
            total_matches,
            matches,
            diagnostics,
        });
    }

    sort_matches_deterministically(&mut matches);
    matches.truncate(query.limit);

    Ok(SearchExecutionOutput {
        total_matches,
        matches,
        diagnostics,
    })
}

pub(super) fn search_with_streaming_lines_prefix_in_universe<F>(
    query: &SearchTextQuery,
    candidate_universe: &SearchCandidateUniverse,
    mut match_columns: F,
) -> FriggResult<SearchExecutionOutput>
where
    F: FnMut(&str, &mut Vec<usize>),
{
    if query.limit == 0 {
        return Ok(SearchExecutionOutput::default());
    }

    let mut matches = Vec::with_capacity(query.limit);
    let mut total_matches = 0usize;
    let mut diagnostics = candidate_universe.diagnostics.clone();
    let mut match_columns_buffer = Vec::new();
    let mut stop_after_prefix = false;

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
            if should_scrub_leading_markdown_comment(rel_path) {
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

                for (line_idx, line) in content.lines().enumerate() {
                    match_columns(line, &mut match_columns_buffer);
                    if match_columns_buffer.is_empty() {
                        continue;
                    }

                    let line_number = line_idx + 1;
                    let mut excerpt_for_line: Option<String> = None;

                    for &column in &match_columns_buffer {
                        total_matches = total_matches.saturating_add(1);
                        if matches.len() == query.limit
                            && matches.last().is_some_and(|worst| {
                                !text_match_candidate_order(
                                    repository_id,
                                    rel_path,
                                    line_number,
                                    column,
                                    line,
                                    worst,
                                )
                                .is_lt()
                            })
                        {
                            stop_after_prefix = true;
                            break;
                        }

                        let candidate = TextMatch {
                            repository_id: repository_id.clone(),
                            path: rel_path.clone(),
                            line: line_number,
                            column,
                            excerpt: excerpt_for_line
                                .get_or_insert_with(|| line.to_owned())
                                .clone(),
                        };
                        retain_bounded_match(&mut matches, query.limit, candidate);
                    }

                    if stop_after_prefix {
                        break 'repositories;
                    }
                }
                continue;
            }

            let file = match fs::File::open(path) {
                Ok(file) => file,
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
            let mut reader = BufReader::new(file);
            let mut line = String::new();
            let mut line_number = 0usize;

            loop {
                line.clear();
                match reader.read_line(&mut line) {
                    Ok(0) => break,
                    Ok(_) => {
                        line_number = line_number.saturating_add(1);
                    }
                    Err(err) => {
                        diagnostics.entries.push(SearchDiagnostic {
                            repository_id: repository_id.clone(),
                            path: Some(rel_path.clone()),
                            kind: SearchDiagnosticKind::Read,
                            message: err.to_string(),
                        });
                        break;
                    }
                }

                trim_trailing_newline(&mut line);
                match_columns(&line, &mut match_columns_buffer);
                if match_columns_buffer.is_empty() {
                    continue;
                }

                let mut excerpt_for_line: Option<String> = None;
                for &column in &match_columns_buffer {
                    total_matches = total_matches.saturating_add(1);
                    if matches.len() == query.limit
                        && matches.last().is_some_and(|worst| {
                            !text_match_candidate_order(
                                repository_id,
                                rel_path,
                                line_number,
                                column,
                                &line,
                                worst,
                            )
                            .is_lt()
                        })
                    {
                        stop_after_prefix = true;
                        break;
                    }

                    let candidate = TextMatch {
                        repository_id: repository_id.clone(),
                        path: rel_path.clone(),
                        line: line_number,
                        column,
                        excerpt: excerpt_for_line.get_or_insert_with(|| line.clone()).clone(),
                    };
                    retain_bounded_match(&mut matches, query.limit, candidate);
                }

                if stop_after_prefix {
                    break 'repositories;
                }
            }
        }
    }

    sort_search_diagnostics_deterministically(&mut diagnostics.entries);

    Ok(SearchExecutionOutput {
        total_matches,
        matches,
        diagnostics,
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
    F: FnMut(&str, &mut Vec<usize>),
{
    let use_bounded_retention = query.limit <= BOUNDED_SEARCH_RESULT_LIMIT_THRESHOLD;
    let mut matches = if use_bounded_retention {
        Vec::with_capacity(query.limit)
    } else {
        Vec::new()
    };
    let mut total_matches = 0usize;
    let mut diagnostics = candidate_universe.diagnostics.clone();
    let mut match_columns_buffer = Vec::new();
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

            for (line_idx, line) in content.lines().enumerate() {
                match_columns(line, &mut match_columns_buffer);
                if match_columns_buffer.is_empty() {
                    continue;
                }

                let line_number = line_idx + 1;
                let mut excerpt_for_line: Option<String> = None;

                for &column in &match_columns_buffer {
                    total_matches = total_matches.saturating_add(1);
                    if use_bounded_retention
                        && matches.len() == query.limit
                        && matches.last().is_some_and(|worst| {
                            !text_match_candidate_order(
                                &repository_id,
                                &rel_path,
                                line_number,
                                column,
                                line,
                                worst,
                            )
                            .is_lt()
                        })
                    {
                        continue;
                    }

                    let candidate = TextMatch {
                        repository_id: repository_id.clone(),
                        path: rel_path.clone(),
                        line: line_number,
                        column,
                        excerpt: excerpt_for_line
                            .get_or_insert_with(|| line.to_owned())
                            .clone(),
                    };

                    if use_bounded_retention {
                        retain_bounded_match(&mut matches, query.limit, candidate);
                    } else {
                        matches.push(candidate);
                    }
                }
            }
        }
    }

    sort_search_diagnostics_deterministically(&mut diagnostics.entries);

    if use_bounded_retention {
        return Ok(SearchExecutionOutput {
            total_matches,
            matches,
            diagnostics,
        });
    }

    sort_matches_deterministically(&mut matches);
    matches.truncate(query.limit);

    Ok(SearchExecutionOutput {
        total_matches,
        matches,
        diagnostics,
    })
}

fn trim_trailing_newline(line: &mut String) {
    if line.ends_with('\n') {
        line.pop();
        if line.ends_with('\r') {
            line.pop();
        }
    }
}
