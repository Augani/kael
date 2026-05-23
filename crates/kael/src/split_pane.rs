use anyhow::{anyhow, Result};
use serde::{Deserialize, Serialize};

/// Direction of a split within the pane tree.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum SplitDirection {
    /// Split panes side by side (left/right).
    Horizontal,
    /// Split panes stacked (top/bottom).
    Vertical,
}

/// Unique identifier for a pane.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct PaneId(pub u64);

/// Unique identifier for a tab within a pane.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct TabId(pub u64);

/// A single tab inside a pane.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Tab {
    /// The unique identifier for this tab.
    pub id: TabId,
    /// Display label shown in the tab bar.
    pub label: String,
    /// Whether the user can close this tab.
    pub closable: bool,
}

/// A pane containing zero or more tabs, with at most one active.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Pane {
    /// The unique identifier for this pane.
    pub id: PaneId,
    tabs: Vec<Tab>,
    active_tab: Option<TabId>,
}

impl Pane {
    /// Create a new empty pane with the given identifier.
    pub fn new(id: PaneId) -> Self {
        Self {
            id,
            tabs: Vec::new(),
            active_tab: None,
        }
    }

    /// Add a tab to this pane and make it the active tab.
    pub fn add_tab(&mut self, tab: Tab) {
        let tab_id = tab.id;
        self.tabs.push(tab);
        self.active_tab = Some(tab_id);
    }

    /// Close and remove a tab by its identifier, returning it if found.
    ///
    /// If the closed tab was active, activates the previous tab (or the next
    /// one if it was the first tab).
    pub fn close_tab(&mut self, tab_id: TabId) -> Option<Tab> {
        let position = self.tabs.iter().position(|t| t.id == tab_id)?;
        let removed = self.tabs.remove(position);

        if self.active_tab == Some(tab_id) {
            self.active_tab = if self.tabs.is_empty() {
                None
            } else {
                let new_index = if position > 0 { position - 1 } else { 0 };
                Some(self.tabs[new_index].id)
            };
        }

        Some(removed)
    }

    /// Set the active tab to the one matching the given identifier.
    ///
    /// Returns `true` if the tab was found and activated, `false` otherwise.
    pub fn activate_tab(&mut self, tab_id: TabId) -> bool {
        if self.tabs.iter().any(|t| t.id == tab_id) {
            self.active_tab = Some(tab_id);
            true
        } else {
            false
        }
    }

    /// Return a reference to the currently active tab, if any.
    pub fn active_tab(&self) -> Option<&Tab> {
        let active_id = self.active_tab?;
        self.tabs.iter().find(|t| t.id == active_id)
    }

    /// Return all tabs in this pane.
    pub fn tabs(&self) -> &[Tab] {
        &self.tabs
    }
}

/// A node in the split tree — either a leaf pane or an interior split.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum SplitNode {
    /// A terminal node holding a single pane.
    Leaf {
        /// The pane at this leaf.
        pane: Pane,
    },
    /// An interior node splitting space among children.
    Split {
        /// The direction this split divides space.
        direction: SplitDirection,
        /// Child nodes of this split.
        children: Vec<SplitNode>,
        /// Size ratios for each child (should sum to ~1.0).
        ratios: Vec<f32>,
    },
}

impl SplitNode {
    fn find_pane(&self, pane_id: PaneId) -> Option<&Pane> {
        match self {
            SplitNode::Leaf { pane } => {
                if pane.id == pane_id {
                    Some(pane)
                } else {
                    None
                }
            }
            SplitNode::Split { children, .. } => {
                children.iter().find_map(|child| child.find_pane(pane_id))
            }
        }
    }

    fn find_pane_mut(&mut self, pane_id: PaneId) -> Option<&mut Pane> {
        match self {
            SplitNode::Leaf { pane } => {
                if pane.id == pane_id {
                    Some(pane)
                } else {
                    None
                }
            }
            SplitNode::Split { children, .. } => children
                .iter_mut()
                .find_map(|child| child.find_pane_mut(pane_id)),
        }
    }

    fn split_at(
        &mut self,
        target_id: PaneId,
        direction: SplitDirection,
        new_pane: Pane,
    ) -> Result<()> {
        match self {
            SplitNode::Leaf { pane } if pane.id == target_id => {
                let existing = std::mem::replace(
                    self,
                    SplitNode::Split {
                        direction,
                        children: Vec::new(),
                        ratios: Vec::new(),
                    },
                );
                if let SplitNode::Split {
                    children, ratios, ..
                } = self
                {
                    children.push(existing);
                    children.push(SplitNode::Leaf { pane: new_pane });
                    ratios.push(0.5);
                    ratios.push(0.5);
                }
                Ok(())
            }
            SplitNode::Leaf { .. } => Err(anyhow!("pane not found")),
            SplitNode::Split { children, .. } => {
                for child in children.iter_mut() {
                    if child
                        .split_at(target_id, direction, new_pane.clone())
                        .is_ok()
                    {
                        return Ok(());
                    }
                }
                Err(anyhow!("pane not found"))
            }
        }
    }

