use std::time::Instant;

use serde::Serialize;

#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize)]
pub struct SearchStageSample {
    pub elapsed_us: u64,
    pub input_count: usize,
    pub output_count: usize,
}

impl SearchStageSample {
    pub const fn new(elapsed_us: u64, input_count: usize, output_count: usize) -> Self {
        Self {
            elapsed_us,
            input_count,
            output_count,
        }
    }
}

#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize)]
pub struct SearchStageAttribution {
    pub candidate_intake: SearchStageSample,
    pub freshness_validation: SearchStageSample,
    pub scan: SearchStageSample,
    pub witness_scoring: SearchStageSample,
    pub graph_expansion: SearchStageSample,
    pub semantic_retrieval: SearchStageSample,
    pub anchor_blending: SearchStageSample,
    pub document_aggregation: SearchStageSample,
    pub final_diversification: SearchStageSample,
}

pub(super) fn elapsed_us(started_at: Instant) -> u64 {
    u64::try_from(started_at.elapsed().as_micros()).unwrap_or(u64::MAX)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn search_stage_sample_new_initializes_fields() {
        let sample = SearchStageSample::new(11, 22, 33);

        assert_eq!(sample.elapsed_us, 11);
        assert_eq!(sample.input_count, 22);
        assert_eq!(sample.output_count, 33);
    }

    #[test]
    fn search_stage_attribution_can_clone_and_preserve_defaults() {
        let attribution = SearchStageAttribution {
            candidate_intake: SearchStageSample::new(1, 2, 3),
            freshness_validation: SearchStageSample::new(4, 5, 6),
            scan: SearchStageSample::new(7, 8, 9),
            witness_scoring: SearchStageSample::new(10, 11, 12),
            graph_expansion: SearchStageSample::new(13, 14, 15),
            semantic_retrieval: SearchStageSample::new(16, 17, 18),
            anchor_blending: SearchStageSample::new(19, 20, 21),
            document_aggregation: SearchStageSample::new(22, 23, 24),
            final_diversification: SearchStageSample::new(25, 26, 27),
        };

        let cloned = attribution.clone();

        assert_eq!(attribution, cloned);
        assert_eq!(attribution.freshness_validation.output_count, cloned.freshness_validation.output_count);
        assert_eq!(attribution.graph_expansion.input_count, cloned.graph_expansion.input_count);
    }
}
