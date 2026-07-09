//! Synthetic seed for forced search edges (not product source).
//!
//! Sentinel phrases are unique so dogfood/lang boards never collide.

pub fn synth_greeting() -> &'static str {
    "hello from futura synth fixture ALPHA_UNIQUE_SENTINEL"
}

pub fn synth_count_target() -> &'static str {
    // Repeated so count_only can report total_matches > 0 without ambiguity.
    "FUTURA_SYNTH_COUNT_TOKEN FUTURA_SYNTH_COUNT_TOKEN"
}

/// Pipe-free body: a literal query with `|` must miss so regex-trap recovery fires.
pub fn synth_regex_trap_body() -> &'static str {
    "plain text without regex metacharacters for trap recovery"
}
