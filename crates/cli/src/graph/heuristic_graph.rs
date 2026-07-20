//! Heuristic symbol registration and relation traversal on the in-memory graph.
//!
//! Registers tree-sitter symbol nodes and walks heuristic relation edges so graph-channel search
//! and navigation can expand from lexical seeds without waiting for SCIP ingest.

use petgraph::Direction;
use petgraph::visit::EdgeRef;

use super::*;

impl SymbolGraph {
    /// Inserts or updates a heuristic symbol node by `symbol_id`; returns `true` when newly created.
    pub fn register_symbol(&mut self, symbol: SymbolNode) -> bool {
        if let Some(index) = self.node_by_symbol.get(&symbol.symbol_id).copied() {
            if let Some(existing) = self.graph.node_weight_mut(index) {
                *existing = symbol;
            }
            return false;
        }

        let symbol_id = symbol.symbol_id.clone();
        let index = self.graph.add_node(symbol);
        self.node_by_symbol.insert(symbol_id, index);
        true
    }

    /// Registers many heuristic symbols, overwriting metadata for existing ids.
    pub fn register_symbols<I>(&mut self, symbols: I)
    where
        I: IntoIterator<Item = SymbolNode>,
    {
        for symbol in symbols {
            let _ = self.register_symbol(symbol);
        }
    }

    /// Looks up a registered heuristic symbol by stable id.
    pub fn symbol(&self, symbol_id: &str) -> Option<&SymbolNode> {
        let index = self.node_by_symbol.get(symbol_id)?;
        self.graph.node_weight(*index)
    }

    /// Number of heuristic symbol nodes currently registered.
    pub fn symbol_count(&self) -> usize {
        self.node_by_symbol.len()
    }

    /// Number of directed heuristic relation edges currently stored.
    pub fn relation_count(&self) -> usize {
        self.graph.edge_count()
    }

    /// Adds a directed heuristic edge if both endpoints exist and the edge is not already present.
    ///
    /// Returns `Ok(true)` when inserted; `Ok(false)` on duplicate; errors if either symbol is unknown.
    pub fn add_relation(
        &mut self,
        from_symbol: &str,
        to_symbol: &str,
        relation: RelationKind,
    ) -> SymbolGraphResult<bool> {
        let from_index = self
            .node_by_symbol
            .get(from_symbol)
            .copied()
            .ok_or_else(|| SymbolGraphError::UnknownFromSymbol(from_symbol.to_owned()))?;
        let to_index = self
            .node_by_symbol
            .get(to_symbol)
            .copied()
            .ok_or_else(|| SymbolGraphError::UnknownToSymbol(to_symbol.to_owned()))?;

        if self
            .graph
            .edges_connecting(from_index, to_index)
            .any(|edge| edge.weight() == &relation)
        {
            return Ok(false);
        }

        self.graph.add_edge(from_index, to_index, relation);
        Ok(true)
    }

    /// Outgoing heuristic edges from `symbol_id`, sorted for stable graph-channel expansion.
    ///
    /// Unknown symbols yield an empty list rather than an error.
    pub fn outgoing_relations(&self, symbol_id: &str) -> Vec<SymbolRelation> {
        let Some(index) = self.node_by_symbol.get(symbol_id).copied() else {
            return Vec::new();
        };

        let mut relations = self
            .graph
            .edges_directed(index, Direction::Outgoing)
            .filter_map(|edge| {
                let from_symbol = self.graph.node_weight(edge.source())?;
                let to_symbol = self.graph.node_weight(edge.target())?;
                Some(SymbolRelation {
                    from_symbol: from_symbol.symbol_id.clone(),
                    to_symbol: to_symbol.symbol_id.clone(),
                    relation: *edge.weight(),
                })
            })
            .collect::<Vec<_>>();

        relations.sort_by(symbol_relation_order);
        relations
    }

    /// Incoming heuristic edges to `symbol_id`, sorted for stable graph-channel expansion.
    pub fn incoming_relations(&self, symbol_id: &str) -> Vec<SymbolRelation> {
        let Some(index) = self.node_by_symbol.get(symbol_id).copied() else {
            return Vec::new();
        };

        let mut relations = self
            .graph
            .edges_directed(index, Direction::Incoming)
            .filter_map(|edge| {
                let from_symbol = self.graph.node_weight(edge.source())?;
                let to_symbol = self.graph.node_weight(edge.target())?;
                Some(SymbolRelation {
                    from_symbol: from_symbol.symbol_id.clone(),
                    to_symbol: to_symbol.symbol_id.clone(),
                    relation: *edge.weight(),
                })
            })
            .collect::<Vec<_>>();

        relations.sort_by(symbol_relation_order);
        relations
    }

    /// Neighbor nodes reachable via outgoing heuristic edges (relation + full node payload).
    pub fn outgoing_adjacency(&self, symbol_id: &str) -> Vec<AdjacentSymbol> {
        let Some(index) = self.node_by_symbol.get(symbol_id).copied() else {
            return Vec::new();
        };

        let mut adjacency = self
            .graph
            .edges_directed(index, Direction::Outgoing)
            .filter_map(|edge| {
                let target = self.graph.node_weight(edge.target())?;
                Some(AdjacentSymbol {
                    relation: *edge.weight(),
                    symbol: target.clone(),
                })
            })
            .collect::<Vec<_>>();

        adjacency.sort_by(adjacent_symbol_order);
        adjacency
    }

    /// Neighbor nodes that point at `symbol_id` via heuristic edges.
    pub fn incoming_adjacency(&self, symbol_id: &str) -> Vec<AdjacentSymbol> {
        let Some(index) = self.node_by_symbol.get(symbol_id).copied() else {
            return Vec::new();
        };

        let mut adjacency = self
            .graph
            .edges_directed(index, Direction::Incoming)
            .filter_map(|edge| {
                let source = self.graph.node_weight(edge.source())?;
                Some(AdjacentSymbol {
                    relation: *edge.weight(),
                    symbol: source.clone(),
                })
            })
            .collect::<Vec<_>>();

        adjacency.sort_by(adjacent_symbol_order);
        adjacency
    }

    /// Incoming heuristic relations as ranked hints (confidence + path/line) for navigation UX.
    pub fn heuristic_relation_hints_for_target(
        &self,
        target_symbol_id: &str,
    ) -> Vec<HeuristicRelationHint> {
        let Some(target_index) = self.node_by_symbol.get(target_symbol_id).copied() else {
            return Vec::new();
        };

        let mut hints = self
            .graph
            .edges_directed(target_index, Direction::Incoming)
            .filter_map(|edge| {
                let source_symbol = self.graph.node_weight(edge.source())?;
                let target_symbol = self.graph.node_weight(edge.target())?;
                Some(HeuristicRelationHint {
                    source_symbol: source_symbol.clone(),
                    target_symbol: target_symbol.clone(),
                    relation: *edge.weight(),
                    confidence: HeuristicConfidence::from_relation(*edge.weight()),
                })
            })
            .collect::<Vec<_>>();

        hints.sort_by(heuristic_relation_hint_order);
        hints
    }
}
