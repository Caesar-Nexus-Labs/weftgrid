//! Binary split-tree layout (P2 Keystone 4, invariant #2).
//!
//! A workspace's layout is a recursive binary tree. Leaves are panes that stack
//! one or more surfaces (`panel_ids` + `selected_panel_id`); branches split two
//! children at a divider position. The tree stores only ids — content lives in
//! the flat `Workspace.panels` registry (invariant #1).

use serde::{Deserialize, Serialize};

use super::PanelId;

/// Orientation of a split branch.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum SplitOrientation {
    /// Children placed left/right.
    Horizontal,
    /// Children placed top/bottom.
    Vertical,
}

/// A node in the binary split-tree.
///
/// Serialized as an internally-tagged enum (`{"type":"pane",...}` /
/// `{"type":"split",...}`) so TS discriminated unions and serde agree on the wire.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "lowercase")]
pub enum LayoutNode {
    /// Leaf: a pane stacking one or more surfaces (in-pane tab bar).
    Pane {
        /// Surfaces stacked in this pane, in tab order. Always >= 1.
        panel_ids: Vec<PanelId>,
        /// Currently-selected surface; `None` falls back to the first.
        #[serde(skip_serializing_if = "Option::is_none", default)]
        selected_panel_id: Option<PanelId>,
    },
    /// Branch: exactly two children split at `divider_position`.
    Split {
        orientation: SplitOrientation,
        /// Fraction of space given to `first`, clamped to 0.1..=0.9.
        divider_position: f64,
        first: Box<LayoutNode>,
        second: Box<LayoutNode>,
    },
}

/// Lower clamp for a split divider position (invariant #2).
pub const DIVIDER_MIN: f64 = 0.1;
/// Upper clamp for a split divider position (invariant #2).
pub const DIVIDER_MAX: f64 = 0.9;

impl LayoutNode {
    /// A single-pane leaf holding one surface.
    pub fn leaf(panel_id: PanelId) -> Self {
        LayoutNode::Pane {
            panel_ids: vec![panel_id],
            selected_panel_id: Some(panel_id),
        }
    }

    /// Clamp a raw divider fraction into the legal 0.1..=0.9 range (invariant #2).
    pub fn clamp_divider(position: f64) -> f64 {
        position.clamp(DIVIDER_MIN, DIVIDER_MAX)
    }

    /// Collect every PanelId referenced anywhere in the tree (depth-first).
    pub fn panel_ids(&self) -> Vec<PanelId> {
        let mut out = Vec::new();
        self.collect_panel_ids(&mut out);
        out
    }

    fn collect_panel_ids(&self, out: &mut Vec<PanelId>) {
        match self {
            LayoutNode::Pane { panel_ids, .. } => out.extend_from_slice(panel_ids),
            LayoutNode::Split { first, second, .. } => {
                first.collect_panel_ids(out);
                second.collect_panel_ids(out);
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use uuid::Uuid;

    #[test]
    fn divider_clamps_to_legal_range() {
        assert_eq!(LayoutNode::clamp_divider(0.0), DIVIDER_MIN);
        assert_eq!(LayoutNode::clamp_divider(1.0), DIVIDER_MAX);
        assert_eq!(LayoutNode::clamp_divider(0.5), 0.5);
    }

    #[test]
    fn pane_leaf_serde_round_trip() {
        let id = Uuid::new_v4();
        let node = LayoutNode::leaf(id);
        let json = serde_json::to_string(&node).unwrap();
        assert!(json.contains("\"type\":\"pane\""));
        let back: LayoutNode = serde_json::from_str(&json).unwrap();
        assert_eq!(node, back);
    }

    #[test]
    fn split_tree_serde_round_trip_and_panel_collection() {
        let a = Uuid::new_v4();
        let b = Uuid::new_v4();
        let node = LayoutNode::Split {
            orientation: SplitOrientation::Horizontal,
            divider_position: 0.5,
            first: Box::new(LayoutNode::leaf(a)),
            second: Box::new(LayoutNode::leaf(b)),
        };
        let json = serde_json::to_string(&node).unwrap();
        assert!(json.contains("\"type\":\"split\""));
        let back: LayoutNode = serde_json::from_str(&json).unwrap();
        assert_eq!(node, back);
        assert_eq!(back.panel_ids(), vec![a, b]);
    }
}
