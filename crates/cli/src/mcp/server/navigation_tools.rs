//! Navigation MCP tools: references, definitions, declarations, implementations, and call hierarchy.

use super::*;
use crate::domain::WorkloadFallbackReason;
use crate::domain::model::ReferenceMatchKind;
use crate::mcp::types::NavigationMode;

mod call_hierarchy;
mod location;
mod references;
