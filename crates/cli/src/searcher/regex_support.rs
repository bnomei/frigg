use regex::{Regex, RegexBuilder};
use thiserror::Error;

use crate::domain::FriggError;

use super::{
    MAX_REGEX_ALTERNATIONS, MAX_REGEX_DFA_SIZE_LIMIT_BYTES, MAX_REGEX_GROUPS,
    MAX_REGEX_PATTERN_BYTES, MAX_REGEX_QUANTIFIERS, MAX_REGEX_SIZE_LIMIT_BYTES,
    REGEX_TRIGRAM_BITMAP_BITS, REGEX_TRIGRAM_BITMAP_WORDS, REGEX_TRIGRAM_HASH_MULTIPLIER,
};

#[derive(Debug, Clone)]
pub(super) struct RegexPrefilterPlan {
    checks: Vec<RegexPrefilterLiteralCheck>,
    needs_bitmap: bool,
}

#[derive(Debug, Clone)]
struct RegexPrefilterLiteralCheck {
    literal: String,
    trigram_hashes: Vec<usize>,
}

#[derive(Debug, Clone)]
struct TrigramBitmap {
    words: [u64; REGEX_TRIGRAM_BITMAP_WORDS],
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum ParsedRegexAtom {
    Literal(u8),
    NonLiteral,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct ParsedRegexQuantifier {
    min_repetitions: usize,
    exact_one: bool,
}

impl RegexPrefilterPlan {
    fn build(pattern: &str) -> Option<Self> {
        let literals = extract_required_regex_literals(pattern)?;
        let mut checks = Vec::with_capacity(literals.len());
        let mut needs_bitmap = false;

        for literal in literals {
            let trigram_hashes = literal_trigram_hashes(literal.as_bytes());
            if !trigram_hashes.is_empty() {
                needs_bitmap = true;
            }
            checks.push(RegexPrefilterLiteralCheck {
                literal,
                trigram_hashes,
            });
        }

        Some(Self {
            checks,
            needs_bitmap,
        })
    }

    pub(super) fn file_may_match(&self, content: &str) -> bool {
        let bitmap = self
            .needs_bitmap
            .then(|| TrigramBitmap::from_bytes(content.as_bytes()));

        for check in &self.checks {
            if !check.trigram_hashes.is_empty() {
                if let Some(bitmap) = bitmap.as_ref() {
                    if check
                        .trigram_hashes
                        .iter()
                        .any(|&hash| !bitmap.contains(hash))
                    {
                        return false;
                    }
                }
            } else if !content.contains(check.literal.as_str()) {
                return false;
            }
        }

        true
    }

    #[cfg(test)]
    pub(super) fn required_literals(&self) -> Vec<&str> {
        self.checks
            .iter()
            .map(|check| check.literal.as_str())
            .collect()
    }
}

impl TrigramBitmap {
    fn from_bytes(bytes: &[u8]) -> Self {
        let mut bitmap = Self {
            words: [0; REGEX_TRIGRAM_BITMAP_WORDS],
        };
        for window in bytes.windows(3) {
            bitmap.insert(trigram_hash(window[0], window[1], window[2]));
        }
        bitmap
    }

    fn insert(&mut self, hash: usize) {
        let word_index = hash / 64;
        let bit_index = hash % 64;
        self.words[word_index] |= 1_u64 << bit_index;
    }

    fn contains(&self, hash: usize) -> bool {
        let word_index = hash / 64;
        let bit_index = hash % 64;
        (self.words[word_index] & (1_u64 << bit_index)) != 0
    }
}

#[derive(Debug, Error)]
pub enum RegexSearchError {
    #[error("regex pattern must not be empty")]
    EmptyPattern,
    #[error("regex pattern length {actual} exceeds limit {max}")]
    PatternTooLong { actual: usize, max: usize },
    #[error("regex pattern alternation count {actual} exceeds limit {max}")]
    TooManyAlternations { actual: usize, max: usize },
    #[error("regex pattern group count {actual} exceeds limit {max}")]
    TooManyGroups { actual: usize, max: usize },
    #[error("regex pattern quantifier count {actual} exceeds limit {max}")]
    TooManyQuantifiers { actual: usize, max: usize },
    #[error("invalid regex: {0}")]
    InvalidRegex(#[from] regex::Error),
}

impl RegexSearchError {
    pub fn code(&self) -> &'static str {
        match self {
            Self::EmptyPattern => "regex_empty_pattern",
            Self::PatternTooLong { .. } => "regex_pattern_too_long",
            Self::TooManyAlternations { .. } => "regex_too_many_alternations",
            Self::TooManyGroups { .. } => "regex_too_many_groups",
            Self::TooManyQuantifiers { .. } => "regex_too_many_quantifiers",
            Self::InvalidRegex(_) => "regex_invalid_pattern",
        }
    }
}

pub fn compile_safe_regex(pattern: &str) -> Result<Regex, RegexSearchError> {
    validate_regex_budget(pattern)?;

    RegexBuilder::new(pattern)
        .size_limit(MAX_REGEX_SIZE_LIMIT_BYTES)
        .dfa_size_limit(MAX_REGEX_DFA_SIZE_LIMIT_BYTES)
        .build()
        .map_err(RegexSearchError::InvalidRegex)
}

pub(super) fn build_regex_prefilter_plan(pattern: &str) -> Option<RegexPrefilterPlan> {
    RegexPrefilterPlan::build(pattern)
}

pub(super) fn regex_error_to_frigg_error(err: RegexSearchError) -> FriggError {
    FriggError::InvalidInput(format!("regex search error [{}]: {err}", err.code()))
}

fn validate_regex_budget(pattern: &str) -> Result<(), RegexSearchError> {
    if pattern.is_empty() {
        return Err(RegexSearchError::EmptyPattern);
    }

    if pattern.len() > MAX_REGEX_PATTERN_BYTES {
        return Err(RegexSearchError::PatternTooLong {
            actual: pattern.len(),
            max: MAX_REGEX_PATTERN_BYTES,
        });
    }

    let alternations = count_unescaped_regex_chars(pattern, &['|']);
    if alternations > MAX_REGEX_ALTERNATIONS {
        return Err(RegexSearchError::TooManyAlternations {
            actual: alternations,
            max: MAX_REGEX_ALTERNATIONS,
        });
    }

    let groups = count_unescaped_regex_chars(pattern, &['(']);
    if groups > MAX_REGEX_GROUPS {
        return Err(RegexSearchError::TooManyGroups {
            actual: groups,
            max: MAX_REGEX_GROUPS,
        });
    }

    let quantifiers = count_unescaped_regex_chars(pattern, &['*', '+', '?', '{']);
    if quantifiers > MAX_REGEX_QUANTIFIERS {
        return Err(RegexSearchError::TooManyQuantifiers {
            actual: quantifiers,
            max: MAX_REGEX_QUANTIFIERS,
        });
    }

    Ok(())
}

fn count_unescaped_regex_chars(pattern: &str, targets: &[char]) -> usize {
    let mut count = 0usize;
    let mut escaped = false;
    let mut in_class = false;

    for ch in pattern.chars() {
        if escaped {
            escaped = false;
            continue;
        }

        if ch == '\\' {
            escaped = true;
            continue;
        }

        if ch == '[' {
            in_class = true;
            continue;
        }

        if ch == ']' && in_class {
            in_class = false;
            continue;
        }

        if in_class {
            continue;
        }

        if targets.contains(&ch) {
            count += 1;
        }
    }

    count
}

fn extract_required_regex_literals(pattern: &str) -> Option<Vec<String>> {
    if pattern.is_empty() || !pattern.is_ascii() {
        return None;
    }

    let bytes = pattern.as_bytes();
    let mut index = 0usize;
    let mut literals = Vec::new();
    let mut current_literal = Vec::new();

    while index < bytes.len() {
        let atom = parse_regex_atom(bytes, &mut index)?;
        let quantifier = parse_regex_quantifier(bytes, &mut index)?;

        match atom {
            ParsedRegexAtom::Literal(byte) => {
                if quantifier.min_repetitions == 0 {
                    flush_required_literal(&mut literals, &mut current_literal);
                } else if quantifier.exact_one {
                    current_literal.push(byte);
                } else {
                    flush_required_literal(&mut literals, &mut current_literal);
                    literals.push(char::from(byte).to_string());
                }
            }
            ParsedRegexAtom::NonLiteral => {
                flush_required_literal(&mut literals, &mut current_literal);
            }
        }
    }

    flush_required_literal(&mut literals, &mut current_literal);

    if literals.is_empty() {
        return None;
    }

    let mut deduped = Vec::with_capacity(literals.len());
    for literal in literals {
        if !deduped.iter().any(|existing| existing == &literal) {
            deduped.push(literal);
        }
    }

    if deduped.is_empty() {
        None
    } else {
        Some(deduped)
    }
}

fn parse_regex_atom(bytes: &[u8], index: &mut usize) -> Option<ParsedRegexAtom> {
    let byte = *bytes.get(*index)?;
    *index += 1;

    match byte {
        b'|' | b'(' | b')' => None,
        b'[' => {
            parse_char_class(bytes, index)?;
            Some(ParsedRegexAtom::NonLiteral)
        }
        b'\\' => parse_escape_atom(bytes, index),
        b'.' | b'^' | b'$' => Some(ParsedRegexAtom::NonLiteral),
        b'*' | b'+' | b'?' | b'{' | b'}' => None,
        _ => Some(ParsedRegexAtom::Literal(byte)),
    }
}

fn parse_char_class(bytes: &[u8], index: &mut usize) -> Option<()> {
    let mut escaped = false;
    while let Some(&byte) = bytes.get(*index) {
        *index += 1;
        if escaped {
            escaped = false;
            continue;
        }
        if byte == b'\\' {
            escaped = true;
            continue;
        }
        if byte == b']' {
            return Some(());
        }
    }
    None
}

fn parse_escape_atom(bytes: &[u8], index: &mut usize) -> Option<ParsedRegexAtom> {
    let escaped = *bytes.get(*index)?;
    *index += 1;

    if is_regex_literal_escape(escaped) {
        return Some(ParsedRegexAtom::Literal(escaped));
    }

    if is_supported_non_literal_escape(escaped) {
        return Some(ParsedRegexAtom::NonLiteral);
    }

    None
}

fn is_regex_literal_escape(escaped: u8) -> bool {
    matches!(
        escaped,
        b'\\'
            | b'.'
            | b'+'
            | b'*'
            | b'?'
            | b'|'
            | b'('
            | b')'
            | b'['
            | b']'
            | b'{'
            | b'}'
            | b'^'
            | b'$'
            | b'-'
    )
}

fn is_supported_non_literal_escape(escaped: u8) -> bool {
    matches!(
        escaped,
        b'd' | b'D'
            | b's'
            | b'S'
            | b'w'
            | b'W'
            | b'b'
            | b'B'
            | b'A'
            | b'z'
            | b'n'
            | b'r'
            | b't'
            | b'f'
            | b'v'
    )
}

fn parse_regex_quantifier(bytes: &[u8], index: &mut usize) -> Option<ParsedRegexQuantifier> {
    let mut quantifier = ParsedRegexQuantifier {
        min_repetitions: 1,
        exact_one: true,
    };
    let Some(&byte) = bytes.get(*index) else {
        return Some(quantifier);
    };

    match byte {
        b'?' => {
            *index += 1;
            quantifier.min_repetitions = 0;
            quantifier.exact_one = false;
            consume_lazy_quantifier_suffix(bytes, index);
        }
        b'*' => {
            *index += 1;
            quantifier.min_repetitions = 0;
            quantifier.exact_one = false;
            consume_lazy_quantifier_suffix(bytes, index);
        }
        b'+' => {
            *index += 1;
            quantifier.min_repetitions = 1;
            quantifier.exact_one = false;
            consume_lazy_quantifier_suffix(bytes, index);
        }
        b'{' => {
            *index += 1;
            let (min, max) = parse_braced_quantifier(bytes, index)?;
            quantifier.min_repetitions = min;
            quantifier.exact_one = min == 1 && max == Some(1);
            consume_lazy_quantifier_suffix(bytes, index);
        }
        _ => {}
    }

    Some(quantifier)
}

fn parse_braced_quantifier(bytes: &[u8], index: &mut usize) -> Option<(usize, Option<usize>)> {
    let min = parse_quantifier_number(bytes, index)?;
    let mut max = Some(min);

    match bytes.get(*index).copied() {
        Some(b'}') => {
            *index += 1;
        }
        Some(b',') => {
            *index += 1;
            match bytes.get(*index).copied() {
                Some(b'}') => {
                    *index += 1;
                    max = None;
                }
                Some(_) => {
                    let upper = parse_quantifier_number(bytes, index)?;
                    if upper < min {
                        return None;
                    }
                    max = Some(upper);
                    if bytes.get(*index).copied() != Some(b'}') {
                        return None;
                    }
                    *index += 1;
                }
                None => return None,
            }
        }
        _ => return None,
    }

    Some((min, max))
}

fn parse_quantifier_number(bytes: &[u8], index: &mut usize) -> Option<usize> {
    let start = *index;
    while let Some(&byte) = bytes.get(*index) {
        if !byte.is_ascii_digit() {
            break;
        }
        *index += 1;
    }
    if *index == start {
        return None;
    }

    std::str::from_utf8(&bytes[start..*index])
        .ok()?
        .parse()
        .ok()
}

fn consume_lazy_quantifier_suffix(bytes: &[u8], index: &mut usize) {
    if bytes.get(*index).copied() == Some(b'?') {
        *index += 1;
    }
}

fn flush_required_literal(literals: &mut Vec<String>, current_literal: &mut Vec<u8>) {
    if current_literal.is_empty() {
        return;
    }
    literals.push(String::from_utf8_lossy(current_literal).to_string());
    current_literal.clear();
}

fn literal_trigram_hashes(bytes: &[u8]) -> Vec<usize> {
    let mut hashes = bytes
        .windows(3)
        .map(|window| trigram_hash(window[0], window[1], window[2]))
        .collect::<Vec<_>>();
    hashes.sort_unstable();
    hashes.dedup();
    hashes
}

fn trigram_hash(left: u8, middle: u8, right: u8) -> usize {
    let packed = (u32::from(left) << 16) | (u32::from(middle) << 8) | u32::from(right);
    (packed.wrapping_mul(REGEX_TRIGRAM_HASH_MULTIPLIER) as usize) & (REGEX_TRIGRAM_BITMAP_BITS - 1)
}