    fn remove_pane(&mut self, pane_id: PaneId) -> Result<bool> {
        match self {
            SplitNode::Leaf { pane } => {
                if pane.id == pane_id {
                    Ok(true)
                } else {
                    Err(anyhow!("pane not found"))
                }
            }
            SplitNode::Split {
                children, ratios, ..
            } => {
                let mut found_index = None;
                for (index, child) in children.iter_mut().enumerate() {
                    match child.remove_pane(pane_id) {
                        Ok(true) => {
                            found_index = Some(index);
                            break;
                        }
                        Ok(false) => return Ok(false),
                        Err(_) => continue,
                    }
                }

                if let Some(index) = found_index {
                    children.remove(index);
                    ratios.remove(index);

                    if children.len() == 1 {
                        let remaining = children.remove(0);
                        *self = remaining;
                    } else {
                        let total: f32 = ratios.iter().sum();
                        if total > 0.0 {
                            for ratio in ratios.iter_mut() {
                                *ratio /= total;
                            }
                        }
                    }
                    Ok(false)
                } else {
                    Err(anyhow!("pane not found"))
                }
            }
        }
    }

    fn collect_panes<'a>(&'a self, out: &mut Vec<&'a Pane>) {
        match self {
            SplitNode::Leaf { pane } => out.push(pane),
            SplitNode::Split { children, .. } => {
                for child in children {
                    child.collect_panes(out);
                }
            }
        }
    }
}

/// Manages a tree of split panes for IDE-style layouts.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SplitTree {
    root: SplitNode,
}

impl SplitTree {
    /// Create a new split tree with a single root pane.
    pub fn new(root_pane: Pane) -> Self {
        Self {
            root: SplitNode::Leaf { pane: root_pane },
        }
    }

    /// Split an existing pane in the given direction, placing a new pane alongside it.
    pub fn split(
        &mut self,
        pane_id: PaneId,
        direction: SplitDirection,
        new_pane: Pane,
    ) -> Result<()> {
        self.root.split_at(pane_id, direction, new_pane)
    }

    /// Find a pane by its identifier.
    pub fn find_pane(&self, pane_id: PaneId) -> Option<&Pane> {
        self.root.find_pane(pane_id)
    }

    /// Find a pane by its identifier, returning a mutable reference.
    pub fn find_pane_mut(&mut self, pane_id: PaneId) -> Option<&mut Pane> {
        self.root.find_pane_mut(pane_id)
    }

    /// Remove a pane from the tree.
    ///
    /// If the parent split has only one child remaining after removal, the
    /// parent collapses to that single child.
    pub fn remove_pane(&mut self, pane_id: PaneId) -> Result<()> {
        match self.root.remove_pane(pane_id) {
            Ok(true) => Err(anyhow!("cannot remove the last pane in the tree")),
            Ok(false) => Ok(()),
            Err(err) => Err(err),
        }
    }

