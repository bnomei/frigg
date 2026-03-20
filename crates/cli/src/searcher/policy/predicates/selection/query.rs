use super::{PredicateLeaf, SelectionFacts};

fn query_mentions_cli(ctx: &SelectionFacts) -> bool {
    ctx.query_mentions_cli
}

fn query_has_exact_terms(ctx: &SelectionFacts) -> bool {
    ctx.query_has_exact_terms
}

fn query_has_identifier_anchor(ctx: &SelectionFacts) -> bool {
    ctx.query_has_identifier_anchor
}

fn query_has_specific_blade_anchors(ctx: &SelectionFacts) -> bool {
    ctx.query_has_specific_blade_anchors
}

macro_rules! leaf {
    ($name:ident, $id:literal, $pred:ident) => {
        pub(crate) const fn $name() -> PredicateLeaf<SelectionFacts> {
            PredicateLeaf::new($id, $pred)
        }
    };
}

leaf!(
    query_mentions_cli_leaf,
    "query.mentions_cli",
    query_mentions_cli
);
leaf!(
    query_has_exact_terms_leaf,
    "query.has_exact_terms",
    query_has_exact_terms
);
leaf!(
    query_has_identifier_anchor_leaf,
    "query.has_identifier_anchor",
    query_has_identifier_anchor
);
leaf!(
    query_has_specific_blade_anchors_leaf,
    "query.has_specific_blade_anchors",
    query_has_specific_blade_anchors
);
