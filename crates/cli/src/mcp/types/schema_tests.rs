#![allow(clippy::panic)]

use std::collections::BTreeSet;
use std::ffi::OsStr;
use std::fs;
use std::path::PathBuf;

use schemars::{JsonSchema, schema_for};
use serde::Deserialize;

use super::*;

#[derive(Debug, Deserialize)]
struct ToolSchemaDoc {
    schema_id: String,
    tool_name: String,
    input_wrapper: String,
    output_wrapper: String,
    input_fields: Vec<String>,
    input_required: Vec<String>,
    output_fields: Vec<String>,
    output_required: Vec<String>,
    #[serde(default)]
    contract_notes: Vec<String>,
    #[serde(default)]
    nested_contracts: Option<Value>,
    #[serde(default)]
    step_tool_schema_refs: Vec<StepToolSchemaRefDoc>,
    #[serde(default)]
    input_example: Option<Value>,
    #[serde(default)]
    output_example: Option<Value>,
}

#[derive(Debug, Deserialize)]
struct StepToolSchemaRefDoc {
    tool_name: String,
    schema_file: String,
    params_wrapper: String,
    response_wrapper: String,
}

fn docs_dir() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../contracts/tools/v1")
}

fn read_doc(file_name: &str) -> ToolSchemaDoc {
    let path = docs_dir().join(file_name);
    let raw = fs::read_to_string(&path)
        .unwrap_or_else(|err| panic!("failed to read schema doc {}: {err}", path.display()));
    serde_json::from_str::<ToolSchemaDoc>(&raw)
        .unwrap_or_else(|err| panic!("failed to parse schema doc {}: {err}", path.display()))
}

fn field_set<T: JsonSchema>() -> BTreeSet<String> {
    let schema_json =
        serde_json::to_value(schema_for!(T)).expect("failed to serialize generated schema");
    schema_json
        .get("properties")
        .and_then(|value| value.as_object())
        .map(|props| props.keys().cloned().collect())
        .unwrap_or_default()
}

fn required_set<T: JsonSchema>() -> BTreeSet<String> {
    let schema_json =
        serde_json::to_value(schema_for!(T)).expect("failed to serialize generated schema");
    schema_json
        .get("required")
        .and_then(|value| value.as_array())
        .map(|items| {
            items
                .iter()
                .filter_map(|item| item.as_str().map(ToOwned::to_owned))
                .collect()
        })
        .unwrap_or_default()
}

fn to_set(values: &[String]) -> BTreeSet<String> {
    values.iter().cloned().collect()
}

fn property_description<T: JsonSchema>(field: &str) -> Option<String> {
    let schema_json =
        serde_json::to_value(schema_for!(T)).expect("failed to serialize generated schema");
    schema_json
        .get("properties")
        .and_then(|value| value.get(field))
        .and_then(|value| value.get("description"))
        .and_then(|value| value.as_str())
        .map(ToOwned::to_owned)
}

fn property_schema<T: JsonSchema>(field: &str) -> Value {
    let schema_json =
        serde_json::to_value(schema_for!(T)).expect("failed to serialize generated schema");
    schema_json
        .get("properties")
        .and_then(Value::as_object)
        .and_then(|props| props.get(field))
        .cloned()
        .unwrap_or_else(|| panic!("missing schema property `{field}`"))
}

fn schema_allows_type(schema: &Value, expected: &str) -> bool {
    schema.get("type").and_then(Value::as_str) == Some(expected)
        || schema
            .get("type")
            .and_then(Value::as_array)
            .is_some_and(|items| items.iter().any(|item| item.as_str() == Some(expected)))
        || schema
            .get("anyOf")
            .and_then(Value::as_array)
            .is_some_and(|variants| {
                variants
                    .iter()
                    .any(|variant| schema_allows_type(variant, expected))
            })
        || schema
            .get("oneOf")
            .and_then(Value::as_array)
            .is_some_and(|variants| {
                variants
                    .iter()
                    .any(|variant| schema_allows_type(variant, expected))
            })
}

fn assert_optional_string_property<T: JsonSchema>(field: &str) {
    let property = property_schema::<T>(field);
    assert!(
        schema_allows_type(&property, "string"),
        "expected `{field}` to allow string transport, got schema: {property}"
    );
    assert!(
        !schema_allows_type(&property, "object"),
        "expected `{field}` to avoid object transport, got schema: {property}"
    );
}