    /// Collect references to every pane in the tree.
    pub fn all_panes(&self) -> Vec<&Pane> {
        let mut panes = Vec::new();
        self.root.collect_panes(&mut panes);
        panes
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn make_tab(id: u64, label: &str) -> Tab {
        Tab {
            id: TabId(id),
            label: label.to_string(),
            closable: true,
        }
    }

    #[test]
    fn pane_add_and_activate_tab() {
        let mut pane = Pane::new(PaneId(1));
        assert!(pane.active_tab().is_none());
        assert!(pane.tabs().is_empty());

        pane.add_tab(make_tab(10, "First"));
        assert_eq!(pane.active_tab().unwrap().id, TabId(10));

        pane.add_tab(make_tab(20, "Second"));
        assert_eq!(pane.active_tab().unwrap().id, TabId(20));
        assert_eq!(pane.tabs().len(), 2);

        assert!(pane.activate_tab(TabId(10)));
        assert_eq!(pane.active_tab().unwrap().id, TabId(10));

        assert!(!pane.activate_tab(TabId(999)));
    }

    #[test]
    fn pane_close_tab_adjusts_active() {
        let mut pane = Pane::new(PaneId(1));
        pane.add_tab(make_tab(1, "A"));
        pane.add_tab(make_tab(2, "B"));
        pane.add_tab(make_tab(3, "C"));
        pane.activate_tab(TabId(2));

        let removed = pane.close_tab(TabId(2));
        assert!(removed.is_some());
        assert_eq!(removed.unwrap().label, "B");
        assert_eq!(pane.active_tab().unwrap().id, TabId(1));
        assert_eq!(pane.tabs().len(), 2);
    }

    #[test]
    fn pane_close_first_tab_activates_next() {
        let mut pane = Pane::new(PaneId(1));
        pane.add_tab(make_tab(1, "A"));
        pane.add_tab(make_tab(2, "B"));
        pane.activate_tab(TabId(1));

        pane.close_tab(TabId(1));
        assert_eq!(pane.active_tab().unwrap().id, TabId(2));
    }

    #[test]
    fn pane_close_last_remaining_tab() {
        let mut pane = Pane::new(PaneId(1));
        pane.add_tab(make_tab(1, "Only"));
        pane.close_tab(TabId(1));
        assert!(pane.active_tab().is_none());
        assert!(pane.tabs().is_empty());
    }

    #[test]
    fn pane_close_nonexistent_tab_returns_none() {
        let mut pane = Pane::new(PaneId(1));
        pane.add_tab(make_tab(1, "A"));
        assert!(pane.close_tab(TabId(999)).is_none());
    }

    #[test]
    fn split_tree_basic_operations() {
        let root = Pane::new(PaneId(1));
        let tree = SplitTree::new(root);

        assert!(tree.find_pane(PaneId(1)).is_some());
        assert!(tree.find_pane(PaneId(999)).is_none());
        assert_eq!(tree.all_panes().len(), 1);
    }

    #[test]
    fn split_tree_split_and_find() {
        let root = Pane::new(PaneId(1));
        let mut tree = SplitTree::new(root);

        let new_pane = Pane::new(PaneId(2));
        tree.split(PaneId(1), SplitDirection::Horizontal, new_pane)
            .unwrap();

        assert!(tree.find_pane(PaneId(1)).is_some());
        assert!(tree.find_pane(PaneId(2)).is_some());
        assert_eq!(tree.all_panes().len(), 2);
    }

    #[test]
    fn split_tree_nested_splits() {
        let mut tree = SplitTree::new(Pane::new(PaneId(1)));
        tree.split(PaneId(1), SplitDirection::Horizontal, Pane::new(PaneId(2)))
            .unwrap();
        tree.split(PaneId(2), SplitDirection::Vertical, Pane::new(PaneId(3)))
            .unwrap();

        assert_eq!(tree.all_panes().len(), 3);
        assert!(tree.find_pane(PaneId(3)).is_some());
    }

    #[test]
    fn split_tree_split_nonexistent_pane_fails() {
        let mut tree = SplitTree::new(Pane::new(PaneId(1)));
        let result = tree.split(
            PaneId(999),
            SplitDirection::Horizontal,
            Pane::new(PaneId(2)),
        );
        assert!(result.is_err());
    }

    #[test]
    fn split_tree_remove_pane_collapses_parent() {
        let mut tree = SplitTree::new(Pane::new(PaneId(1)));
        tree.split(PaneId(1), SplitDirection::Horizontal, Pane::new(PaneId(2)))
            .unwrap();

        tree.remove_pane(PaneId(2)).unwrap();
        assert_eq!(tree.all_panes().len(), 1);
        assert!(tree.find_pane(PaneId(1)).is_some());
        assert!(tree.find_pane(PaneId(2)).is_none());
    }

    #[test]
    fn split_tree_cannot_remove_last_pane() {
        let mut tree = SplitTree::new(Pane::new(PaneId(1)));
        let result = tree.remove_pane(PaneId(1));
        assert!(result.is_err());
    }

    #[test]
    fn split_tree_remove_nonexistent_pane_fails() {
        let mut tree = SplitTree::new(Pane::new(PaneId(1)));
        assert!(tree.remove_pane(PaneId(999)).is_err());
    }

    #[test]
    fn split_tree_find_pane_mut() {
        let mut tree = SplitTree::new(Pane::new(PaneId(1)));
        let pane = tree.find_pane_mut(PaneId(1)).unwrap();
        pane.add_tab(make_tab(10, "Hello"));
        assert_eq!(tree.find_pane(PaneId(1)).unwrap().tabs().len(), 1);
    }

    #[test]
    fn split_tree_remove_from_three_children() {
        let mut tree = SplitTree::new(Pane::new(PaneId(1)));
        tree.split(PaneId(1), SplitDirection::Horizontal, Pane::new(PaneId(2)))
            .unwrap();

        if let SplitNode::Split {
            children, ratios, ..
        } = &mut tree.root
        {
            children.push(SplitNode::Leaf {
                pane: Pane::new(PaneId(3)),
            });
            ratios.push(0.33);
            for ratio in ratios.iter_mut() {
                *ratio = 1.0 / 3.0;
            }
        }

        assert_eq!(tree.all_panes().len(), 3);
        tree.remove_pane(PaneId(2)).unwrap();
        assert_eq!(tree.all_panes().len(), 2);
    }

    #[test]
    fn split_tree_serialization_roundtrip() {
        let mut tree = SplitTree::new(Pane::new(PaneId(1)));
        tree.find_pane_mut(PaneId(1))
            .unwrap()
            .add_tab(make_tab(10, "Test"));
        tree.split(PaneId(1), SplitDirection::Vertical, Pane::new(PaneId(2)))
            .unwrap();

        let json = serde_json::to_string(&tree).unwrap();
        let restored: SplitTree = serde_json::from_str(&json).unwrap();

        assert_eq!(restored.all_panes().len(), 2);
        assert_eq!(restored.find_pane(PaneId(1)).unwrap().tabs().len(), 1);
    }
}
