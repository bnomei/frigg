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