fn assert_contract<TInput: JsonSchema, TOutput: JsonSchema>(
    file_name: &str,
    tool_name: &str,
    input_wrapper: &str,
    output_wrapper: &str,
) {
    let doc = read_doc(file_name);
    assert_eq!(doc.schema_id, format!("frigg.tools.{tool_name}.v1"));
    assert_eq!(doc.tool_name, tool_name);
    assert_eq!(doc.input_wrapper, input_wrapper);
    assert_eq!(doc.output_wrapper, output_wrapper);
    assert_eq!(to_set(&doc.input_fields), field_set::<TInput>());
    assert_eq!(to_set(&doc.input_required), required_set::<TInput>());
    assert_eq!(to_set(&doc.output_fields), field_set::<TOutput>());
    assert_eq!(to_set(&doc.output_required), required_set::<TOutput>());
}

fn assert_examples_parse<TInput, TOutput>(file_name: &str)
where
    TInput: for<'de> Deserialize<'de>,
    TOutput: for<'de> Deserialize<'de>,
{
    let doc = read_doc(file_name);
    assert!(
        !doc.contract_notes.is_empty(),
        "{file_name} should publish contract_notes for nested deep-search payload guidance"
    );
    assert!(
        doc.nested_contracts.is_some(),
        "{file_name} should publish nested_contracts guidance"
    );

    let input_example = doc
        .input_example
        .unwrap_or_else(|| panic!("{file_name} should publish an input_example"));
    serde_json::from_value::<TInput>(input_example)
        .unwrap_or_else(|err| panic!("failed to parse input_example in {file_name}: {err}"));

    let output_example = doc
        .output_example
        .unwrap_or_else(|| panic!("{file_name} should publish an output_example"));
    serde_json::from_value::<TOutput>(output_example)
        .unwrap_or_else(|err| panic!("failed to parse output_example in {file_name}: {err}"));
}

fn nested_strings(doc: &ToolSchemaDoc, pointer: &str) -> BTreeSet<String> {
    doc.nested_contracts
        .as_ref()
        .unwrap_or_else(|| panic!("missing nested_contracts for {pointer}"))
        .pointer(pointer)
        .and_then(Value::as_array)
        .unwrap_or_else(|| panic!("missing string array at nested_contracts{pointer}"))
        .iter()
        .map(|value| {
            value
                .as_str()
                .unwrap_or_else(|| panic!("expected string at nested_contracts{pointer}"))
                .to_owned()
        })
        .collect()
}

fn assert_step_tool_schema_refs(file_name: &str) {
    let doc = read_doc(file_name);
    let actual = doc
        .step_tool_schema_refs
        .iter()
        .map(|entry| {
            (
                entry.tool_name.as_str(),
                entry.schema_file.as_str(),
                entry.params_wrapper.as_str(),
                entry.response_wrapper.as_str(),
            )
        })
        .collect::<Vec<_>>();
    let expected = vec![
        (
            "list_repositories",
            "list_repositories.v1.schema.json",
            "ListRepositoriesParams",
            "ListRepositoriesResponse",
        ),
        (
            "read_file",
            "read_file.v1.schema.json",
            "ReadFileParams",
            "ReadFileResponse",
        ),
        (
            "search_text",
            "search_text.v1.schema.json",
            "SearchTextParams",
            "SearchTextResponse",
        ),
        (
            "search_symbol",
            "search_symbol.v1.schema.json",
            "SearchSymbolParams",
            "SearchSymbolResponse",
        ),
        (
            "find_references",
            "find_references.v1.schema.json",
            "FindReferencesParams",
            "FindReferencesResponse",
        ),
    ];
    assert_eq!(
        actual, expected,
        "{file_name} step_tool_schema_refs drifted from the allowed deep-search step surface"
    );

    for entry in &doc.step_tool_schema_refs {
        let schema_path = docs_dir().join(&entry.schema_file);
        assert!(
            schema_path.exists(),
            "referenced schema file {} does not exist",
            schema_path.display()
        );
    }
}

fn assert_deep_search_stdio_setup_notes(file_name: &str) {
    let doc = read_doc(file_name);
    let notes = doc.contract_notes.join(" ");
    for required in [
        "FRIGG_MCP_TOOL_SURFACE_PROFILE=extended",
        "RUST_LOG=error",
        "--watch-mode off",
        "list_repositories",
    ] {
        assert!(
            notes.contains(required),
            "{file_name} contract_notes should mention `{required}`: {notes}"
        );
    }
}

