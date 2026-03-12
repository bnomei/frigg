use crate::searcher::surfaces::HybridSourceClass;

pub(crate) fn selection_class_novelty_bonus(class: HybridSourceClass) -> f32 {
    match class {
        HybridSourceClass::ErrorContracts
        | HybridSourceClass::ToolContracts
        | HybridSourceClass::BenchmarkDocs => 0.08,
        HybridSourceClass::Documentation
        | HybridSourceClass::Runtime
        | HybridSourceClass::Project
        | HybridSourceClass::Tests => 0.04,
        HybridSourceClass::Support => 0.02,
        HybridSourceClass::Fixtures => 0.035,
        HybridSourceClass::Readme => 0.02,
        HybridSourceClass::Specs | HybridSourceClass::Other => 0.0,
        _ => 0.04,
    }
}

pub(crate) fn selection_class_repeat_penalty(class: HybridSourceClass) -> f32 {
    match class {
        HybridSourceClass::ToolContracts => 0.09,
        HybridSourceClass::BenchmarkDocs => 0.07,
        HybridSourceClass::ErrorContracts | HybridSourceClass::Documentation => 0.05,
        HybridSourceClass::Readme => 0.03,
        HybridSourceClass::Runtime
        | HybridSourceClass::Project
        | HybridSourceClass::Tests
        | HybridSourceClass::Fixtures => 0.015,
        HybridSourceClass::Support => 0.02,
        HybridSourceClass::Specs | HybridSourceClass::Other => 0.01,
        _ => 0.015,
    }
}
