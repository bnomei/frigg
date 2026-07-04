//! Search helper caches that are not query-answer caches.
//!
//! Caches expensive search helper state such as corpora slices; distinct from MCP query-answer
//! response caches.

use super::*;

impl FriggMcpServer {
    pub(crate) fn compile_cached_safe_regex(
        &self,
        raw: &str,
    ) -> Result<regex::Regex, crate::searcher::RegexSearchError> {
        if let Some(cached) = self
            .cache_state
            .compiled_safe_regex_cache
            .read()
            .unwrap_or_else(|poisoned| poisoned.into_inner())
            .get(raw)
            .cloned()
        {
            self.record_runtime_cache_event(
                RuntimeCacheFamily::CompiledSafeRegex,
                RuntimeCacheEvent::Hit,
                1,
            );
            return Ok(cached);
        }
        self.record_runtime_cache_event(
            RuntimeCacheFamily::CompiledSafeRegex,
            RuntimeCacheEvent::Miss,
            1,
        );

        let compiled = compile_safe_regex(raw)?;
        let mut cache = self
            .cache_state
            .compiled_safe_regex_cache
            .write()
            .unwrap_or_else(|poisoned| poisoned.into_inner());
        if let Some(cached) = cache.get(raw).cloned() {
            self.record_runtime_cache_event(
                RuntimeCacheFamily::CompiledSafeRegex,
                RuntimeCacheEvent::Hit,
                1,
            );
            return Ok(cached);
        }
        let inserted = cache.insert(raw.to_owned(), compiled.clone()).is_none();
        if inserted {
            self.record_runtime_cache_event(
                RuntimeCacheFamily::CompiledSafeRegex,
                RuntimeCacheEvent::Insert,
                1,
            );
        }
        self.trim_runtime_cache_to_budget(
            RuntimeCacheFamily::CompiledSafeRegex,
            &mut cache,
            |pattern, _| pattern.len().saturating_add(256),
        );
        Ok(compiled)
    }
}