fn assert_run_nested_contracts(file_name: &str) {
    let doc = read_doc(file_name);
    assert_eq!(
        nested_strings(&doc, "/playbook/required"),
        required_set::<DeepSearchPlaybookContract>()
    );
    assert_eq!(
        nested_strings(&doc, "/playbook/step_required"),
        required_set::<DeepSearchPlaybookStepContract>()
    );
    assert_eq!(
        nested_strings(&doc, "/playbook/allowed_step_tools"),
        [
            "find_references",
            "list_repositories",
            "read_file",
            "search_symbol",
            "search_text",
        ]
        .into_iter()
        .map(ToOwned::to_owned)
        .collect()
    );
    assert_eq!(
        nested_strings(&doc, "/trace_artifact/required"),
        required_set::<DeepSearchTraceArtifactContract>()
    );
    assert_eq!(
        nested_strings(&doc, "/trace_artifact/step_required"),
        required_set::<DeepSearchTraceStepContract>()
    );
}

fn assert_replay_nested_contracts(file_name: &str) {
    let doc = read_doc(file_name);
    assert_eq!(
        nested_strings(&doc, "/playbook/required"),
        required_set::<DeepSearchPlaybookContract>()
    );
    assert_eq!(
        nested_strings(&doc, "/playbook/step_required"),
        required_set::<DeepSearchPlaybookStepContract>()
    );
    assert_eq!(
        nested_strings(&doc, "/expected_trace_artifact/required"),
        required_set::<DeepSearchTraceArtifactContract>()
    );
    assert_eq!(
        nested_strings(&doc, "/replay_response/required"),
        required_set::<DeepSearchReplayResponse>()
    );
}

fn assert_citation_nested_contracts(file_name: &str) {
    let doc = read_doc(file_name);
    assert_eq!(
        nested_strings(&doc, "/trace_artifact/required"),
        required_set::<DeepSearchTraceArtifactContract>()
    );
    assert_eq!(
        nested_strings(&doc, "/citation_payload/required"),
        required_set::<DeepSearchCitationPayloadContract>()
    );
    assert_eq!(
        nested_strings(&doc, "/citation_payload/claim_required"),
        required_set::<DeepSearchClaimContract>()
    );
    assert_eq!(
        nested_strings(&doc, "/citation_payload/citation_required"),
        required_set::<DeepSearchCitationContract>()
    );
    assert_eq!(
        nested_strings(&doc, "/citation_payload/span_required"),
        required_set::<DeepSearchFileSpanContract>()
    );
}

#[test]
fn schema_list_repositories_contract_matches_wrappers() {
    assert_contract::<ListRepositoriesParams, ListRepositoriesResponse>(
        "list_repositories.v1.schema.json",
        "list_repositories",
        "ListRepositoriesParams",
        "ListRepositoriesResponse",
    );
}

#[test]
fn schema_workspace_attach_contract_matches_wrappers() {
    assert_contract::<WorkspaceAttachParams, WorkspaceAttachResponse>(
        "workspace_attach.v1.schema.json",
        "workspace_attach",
        "WorkspaceAttachParams",
        "WorkspaceAttachResponse",
    );
}

#[test]
fn schema_workspace_current_contract_matches_wrappers() {
    assert_contract::<WorkspaceCurrentParams, WorkspaceCurrentResponse>(
        "workspace_current.v1.schema.json",
        "workspace_current",
        "WorkspaceCurrentParams",
        "WorkspaceCurrentResponse",
    );
}

#[test]
fn schema_read_file_contract_matches_wrappers() {
    assert_contract::<ReadFileParams, ReadFileResponse>(
        "read_file.v1.schema.json",
        "read_file",
        "ReadFileParams",
        "ReadFileResponse",
    );
}

#[test]
fn schema_explore_contract_matches_wrappers() {
    assert_contract::<ExploreParams, ExploreResponse>(
        "explore.v1.schema.json",
        "explore",
        "ExploreParams",
        "ExploreResponse",
    );
}

#[test]
fn schema_explore_examples_parse_against_wrappers() {
    assert_examples_parse::<ExploreParams, ExploreResponse>("explore.v1.schema.json");
}

#[test]
fn schema_search_text_contract_matches_wrappers() {
    assert_contract::<SearchTextParams, SearchTextResponse>(
        "search_text.v1.schema.json",
        "search_text",
        "SearchTextParams",
        "SearchTextResponse",
    );
}

