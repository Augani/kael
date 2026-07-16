use anyhow::{Result, anyhow};
use serde::{Deserialize, Deserializer, Serialize, de::Error as _};
use std::collections::HashSet;

const MAX_TABS_PER_PANE: usize = 1_024;
const MAX_SPLIT_PANES: usize = 4_096;
const MAX_SPLIT_DEPTH: usize = 256;
const MAX_TAB_LABEL_BYTES: usize = 1_024;

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
        let _ = self.add_tab_checked(tab);
    }

    /// Add a validated, uniquely identified tab and make it active.
    pub fn add_tab_checked(&mut self, tab: Tab) -> Result<()> {
        validate_tab(&tab)?;
        anyhow::ensure!(
            self.tabs.len() < MAX_TABS_PER_PANE,
            "pane cannot exceed {MAX_TABS_PER_PANE} tabs"
        );
        anyhow::ensure!(
            !self.tabs.iter().any(|existing| existing.id == tab.id),
            "tab identifier is already present in pane"
        );
        let tab_id = tab.id;
        self.tabs.push(tab);
        self.active_tab = Some(tab_id);
        Ok(())
    }

    /// Close and remove a tab by its identifier, returning it if found.
    ///
    /// If the closed tab was active, activates the previous tab (or the next
    /// one if it was the first tab).
    pub fn close_tab(&mut self, tab_id: TabId) -> Option<Tab> {
        let position = self.tabs.iter().position(|t| t.id == tab_id)?;
        if !self.tabs[position].closable {
            return None;
        }
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

fn validate_tab(tab: &Tab) -> Result<()> {
    anyhow::ensure!(tab.id.0 != 0, "tab identifier cannot be zero");
    anyhow::ensure!(!tab.label.trim().is_empty(), "tab label cannot be empty");
    anyhow::ensure!(
        tab.label.len() <= MAX_TAB_LABEL_BYTES,
        "tab label cannot exceed {MAX_TAB_LABEL_BYTES} bytes"
    );
    anyhow::ensure!(
        !tab.label.chars().any(char::is_control),
        "tab label cannot contain control characters"
    );
    Ok(())
}

fn validate_pane(pane: &Pane) -> Result<()> {
    anyhow::ensure!(pane.id.0 != 0, "pane identifier cannot be zero");
    anyhow::ensure!(
        pane.tabs.len() <= MAX_TABS_PER_PANE,
        "pane cannot exceed {MAX_TABS_PER_PANE} tabs"
    );
    let mut tab_ids = HashSet::new();
    for tab in &pane.tabs {
        validate_tab(tab)?;
        anyhow::ensure!(
            tab_ids.insert(tab.id),
            "pane contains duplicate tab identifiers"
        );
    }
    anyhow::ensure!(
        pane.active_tab
            .is_none_or(|active| tab_ids.contains(&active)),
        "active tab does not exist in pane"
    );
    Ok(())
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
                    if child.find_pane(target_id).is_some() {
                        child.split_at(target_id, direction, new_pane)?;
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
                    if index < ratios.len() {
                        ratios.remove(index);
                    }

                    if children.len() == 1 {
                        let remaining = children.remove(0);
                        *self = remaining;
                    } else {
                        let total = ratios.iter().map(|ratio| f64::from(*ratio)).sum::<f64>();
                        if total.is_finite() && total > 0.0 {
                            for ratio in ratios.iter_mut() {
                                *ratio = (f64::from(*ratio) / total) as f32;
                            }
                        } else {
                            let equal = 1.0 / children.len() as f32;
                            ratios.clear();
                            ratios.resize(children.len(), equal);
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
#[derive(Debug, Clone, Serialize)]
pub struct SplitTree {
    root: SplitNode,
}

impl<'de> Deserialize<'de> for SplitTree {
    fn deserialize<D>(deserializer: D) -> std::result::Result<Self, D::Error>
    where
        D: Deserializer<'de>,
    {
        #[derive(Deserialize)]
        struct SerializedSplitTree {
            root: SplitNode,
        }

        let serialized = SerializedSplitTree::deserialize(deserializer)?;
        let tree = Self {
            root: serialized.root,
        };
        tree.validate().map_err(D::Error::custom)?;
        Ok(tree)
    }
}

impl SplitTree {
    /// Create a new split tree with a single root pane.
    pub fn new(root_pane: Pane) -> Self {
        Self {
            root: SplitNode::Leaf { pane: root_pane },
        }
    }

    /// Create a split tree after validating its root pane.
    pub fn new_checked(root_pane: Pane) -> Result<Self> {
        validate_pane(&root_pane)?;
        Ok(Self::new(root_pane))
    }

    /// Split an existing pane in the given direction, placing a new pane alongside it.
    pub fn split(
        &mut self,
        pane_id: PaneId,
        direction: SplitDirection,
        new_pane: Pane,
    ) -> Result<()> {
        self.validate()?;
        validate_pane(&new_pane)?;
        anyhow::ensure!(
            self.find_pane(new_pane.id).is_none(),
            "pane identifier is already present in split tree"
        );
        let (pane_count, target_depth) = self.layout_stats(Some(pane_id))?;
        anyhow::ensure!(
            pane_count < MAX_SPLIT_PANES,
            "split tree cannot exceed {MAX_SPLIT_PANES} panes"
        );
        let target_depth = target_depth.ok_or_else(|| anyhow!("pane not found"))?;
        anyhow::ensure!(
            target_depth < MAX_SPLIT_DEPTH,
            "split tree cannot exceed depth {MAX_SPLIT_DEPTH}"
        );
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
        self.validate()?;
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

    /// Validate pane/tab identifiers, ratios, tree shape, count, and depth.
    pub fn validate(&self) -> Result<()> {
        self.layout_stats(None).map(|_| ())
    }

    fn layout_stats(&self, target: Option<PaneId>) -> Result<(usize, Option<usize>)> {
        let mut pane_ids = HashSet::new();
        let mut pane_count = 0;
        let mut target_depth = None;
        let mut pending = vec![(&self.root, 1_usize)];
        while let Some((node, depth)) = pending.pop() {
            anyhow::ensure!(
                depth <= MAX_SPLIT_DEPTH,
                "split tree cannot exceed depth {MAX_SPLIT_DEPTH}"
            );
            match node {
                SplitNode::Leaf { pane } => {
                    validate_pane(pane)?;
                    anyhow::ensure!(
                        pane_ids.insert(pane.id),
                        "split tree contains duplicate pane identifiers"
                    );
                    pane_count += 1;
                    anyhow::ensure!(
                        pane_count <= MAX_SPLIT_PANES,
                        "split tree cannot exceed {MAX_SPLIT_PANES} panes"
                    );
                    if target == Some(pane.id) {
                        target_depth = Some(depth);
                    }
                }
                SplitNode::Split {
                    children, ratios, ..
                } => {
                    anyhow::ensure!(
                        children.len() >= 2,
                        "split node must contain at least two children"
                    );
                    anyhow::ensure!(
                        children.len() == ratios.len(),
                        "split ratios must match child count"
                    );
                    anyhow::ensure!(
                        ratios.iter().all(|ratio| ratio.is_finite() && *ratio > 0.0),
                        "split ratios must be finite and positive"
                    );
                    let total = ratios.iter().map(|ratio| f64::from(*ratio)).sum::<f64>();
                    anyhow::ensure!((total - 1.0).abs() <= 0.01, "split ratios must sum to one");
                    pending.extend(children.iter().rev().map(|child| (child, depth + 1)));
                }
            }
        }
        Ok((pane_count, target_depth))
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

    #[test]
    fn non_closable_tabs_are_preserved() {
        let mut pane = Pane::new(PaneId(1));
        pane.add_tab(Tab {
            id: TabId(1),
            label: "Pinned".into(),
            closable: false,
        });

        assert!(pane.close_tab(TabId(1)).is_none());
        assert_eq!(pane.tabs().len(), 1);
        assert_eq!(pane.active_tab().map(|tab| tab.id), Some(TabId(1)));
    }

    #[test]
    fn checked_tab_and_pane_insertion_rejects_invalid_identity() {
        let mut pane = Pane::new(PaneId(1));
        assert!(
            pane.add_tab_checked(Tab {
                id: TabId(0),
                label: "Invalid".into(),
                closable: true,
            })
            .is_err()
        );
        pane.add_tab_checked(make_tab(1, "One")).unwrap();
        assert!(pane.add_tab_checked(make_tab(1, "Duplicate")).is_err());
        assert_eq!(pane.tabs().len(), 1);

        assert!(SplitTree::new_checked(Pane::new(PaneId(0))).is_err());
        let mut tree = SplitTree::new_checked(Pane::new(PaneId(1))).unwrap();
        assert!(
            tree.split(PaneId(1), SplitDirection::Horizontal, Pane::new(PaneId(1)),)
                .is_err()
        );
        assert_eq!(tree.all_panes().len(), 1);
    }

    #[test]
    fn persisted_split_layouts_validate_shape_ratios_and_active_tabs() {
        let ratio_mismatch = r#"{
            "root": {
                "Split": {
                    "direction": "Horizontal",
                    "children": [
                        {"Leaf":{"pane":{"id":1,"tabs":[],"active_tab":null}}},
                        {"Leaf":{"pane":{"id":2,"tabs":[],"active_tab":null}}}
                    ],
                    "ratios": [1.0]
                }
            }
        }"#;
        assert!(serde_json::from_str::<SplitTree>(ratio_mismatch).is_err());

        let zero_ratio = ratio_mismatch.replace("[1.0]", "[0.0, 1.0]");
        assert!(serde_json::from_str::<SplitTree>(&zero_ratio).is_err());

        let missing_active = r#"{
            "root": {
                "Leaf": {
                    "pane": {
                        "id": 1,
                        "tabs": [{"id":1,"label":"One","closable":true}],
                        "active_tab": 2
                    }
                }
            }
        }"#;
        assert!(serde_json::from_str::<SplitTree>(missing_active).is_err());
    }
}
