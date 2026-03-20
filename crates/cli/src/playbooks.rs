//! Playbook parsing and regression tracing for repeatable search evaluation. This layer turns
//! retrieval expectations into executable probes so ranking changes can be measured without
//! coupling that logic to the MCP runtime.

mod loader;
mod model;
mod parser;
mod runner;
mod trace;
mod witness;

pub use loader::{load_hybrid_playbook_regressions, load_playbook_document};
pub use model::{
    HybridPlaybookCandidateTraceSnapshot, HybridPlaybookChannelHitSnapshot,
    HybridPlaybookChannelTrace, HybridPlaybookProbeOutcome, HybridPlaybookRankedHitSnapshot,
    HybridPlaybookRegression, HybridPlaybookRunSummary, HybridPlaybookTracePacket,
    HybridPlaybookWitnessOutcome, HybridWitnessGroup, HybridWitnessMatchBy, HybridWitnessMatchMode,
    HybridWitnessRequirement, LoadedHybridPlaybookRegression, PlaybookDocument, PlaybookMetadata,
};
pub use parser::{parse_playbook_document, scrub_playbook_metadata_header};
pub use runner::{run_hybrid_playbook_regression, run_hybrid_playbook_regressions};

#[cfg(test)]
use witness::{semantic_status_allowed, witness_outcomes};

#[cfg(test)]
mod tests {
    use super::{
        HybridPlaybookProbeOutcome, HybridPlaybookWitnessOutcome, HybridWitnessGroup,
        HybridWitnessMatchBy, HybridWitnessMatchMode, HybridWitnessRequirement, PlaybookDocument,
        load_hybrid_playbook_regressions, parse_playbook_document, scrub_playbook_metadata_header,
        semantic_status_allowed, witness_outcomes,
    };
    use crate::domain::FriggResult;
    use std::env;
    use std::fs;
    use std::path::Path;
    use std::time::{SystemTime, UNIX_EPOCH};

    fn mk_group(
        group_id: &str,
        match_any: Vec<&str>,
        required_when: HybridWitnessRequirement,
    ) -> HybridWitnessGroup {
        HybridWitnessGroup {
            group_id: group_id.to_owned(),
            match_any: match_any.into_iter().map(str::to_owned).collect(),
            match_mode: HybridWitnessMatchMode::ExactAny,
            accepted_prefixes: Vec::new(),
            required_when,
        }
    }

    fn mk_outcome(
        group_id: &str,
        match_any: Vec<&str>,
        required_when: HybridWitnessRequirement,
        passed: bool,
    ) -> HybridPlaybookWitnessOutcome {
        HybridPlaybookWitnessOutcome {
            group_id: group_id.to_owned(),
            match_any: match_any.into_iter().map(str::to_owned).collect(),
            match_mode: HybridWitnessMatchMode::ExactAny,
            accepted_prefixes: Vec::new(),
            required_when,
            matched_by: HybridWitnessMatchBy::None,
            matched_path: None,
            passed,
        }
    }

    #[test]
    fn parse_playbook_document_extracts_metadata_and_body() -> FriggResult<()> {
        let raw = r#"# Example

<!-- frigg-playbook
{
  "schema": "frigg.playbook.hybrid.v1",
  "playbook_id": "hybrid-search-context-retrieval",
  "query": "semantic runtime strict failure note metadata",
  "top_k": 8,
  "allowed_semantic_statuses": ["ok", "degraded", "disabled"],
  "required_witness_groups": [
    {
      "name": "docs",
      "paths": ["contracts/errors.md"],
      "required_when": "semantic_ok"
    }
  ],
  "target_witness_groups": [
    {
      "name": "docs",
      "paths": ["contracts/errors.md"]
    }
  ]
}
-->
Body text.
"#;

        let parsed = parse_playbook_document(raw)?;
        assert_eq!(
            parsed.metadata.playbook_schema,
            "frigg.playbook.hybrid.v1".to_owned()
        );
        assert_eq!(
            parsed.metadata.playbook_id,
            "hybrid-search-context-retrieval".to_owned()
        );
        let spec = parsed
            .metadata
            .hybrid_regression
            .clone()
            .expect("hybrid regression metadata must be present");
        assert_eq!(spec.query, "semantic runtime strict failure note metadata");
        assert_eq!(spec.top_k, 8);
        assert_eq!(
            spec.allowed_semantic_statuses,
            vec!["ok", "degraded", "disabled"]
        );
        assert_eq!(spec.witness_groups.len(), 1);
        assert_eq!(
            spec.witness_groups[0].required_when,
            HybridWitnessRequirement::SemanticOk
        );
        assert_eq!(spec.target_witness_groups.len(), 1);
        assert_eq!(
            spec.target_witness_groups[0].match_any,
            vec!["contracts/errors.md"]
        );
        assert_eq!(
            parsed,
            PlaybookDocument {
                metadata: parsed.metadata.clone(),
                body: "# Example\n\n\nBody text.\n".to_owned(),
            }
        );
        Ok(())
    }