#[test]
fn schema_search_text_includes_scoping_guidance() {
    let repository_id = property_description::<SearchTextParams>("repository_id")
        .expect("repository_id should expose a schema description");
    assert!(
        repository_id.contains("list_repositories"),
        "repository_id description should mention list_repositories guidance: {repository_id}"
    );

    let path_regex = property_description::<SearchTextParams>("path_regex")
        .expect("path_regex should expose a schema description");
    assert!(
        path_regex.contains("canonical repository-relative paths"),
        "path_regex description should mention canonical repository-relative paths: {path_regex}"
    );
    assert!(
        path_regex.contains("code, docs, or runtime slices"),
        "path_regex description should explain scoping guidance: {path_regex}"
    );
}

#[test]
fn schema_search_hybrid_contract_matches_wrappers() {
    assert_contract::<SearchHybridParams, SearchHybridResponse>(
        "search_hybrid.v1.schema.json",
        "search_hybrid",
        "SearchHybridParams",
        "SearchHybridResponse",
    );
}

#[test]
fn schema_search_hybrid_includes_follow_up_guidance() {
    let query = property_description::<SearchHybridParams>("query")
        .expect("query should expose a schema description");
    assert!(
        query.contains("search_symbol"),
        "search_hybrid.query description should mention search_symbol follow-up guidance: {query}"
    );
    assert!(
        query.contains("path_regex"),
        "search_hybrid.query description should mention scoped search_text path_regex guidance: {query}"
    );

    let metadata = property_description::<SearchHybridResponse>("metadata")
        .expect("metadata should expose a schema description");
    assert!(
        metadata.contains("Canonical structured"),
        "search_hybrid.metadata description should mention canonical structured diagnostics: {metadata}"
    );
    let note = property_description::<SearchHybridResponse>("note")
        .expect("note should expose a schema description");
    assert!(
        note.contains("Legacy"),
        "search_hybrid.note description should mention legacy compatibility transport: {note}"
    );
    assert!(
        note.contains("JSON-encoded"),
        "search_hybrid.note description should mention JSON-encoded compatibility metadata: {note}"
    );
}

#[test]
fn schema_search_hybrid_note_remains_string_encoded() {
    assert_optional_string_property::<SearchHybridResponse>("note");
}

#[test]
fn schema_search_symbol_contract_matches_wrappers() {
    assert_contract::<SearchSymbolParams, SearchSymbolResponse>(
        "search_symbol.v1.schema.json",
        "search_symbol",
        "SearchSymbolParams",
        "SearchSymbolResponse",
    );
}

#[test]
fn schema_search_symbol_includes_runtime_pivot_guidance() {
    let query = property_description::<SearchSymbolParams>("query")
        .expect("query should expose a schema description");
    assert!(
        query.contains("search_hybrid"),
        "search_symbol.query description should mention search_hybrid follow-up guidance: {query}"
    );
    assert!(
        query.contains("runtime anchor"),
        "search_symbol.query description should explain runtime-anchor usage: {query}"
    );

    let path_class = property_description::<SearchSymbolParams>("path_class")
        .expect("path_class should expose a schema description");
    assert!(
        path_class.contains("runtime") && path_class.contains("support"),
        "search_symbol.path_class description should explain path classes: {path_class}"
    );

    let path_regex = property_description::<SearchSymbolParams>("path_regex")
        .expect("path_regex should expose a schema description");
    assert!(
        path_regex.contains("canonical repository-relative symbol paths"),
        "search_symbol.path_regex description should mention canonical repository-relative symbol paths: {path_regex}"
    );
    assert!(
        path_regex.contains("constrain overloaded names"),
        "search_symbol.path_regex description should explain overloaded-name scoping: {path_regex}"
    );
}

#[test]
fn schema_find_references_contract_matches_wrappers() {
    assert_contract::<FindReferencesParams, FindReferencesResponse>(
        "find_references.v1.schema.json",
        "find_references",
        "FindReferencesParams",
        "FindReferencesResponse",
    );
}

#[test]
fn schema_go_to_definition_contract_matches_wrappers() {
    assert_contract::<GoToDefinitionParams, GoToDefinitionResponse>(
        "go_to_definition.v1.schema.json",
        "go_to_definition",
        "GoToDefinitionParams",
        "GoToDefinitionResponse",
    );
}

#[test]
fn schema_find_declarations_contract_matches_wrappers() {
    assert_contract::<FindDeclarationsParams, FindDeclarationsResponse>(
        "find_declarations.v1.schema.json",
        "find_declarations",
        "FindDeclarationsParams",
        "FindDeclarationsResponse",
    );
}

