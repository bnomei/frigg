//! Routing scorecard for the production frigg-first skill.
//!
//! Reads `skills/frigg-first-code-search/SKILL.md` (workspace-relative from
//! `CARGO_MANIFEST_DIR/../..`) and asserts the compact intent map / scenario
//! picker still routes to the expected first tools. Fails if the skill regresses.

#![allow(clippy::panic)]

use std::path::PathBuf;

fn production_skill_markdown() -> String {
    let path = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("../..")
        .join("skills/frigg-first-code-search/SKILL.md");
    assert!(
        path.is_file(),
        "production skill must exist at {} (routing scorecard)",
        path.display()
    );
    std::fs::read_to_string(&path)
        .unwrap_or_else(|err| panic!("failed to read production skill {}: {err}", path.display()))
}

/// Extract the compact intent-map fenced block that follows the scenario picker.
fn compact_intent_map(skill: &str) -> &str {
    const MARKER: &str = "Compact intent map (same routing, shorter):";
    let after_marker = skill
        .split_once(MARKER)
        .unwrap_or_else(|| panic!("skill must include compact intent map marker: {MARKER:?}"))
        .1;
    let after_fence = after_marker
        .split_once("```text")
        .unwrap_or_else(|| panic!("compact intent map must open a ```text fence"))
        .1;
    after_fence
        .split_once("```")
        .unwrap_or_else(|| panic!("compact intent map fence must close"))
        .0
}

fn assert_intent_line_routes(map: &str, intent_needle: &str, expected_tool: &str) {
    let line = map
        .lines()
        .map(str::trim)
        .find(|line| line.contains(intent_needle))
        .unwrap_or_else(|| {
            panic!("compact intent map missing line containing {intent_needle:?}:\n{map}")
        });
    assert!(
        line.contains(expected_tool),
        "intent {intent_needle:?} must first-route to {expected_tool:?}, got line: {line}"
    );
}

#[test]
fn futura_routing_scorecard_skill_intent_map_and_shell_card() {
    let skill = production_skill_markdown();

    assert!(
        skill.contains("Pick your scenario"),
        "skill must keep scenario-first picker heading (Pick your scenario)"
    );
    assert!(
        skill.contains("## Full shell → Frigg card")
            || skill.contains("| Shell habit | Frigg call |"),
        "skill must keep the shell → Frigg translation card"
    );

    let map = compact_intent_map(&skill);

    // Known string/regex → search_text
    assert_intent_line_routes(map, "Known string or regex", "search_text");
    // Known function/type → search_symbol
    assert_intent_line_routes(map, "Known function/type", "search_symbol");
    // Vague where is X → search_hybrid
    assert_intent_line_routes(map, "where is X", "search_hybrid");
    // Several guesses → search_batch
    assert_intent_line_routes(map, "Several guesses", "search_batch");
    // Need proof → read_match
    assert_intent_line_routes(map, "Need proof", "read_match");
    // Need impact → find_references OR impact_bundle
    {
        let line = map
            .lines()
            .map(str::trim)
            .find(|line| line.contains("Need impact"))
            .expect("compact intent map must include Need impact");
        assert!(
            line.contains("impact_bundle") || line.contains("find_references"),
            "Need impact must route to impact_bundle or find_references, got: {line}"
        );
    }
    // Wrong repo/zero → workspace
    assert_intent_line_routes(map, "Wrong repo", "workspace");

    // BAD list contains hybrid -> grep anti-pattern
    assert!(
        skill.contains("BAD: hybrid -> grep") || skill.contains("hybrid -> grep"),
        "skill BAD list must contain hybrid -> grep anti-pattern"
    );
}