    #[test]
    fn parse_playbook_document_normalizes_nested_hybrid_defaults_and_witness_groups()
    -> FriggResult<()> {
        let raw = r#"<!-- frigg-playbook
{
  "playbook_schema": "frigg.playbook.hybrid.v1",
  "playbook_id": "nested-hybrid-defaults",
  "hybrid_regression": {
    "query": "trace hybrid witness defaults",
    "allowed_semantic_statuses": ["ok"],
    "witness_groups": [
      {
        "group_id": "runtime",
        "match_any": ["src/runtime.rs"]
      }
    ],
    "target_witness_groups": [
      {
        "name": "docs",
        "paths": ["docs/runtime.md"]
      }
    ],
    "target_paths": ["contracts/runtime.md"]
  }
}
-->
"#;

        let parsed = parse_playbook_document(raw)?;
        let spec = parsed
            .metadata
            .hybrid_regression
            .expect("hybrid regression metadata must be present");
        assert_eq!(spec.query, "trace hybrid witness defaults");
        assert_eq!(spec.top_k, 8);
        assert_eq!(spec.allowed_semantic_statuses, vec!["ok"]);
        assert_eq!(spec.witness_groups.len(), 1);
        assert_eq!(spec.witness_groups[0].group_id, "runtime");
        assert_eq!(spec.witness_groups[0].match_any, vec!["src/runtime.rs"]);
        assert_eq!(
            spec.witness_groups[0].required_when,
            HybridWitnessRequirement::Always
        );
        assert_eq!(spec.target_witness_groups.len(), 2);
        assert_eq!(spec.target_witness_groups[0].group_id, "docs");
        assert_eq!(
            spec.target_witness_groups[0].match_any,
            vec!["docs/runtime.md"]
        );
        assert_eq!(
            spec.target_witness_groups[0].required_when,
            HybridWitnessRequirement::Always
        );
        assert_eq!(
            spec.target_witness_groups[1].group_id,
            "contracts/runtime.md"
        );
        assert_eq!(
            spec.target_witness_groups[1].match_any,
            vec!["contracts/runtime.md"]
        );
        assert_eq!(
            spec.target_witness_groups[1].required_when,
            HybridWitnessRequirement::SemanticOk
        );
        Ok(())
    }

    #[test]
    fn witness_outcomes_evaluates_required_and_optional_groups() {
        let groups = vec![
            mk_group(
                "always",
                vec!["src/lib.rs"],
                HybridWitnessRequirement::Always,
            ),
            mk_group(
                "ok-only",
                vec!["docs/ok.md"],
                HybridWitnessRequirement::SemanticOk,
            ),
            mk_group(
                "empty",
                vec!["missing"],
                HybridWitnessRequirement::SemanticOk,
            ),
        ];

        let all_required = witness_outcomes(&groups, &["src/lib.rs".to_owned()], true, false);
        assert_eq!(all_required.len(), 3);
        assert_eq!(all_required[0].passed, true);
        assert_eq!(all_required[0].matched_by, HybridWitnessMatchBy::Exact);
        assert_eq!(all_required[1].passed, false);
        assert_eq!(all_required[1].matched_by, HybridWitnessMatchBy::None);
        assert_eq!(all_required[2].passed, false);
        assert_eq!(all_required[2].matched_by, HybridWitnessMatchBy::None);

        let all_required = witness_outcomes(&groups, &["src/ignored".to_owned()], false, true);
        assert_eq!(all_required.len(), 1);
        assert_eq!(all_required[0].passed, false);
        assert_eq!(all_required[0].matched_by, HybridWitnessMatchBy::None);
        let all_required = witness_outcomes(&groups, &["docs/ok.md".to_owned()], true, false);
        assert_eq!(all_required.len(), 3);
        assert_eq!(all_required[0].passed, false);
        assert_eq!(all_required[1].passed, true);
        assert_eq!(all_required[1].matched_by, HybridWitnessMatchBy::Exact);
        assert_eq!(all_required[2].passed, false);
    }