#[test]
fn schema_find_implementations_contract_matches_wrappers() {
    assert_contract::<FindImplementationsParams, FindImplementationsResponse>(
        "find_implementations.v1.schema.json",
        "find_implementations",
        "FindImplementationsParams",
        "FindImplementationsResponse",
    );
}

#[test]
fn schema_incoming_calls_contract_matches_wrappers() {
    assert_contract::<IncomingCallsParams, IncomingCallsResponse>(
        "incoming_calls.v1.schema.json",
        "incoming_calls",
        "IncomingCallsParams",
        "IncomingCallsResponse",
    );
}

#[test]
fn schema_outgoing_calls_contract_matches_wrappers() {
    assert_contract::<OutgoingCallsParams, OutgoingCallsResponse>(
        "outgoing_calls.v1.schema.json",
        "outgoing_calls",
        "OutgoingCallsParams",
        "OutgoingCallsResponse",
    );
}

#[test]
fn schema_document_symbols_contract_matches_wrappers() {
    assert_contract::<DocumentSymbolsParams, DocumentSymbolsResponse>(
        "document_symbols.v1.schema.json",
        "document_symbols",
        "DocumentSymbolsParams",
        "DocumentSymbolsResponse",
    );
}

#[test]
fn schema_search_structural_contract_matches_wrappers() {
    assert_contract::<SearchStructuralParams, SearchStructuralResponse>(
        "search_structural.v1.schema.json",
        "search_structural",
        "SearchStructuralParams",
        "SearchStructuralResponse",
    );
}

#[test]
fn schema_deep_search_run_contract_matches_wrappers() {
    assert_contract::<DeepSearchRunParams, DeepSearchRunResponse>(
        "deep_search_run.v1.schema.json",
        "deep_search_run",
        "DeepSearchRunParams",
        "DeepSearchRunResponse",
    );
}

#[test]
fn schema_deep_search_replay_contract_matches_wrappers() {
    assert_contract::<DeepSearchReplayParams, DeepSearchReplayResponse>(
        "deep_search_replay.v1.schema.json",
        "deep_search_replay",
        "DeepSearchReplayParams",
        "DeepSearchReplayResponse",
    );
}

#[test]
fn schema_deep_search_compose_citations_contract_matches_wrappers() {
    assert_contract::<DeepSearchComposeCitationsParams, DeepSearchComposeCitationsResponse>(
        "deep_search_compose_citations.v1.schema.json",
        "deep_search_compose_citations",
        "DeepSearchComposeCitationsParams",
        "DeepSearchComposeCitationsResponse",
    );
}

#[test]
fn schema_deep_search_run_examples_parse_against_wrappers() {
    assert_examples_parse::<DeepSearchRunParams, DeepSearchRunResponse>(
        "deep_search_run.v1.schema.json",
    );
}

#[test]
fn schema_deep_search_run_contract_notes_and_step_refs_stay_in_sync() {
    assert_deep_search_stdio_setup_notes("deep_search_run.v1.schema.json");
    assert_step_tool_schema_refs("deep_search_run.v1.schema.json");
    assert_run_nested_contracts("deep_search_run.v1.schema.json");
}

#[test]
fn schema_deep_search_replay_examples_parse_against_wrappers() {
    assert_examples_parse::<DeepSearchReplayParams, DeepSearchReplayResponse>(
        "deep_search_replay.v1.schema.json",
    );
}

#[test]
fn schema_deep_search_replay_contract_notes_and_step_refs_stay_in_sync() {
    assert_deep_search_stdio_setup_notes("deep_search_replay.v1.schema.json");
    assert_step_tool_schema_refs("deep_search_replay.v1.schema.json");
    assert_replay_nested_contracts("deep_search_replay.v1.schema.json");
}

#[test]
fn schema_deep_search_compose_citations_examples_parse_against_wrappers() {
    assert_examples_parse::<DeepSearchComposeCitationsParams, DeepSearchComposeCitationsResponse>(
        "deep_search_compose_citations.v1.schema.json",
    );
}

#[test]
fn schema_deep_search_compose_citations_contract_notes_and_step_refs_stay_in_sync() {
    assert_deep_search_stdio_setup_notes("deep_search_compose_citations.v1.schema.json");
    assert_step_tool_schema_refs("deep_search_compose_citations.v1.schema.json");
    assert_citation_nested_contracts("deep_search_compose_citations.v1.schema.json");
}

#[test]
fn schema_docs_presence_for_read_only_tools() {
    let base = docs_dir();
    let expected = PUBLIC_READ_ONLY_TOOL_NAMES
        .iter()
        .map(|name| format!("{name}.v1.schema.json"))
        .collect::<BTreeSet<_>>();
    let actual = fs::read_dir(&base)
        .unwrap_or_else(|err| panic!("failed to read schema docs dir {}: {err}", base.display()))
        .map(|entry| {
            entry
                .unwrap_or_else(|err| panic!("failed to read schema docs dir entry: {err}"))
                .path()
        })
        .filter(|path| path.extension() == Some(OsStr::new("json")))
        .filter_map(|path| {
            path.file_name()
                .and_then(|name| name.to_str().map(ToOwned::to_owned))
        })
        .collect::<BTreeSet<_>>();

    assert_eq!(
        actual, expected,
        "public tool schema file set drifted; update tests/contracts intentionally before adding tools"
    );
}

#[test]
fn schema_core_read_only_input_fields_exclude_confirm_param() {
    let confirm = WRITE_CONFIRM_PARAM.to_owned();
    let input_field_sets = [
        field_set::<ListRepositoriesParams>(),
        field_set::<WorkspaceAttachParams>(),
        field_set::<WorkspaceCurrentParams>(),
        field_set::<ReadFileParams>(),
        field_set::<ExploreParams>(),
        field_set::<SearchTextParams>(),
        field_set::<SearchHybridParams>(),
        field_set::<SearchSymbolParams>(),
        field_set::<FindReferencesParams>(),
        field_set::<GoToDefinitionParams>(),
        field_set::<FindDeclarationsParams>(),
        field_set::<FindImplementationsParams>(),
        field_set::<IncomingCallsParams>(),
        field_set::<OutgoingCallsParams>(),
        field_set::<DocumentSymbolsParams>(),
        field_set::<SearchStructuralParams>(),
        field_set::<DeepSearchRunParams>(),
        field_set::<DeepSearchReplayParams>(),
        field_set::<DeepSearchComposeCitationsParams>(),
    ];

    for fields in input_field_sets {
        assert!(
            !fields.contains(&confirm),
            "read-only tool params must not expose `{}` before a write-surface contract upgrade",
            WRITE_CONFIRM_PARAM
        );
    }
}

#[test]
fn schema_write_surface_policy_markers_are_present_in_contract_docs() {
    let tools_readme_path = docs_dir().join("README.md");
    let tools_readme = fs::read_to_string(&tools_readme_path).unwrap_or_else(|err| {
        panic!(
            "failed to read tools contract README {}: {err}",
            tools_readme_path.display()
        )
    });
    for marker in [
        "write_surface_policy: v1",
        "current_public_tool_surface: read_only",
        "write_confirm_required: true",
        "write_confirm_semantics: reject_missing_or_false_confirm_before_side_effects",
        "write_safety_invariant_workspace_boundary: required",
        "write_safety_invariant_path_traversal_defense: required",
        "write_safety_invariant_regex_budget_limits: required",
        "write_safety_invariant_typed_deterministic_errors: required",
    ] {
        assert!(
            tools_readme.contains(marker),
            "tools contract README is missing policy marker `{marker}`"
        );
    }

    let confirm_param_marker = format!("write_confirm_param: {WRITE_CONFIRM_PARAM}");
    assert!(
        tools_readme.contains(&confirm_param_marker),
        "tools contract README is missing policy marker `{confirm_param_marker}`"
    );
    let confirm_error_marker =
        format!("write_confirm_failure_error_code: {WRITE_CONFIRMATION_REQUIRED_ERROR_CODE}");
    assert!(
        tools_readme.contains(&confirm_error_marker),
        "tools contract README is missing policy marker `{confirm_error_marker}`"
    );

    let errors_path = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../contracts/errors.md");
    let errors_doc = fs::read_to_string(&errors_path).unwrap_or_else(|err| {
        panic!(
            "failed to read errors contract {}: {err}",
            errors_path.display()
        )
    });
    for marker in [
        "write_surface_policy: v1",
        "write_confirm_required: true",
        "write_no_side_effect_without_confirm: true",
    ] {
        assert!(
            errors_doc.contains(marker),
            "errors contract is missing policy marker `{marker}`"
        );
    }
    assert!(
        errors_doc.contains(&confirm_param_marker),
        "errors contract is missing policy marker `{confirm_param_marker}`"
    );
    assert!(
        errors_doc.contains(&confirm_error_marker),
        "errors contract is missing policy marker `{confirm_error_marker}`"
    );
}