    #[test]
    fn witness_outcomes_records_prefix_hits_without_flipping_exact_gate() {
        let groups = vec![HybridWitnessGroup {
            group_id: "tests".to_owned(),
            match_any: vec!["apps/server/tests/unit/foo_test.py".to_owned()],
            match_mode: HybridWitnessMatchMode::ExactOrPrefix,
            accepted_prefixes: vec!["apps/server/tests".to_owned()],
            required_when: HybridWitnessRequirement::Always,
        }];

        let outcomes = witness_outcomes(
            &groups,
            &["apps/server/tests/integration/bar_test.py".to_owned()],
            true,
            false,
        );
        assert_eq!(outcomes.len(), 1);
        assert_eq!(outcomes[0].matched_by, HybridWitnessMatchBy::Prefix);
        assert_eq!(
            outcomes[0].matched_path,
            Some("apps/server/tests/integration/bar_test.py".to_owned())
        );
        assert!(!outcomes[0].passed);
    }

    #[test]
    fn semantic_status_allowed_respects_empty_allowlist_as_open() {
        assert!(semantic_status_allowed(&[], "OK"));
        assert!(semantic_status_allowed(&[], "random"));
    }

    #[test]
    fn hybrid_probe_outcome_helpers_cover_required_and_target_modes() {
        let outcome = HybridPlaybookProbeOutcome {
            file_name: "pb.md".to_owned(),
            playbook_id: "pb".to_owned(),
            semantic_status: "ok".to_owned(),
            semantic_reason: None,
            status_allowed: true,
            duration_ms: 1,
            execution_error: None,
            matched_paths: vec!["src/lib.rs".to_owned()],
            trace_path: None,
            required_witness_groups: vec![mk_outcome(
                "runtime",
                vec!["src/lib.rs"],
                HybridWitnessRequirement::Always,
                true,
            )],
            target_witness_groups: vec![mk_outcome(
                "docs",
                vec!["README.md"],
                HybridWitnessRequirement::SemanticOk,
                false,
            )],
        };

        assert!(outcome.passed_required());
        assert!(!outcome.passed_targets());
        assert!(!outcome.passed_all(true));
        assert!(outcome.passed_all(false));
        assert_eq!(outcome.required_missing(), Vec::<String>::new());
        assert_eq!(
            outcome.target_missing(),
            vec!["docs -> [\"README.md\"]".to_owned()]
        );

        let errored = HybridPlaybookProbeOutcome {
            execution_error: Some("boom".to_owned()),
            status_allowed: false,
            ..outcome
        };
        assert!(!errored.passed_required());
    }

    #[test]
    fn hybrid_probe_outcome_target_only_blocks_disabled_semantic() {
        let groups = vec![mk_group(
            "docs",
            vec!["docs/readme.md"],
            HybridWitnessRequirement::Always,
        )];
        let targets = witness_outcomes(&groups, &["docs/readme.md".to_owned()], false, true);
        assert_eq!(targets.len(), 1);
        assert!(targets[0].passed);
    }

    #[test]
    fn parse_playbook_document_requires_query_for_legacy_hybrid_metadata() {
        let raw = r#"<!-- frigg-playbook
{
  "schema": "frigg.playbook.hybrid.v1",
  "playbook_id": "missing-query"
}
-->
"#;

        let error =
            parse_playbook_document(raw).expect_err("hybrid playbooks without a query should fail");
        assert!(
            error
                .to_string()
                .contains("hybrid playbook metadata must include a query"),
            "unexpected missing query error: {error}"
        );
    }

    #[test]
    fn parse_playbook_document_rejects_witness_groups_without_identity() {
        let raw = r#"<!-- frigg-playbook
{
  "playbook_schema": "frigg.playbook.hybrid.v1",
  "playbook_id": "missing-group-id",
  "hybrid_regression": {
    "query": "trace witness identity validation",
    "witness_groups": [
      {
        "paths": ["src/lib.rs"]
      }
    ]
  }
}
-->
"#;

        let error = parse_playbook_document(raw)
            .expect_err("witness groups without group_id or name should fail");
        assert!(
            error
                .to_string()
                .contains("hybrid witness group must include group_id or name"),
            "unexpected missing witness group identity error: {error}"
        );
    }

    #[test]
    fn parse_playbook_document_rejects_witness_groups_without_paths() {
        let raw = r#"<!-- frigg-playbook
{
  "playbook_schema": "frigg.playbook.hybrid.v1",
  "playbook_id": "missing-group-paths",
  "hybrid_regression": {
    "query": "trace witness path validation",
    "target_witness_groups": [
      {
        "name": "docs"
      }
    ]
  }
}
-->
"#;

        let error = parse_playbook_document(raw)
            .expect_err("witness groups without match_any or paths should fail");
        assert!(
            error
                .to_string()
                .contains("hybrid witness group 'docs' must include at least one path"),
            "unexpected missing witness group paths error: {error}"
        );
    }

    #[test]
    fn semantic_status_allowed_treats_unavailable_like_disabled_fallback() {
        let allowed = vec![
            "ok".to_owned(),
            "degraded".to_owned(),
            "disabled".to_owned(),
        ];

        assert!(super::semantic_status_allowed(&allowed, "disabled"));
        assert!(super::semantic_status_allowed(&allowed, "unavailable"));
        assert!(!super::semantic_status_allowed(
            &["ok".to_owned()],
            "unavailable"
        ));
    }

    #[test]
    fn load_hybrid_playbook_regressions_requires_metadata_for_markdown_playbooks() -> FriggResult<()>
    {
        let root = temp_playbook_root("missing-metadata");
        fs::create_dir_all(&root).map_err(crate::domain::FriggError::Io)?;
        fs::write(root.join("README.md"), "# Playbooks\n")
            .map_err(crate::domain::FriggError::Io)?;
        fs::write(root.join("alpha.md"), "# Alpha\n").map_err(crate::domain::FriggError::Io)?;

        let error = load_hybrid_playbook_regressions(&root)
            .expect_err("markdown playbooks without metadata should fail");
        assert!(
            error
                .to_string()
                .contains("failed to load playbook metadata"),
            "unexpected playbook metadata error: {error}"
        );

        cleanup_root(&root);
        Ok(())
    }

    #[test]
    fn load_hybrid_playbook_regressions_requires_hybrid_regression_metadata() -> FriggResult<()> {
        let root = temp_playbook_root("missing-hybrid-regression");
        fs::create_dir_all(&root).map_err(crate::domain::FriggError::Io)?;
        fs::write(
            root.join("alpha.md"),
            r#"<!-- frigg-playbook
{
  "playbook_schema": "frigg.playbook.v1",
  "playbook_id": "docs-only"
}
-->
# Alpha
"#,
        )
        .map_err(crate::domain::FriggError::Io)?;

        let error = load_hybrid_playbook_regressions(&root)
            .expect_err("non-hybrid playbooks should fail executable regression loading");
        assert!(
            error
                .to_string()
                .contains("missing hybrid_regression metadata"),
            "unexpected missing hybrid regression error: {error}"
        );

        cleanup_root(&root);
        Ok(())
    }

    #[test]
    fn load_hybrid_playbook_regressions_rejects_empty_playbook_roots() -> FriggResult<()> {
        let root = temp_playbook_root("empty-root");
        fs::create_dir_all(&root).map_err(crate::domain::FriggError::Io)?;

        let error =
            load_hybrid_playbook_regressions(&root).expect_err("empty playbook roots should fail");
        assert!(
            error
                .to_string()
                .contains("no executable hybrid playbooks found under"),
            "unexpected empty playbook root error: {error}"
        );

        cleanup_root(&root);
        Ok(())
    }

    #[test]
    fn scrub_playbook_metadata_header_preserves_line_numbers_but_hides_query_text() {
        let raw = r#"<!-- frigg-playbook
{
  "playbook_schema": "frigg.playbook.v1",
  "playbook_id": "http-auth-entrypoint-trace",
  "hybrid_regression": {
    "query": "where is the optional HTTP MCP auth token declared enforced and documented"
  }
}
-->
# HTTP Auth
"#;

        let scrubbed = scrub_playbook_metadata_header(raw);
        assert_eq!(raw.lines().count(), scrubbed.lines().count());
        assert!(
            !scrubbed.contains("where is the optional HTTP MCP auth token"),
            "scrubbed playbook text should not expose executable query strings"
        );
        assert!(scrubbed.contains("# HTTP Auth"));
    }

    fn temp_playbook_root(test_name: &str) -> std::path::PathBuf {
        let nonce = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap_or_default()
            .as_nanos();
        env::temp_dir().join(format!(
            "frigg-playbooks-{test_name}-{nonce}-{}",
            std::process::id()
        ))
    }

    fn cleanup_root(root: &Path) {
        let _ = fs::remove_dir_all(root);
    }
}
