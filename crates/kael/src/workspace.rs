use std::collections::HashMap;

use anyhow::{Context as _, Result, anyhow};
use serde::{Deserialize, Serialize};

use crate::{Bounds, Pixels, Size, point, px, size};

/// A region of the workspace where panels can be docked.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum DockArea {
    /// The left side of the workspace.
    Left,
    /// The right side of the workspace.
    Right,
    /// The bottom of the workspace.
    Bottom,
    /// The central content area.
    #[default]
    Center,
}

/// A group of tabs within a single dock area.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
pub struct TabGroup {
    /// The dock area this group occupies.
    pub dock_area: DockArea,
    /// Identifiers of panels in this group.
    pub panel_ids: Vec<String>,
    /// The currently active panel in this group.
    pub active_panel_id: Option<String>,
}

impl TabGroup {
    /// Creates a new empty tab group for the given dock area.
    pub fn new(dock_area: DockArea) -> Self {
        Self {
            dock_area,
            panel_ids: Vec::new(),
            active_panel_id: None,
        }
    }

    /// Adds a panel to this group if not already present.
    pub fn add_panel(&mut self, panel_id: impl Into<String>) {
        let id = panel_id.into();
        if self.panel_ids.contains(&id) {
            return;
        }
        self.panel_ids.push(id.clone());
        if self.active_panel_id.is_none() {
            self.active_panel_id = Some(id);
        }
    }

    /// Removes a panel from this group.
    pub fn remove_panel(&mut self, panel_id: &str) {
        self.panel_ids.retain(|id| id != panel_id);
        if self.active_panel_id.as_deref() == Some(panel_id) {
            self.active_panel_id = self.panel_ids.first().cloned();
        }
    }

    /// Sets the active panel if it exists in this group.
    pub fn set_active(&mut self, panel_id: &str) {
        if self.panel_ids.iter().any(|id| id == panel_id) {
            self.active_panel_id = Some(panel_id.to_string());
        }
    }
}

/// A dockable panel within a workspace.
pub trait Panel: Send + Sync {
    /// Returns the unique identifier for this panel.
    fn id(&self) -> &str;
    /// Returns the human-readable title for this panel.
    fn title(&self) -> &str;
    /// Returns the dock area this panel prefers.
    fn dock_area(&self) -> DockArea;
}

/// Serializable representation of a workspace layout.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
pub struct WorkspaceLayout {
    /// Mapping from panel ID to dock area.
    pub panel_positions: HashMap<String, DockArea>,
    /// Tab groups in the layout.
    pub tab_groups: Vec<TabGroup>,
    /// Relative sizes for each dock area.
    pub sizes: HashMap<DockArea, f32>,
}

/// Computed bounds for each dock area within a window.
#[derive(Debug, Clone)]
pub struct DockLayout {
    /// Bounds for the left panel area, if any.
    pub left: Option<Bounds<Pixels>>,
    /// Bounds for the right panel area, if any.
    pub right: Option<Bounds<Pixels>>,
    /// Bounds for the bottom panel area, if any.
    pub bottom: Option<Bounds<Pixels>>,
    /// Bounds for the central content area.
    pub center: Bounds<Pixels>,
}

/// A multi-pane workspace with dockable panels and layout persistence.
pub struct Workspace {
    panels: HashMap<String, Box<dyn Panel>>,
    tab_groups: Vec<TabGroup>,
    layout: WorkspaceLayout,
}

impl std::fmt::Debug for Workspace {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Workspace")
            .field("panel_count", &self.panels.len())
            .field("tab_groups", &self.tab_groups)
            .field("layout", &self.layout)
            .finish()
    }
}

impl Workspace {
    /// Creates a new empty workspace.
    pub fn new() -> Self {
        Self {
            panels: HashMap::new(),
            tab_groups: Vec::new(),
            layout: WorkspaceLayout::default(),
        }
    }

    /// Adds a panel to the workspace.
    pub fn add_panel(&mut self, panel: Box<dyn Panel>) {
        let id = panel.id().to_string();
        let area = panel.dock_area();
        self.panels.insert(id.clone(), panel);
        self.layout.panel_positions.insert(id.clone(), area);

        let group = self.tab_groups.iter_mut().find(|g| g.dock_area == area);
        if let Some(group) = group {
            group.add_panel(&id);
        } else {
            let mut new_group = TabGroup::new(area);
            new_group.add_panel(&id);
            self.tab_groups.push(new_group);
        }
    }

    /// Removes a panel from the workspace.
    pub fn remove_panel(&mut self, panel_id: &str) -> Option<Box<dyn Panel>> {
        self.layout.panel_positions.remove(panel_id);
        for group in &mut self.tab_groups {
            group.remove_panel(panel_id);
        }
        self.tab_groups.retain(|g| !g.panel_ids.is_empty());
        self.panels.remove(panel_id)
    }

    /// Moves a panel to a different dock area.
    pub fn move_panel(&mut self, panel_id: &str, target: DockArea) -> Result<()> {
        if !self.panels.contains_key(panel_id) {
            return Err(anyhow!("panel not found: {}", panel_id));
        }

        self.layout
            .panel_positions
            .insert(panel_id.to_string(), target);

        for group in &mut self.tab_groups {
            group.remove_panel(panel_id);
        }
        self.tab_groups.retain(|g| !g.panel_ids.is_empty());

        let group = self.tab_groups.iter_mut().find(|g| g.dock_area == target);
        if let Some(group) = group {
            group.add_panel(panel_id);
        } else {
            let mut new_group = TabGroup::new(target);
            new_group.add_panel(panel_id);
            self.tab_groups.push(new_group);
        }

        Ok(())
    }

    /// Returns a reference to a panel by ID.
    pub fn panel(&self, id: &str) -> Option<&dyn Panel> {
        self.panels.get(id).map(|b| b.as_ref())
    }

    /// Returns all panels in the workspace.
    pub fn panels(&self) -> Vec<&dyn Panel> {
        self.panels.values().map(|b| b.as_ref()).collect()
    }

    /// Returns all tab groups.
    pub fn tab_groups(&self) -> &[TabGroup] {
        &self.tab_groups
    }

    /// Returns mutable access to all tab groups.
    pub fn tab_groups_mut(&mut self) -> &mut [TabGroup] {
        &mut self.tab_groups
    }

    /// Serializes the current layout to a JSON string.
    pub fn save_layout(&self) -> Result<String> {
        let layout = WorkspaceLayout {
            panel_positions: self.layout.panel_positions.clone(),
            tab_groups: self.tab_groups.clone(),
            sizes: self.layout.sizes.clone(),
        };
        serde_json::to_string_pretty(&layout).context("failed to serialize workspace layout")
    }

    /// Restores layout from a previously saved JSON string.
    pub fn restore_layout(&mut self, json: &str) -> Result<()> {
        let layout: WorkspaceLayout =
            serde_json::from_str(json).context("failed to deserialize workspace layout")?;
        self.layout = layout.clone();
        self.tab_groups = layout.tab_groups;

        for (panel_id, area) in &self.layout.panel_positions {
            if !self.panels.contains_key(panel_id) {
                continue;
            }
            let group = self.tab_groups.iter_mut().find(|g| g.dock_area == *area);
            if let Some(group) = group {
                if !group.panel_ids.contains(panel_id) {
                    group.add_panel(panel_id);
                }
            } else {
                let mut new_group = TabGroup::new(*area);
                new_group.add_panel(panel_id);
                self.tab_groups.push(new_group);
            }
        }

        Ok(())
    }

    /// Returns the current workspace layout.
    pub fn layout(&self) -> &WorkspaceLayout {
        &self.layout
    }

    /// Returns mutable access to the workspace layout.
    pub fn layout_mut(&mut self) -> &mut WorkspaceLayout {
        &mut self.layout
    }

    /// Computes window bounds for each dock area based on the current layout.
    pub fn compute_dock_layout(&self, viewport: Size<Pixels>) -> DockLayout {
        let left_ratio = self
            .layout
            .sizes
            .get(&DockArea::Left)
            .copied()
            .unwrap_or(0.2);
        let right_ratio = self
            .layout
            .sizes
            .get(&DockArea::Right)
            .copied()
            .unwrap_or(0.2);
        let bottom_ratio = self
            .layout
            .sizes
            .get(&DockArea::Bottom)
            .copied()
            .unwrap_or(0.2);

        let left_width = left_ratio * viewport.width.0;
        let right_width = right_ratio * viewport.width.0;
        let bottom_height = bottom_ratio * viewport.height.0;

        let has_left = self
            .tab_groups
            .iter()
            .any(|g| g.dock_area == DockArea::Left && !g.panel_ids.is_empty());
        let has_right = self
            .tab_groups
            .iter()
            .any(|g| g.dock_area == DockArea::Right && !g.panel_ids.is_empty());
        let has_bottom = self
            .tab_groups
            .iter()
            .any(|g| g.dock_area == DockArea::Bottom && !g.panel_ids.is_empty());

        let left = if has_left {
            Some(Bounds::new(
                point(px(0.0), px(0.0)),
                size(px(left_width), viewport.height),
            ))
        } else {
            None
        };

        let right = if has_right {
            Some(Bounds::new(
                point(px(viewport.width.0 - right_width), px(0.0)),
                size(px(right_width), viewport.height),
            ))
        } else {
            None
        };

        let center_x = if has_left { left_width } else { 0.0 };
        let center_width = viewport.width.0 - left_width - right_width;
        let center_height = if has_bottom {
            viewport.height.0 - bottom_height
        } else {
            viewport.height.0
        };

        let bottom = if has_bottom {
            Some(Bounds::new(
                point(px(center_x), px(viewport.height.0 - bottom_height)),
                size(px(center_width), px(bottom_height)),
            ))
        } else {
            None
        };

        let center = Bounds::new(
            point(px(center_x), px(0.0)),
            size(px(center_width), px(center_height)),
        );

        DockLayout {
            left,
            right,
            bottom,
            center,
        }
    }
}

impl Default for Workspace {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    struct TestPanel {
        id: String,
        title: String,
        area: DockArea,
    }

    impl Panel for TestPanel {
        fn id(&self) -> &str {
            &self.id
        }

        fn title(&self) -> &str {
            &self.title
        }

        fn dock_area(&self) -> DockArea {
            self.area
        }
    }

    #[test]
    fn test_workspace_add_remove_panel() {
        let mut workspace = Workspace::new();
        workspace.add_panel(Box::new(TestPanel {
            id: "p1".to_string(),
            title: "Panel 1".to_string(),
            area: DockArea::Left,
        }));

        assert!(workspace.panel("p1").is_some());
        assert_eq!(workspace.tab_groups().len(), 1);
        assert_eq!(workspace.tab_groups()[0].panel_ids, vec!["p1"]);

        workspace.add_panel(Box::new(TestPanel {
            id: "p2".to_string(),
            title: "Panel 2".to_string(),
            area: DockArea::Left,
        }));

        assert_eq!(workspace.tab_groups()[0].panel_ids, vec!["p1", "p2"]);

        let removed = workspace.remove_panel("p1");
        assert!(removed.is_some());
        assert!(workspace.panel("p1").is_none());
        assert_eq!(workspace.tab_groups()[0].panel_ids, vec!["p2"]);
    }

    #[test]
    fn test_workspace_move_panel() {
        let mut workspace = Workspace::new();
        workspace.add_panel(Box::new(TestPanel {
            id: "p1".to_string(),
            title: "Panel 1".to_string(),
            area: DockArea::Left,
        }));

        workspace.move_panel("p1", DockArea::Right).unwrap();
        assert_eq!(workspace.tab_groups().len(), 1);
        assert_eq!(workspace.tab_groups()[0].dock_area, DockArea::Right);
    }

    #[test]
    fn test_workspace_layout_persistence() {
        let mut workspace = Workspace::new();
        workspace.add_panel(Box::new(TestPanel {
            id: "p1".to_string(),
            title: "Panel 1".to_string(),
            area: DockArea::Left,
        }));
        workspace.add_panel(Box::new(TestPanel {
            id: "p2".to_string(),
            title: "Panel 2".to_string(),
            area: DockArea::Bottom,
        }));

        workspace.layout_mut().sizes.insert(DockArea::Left, 0.25);
        let json = workspace.save_layout().unwrap();

        let mut restored = Workspace::new();
        restored.add_panel(Box::new(TestPanel {
            id: "p1".to_string(),
            title: "Panel 1".to_string(),
            area: DockArea::Center,
        }));
        restored.add_panel(Box::new(TestPanel {
            id: "p2".to_string(),
            title: "Panel 2".to_string(),
            area: DockArea::Center,
        }));
        restored.restore_layout(&json).unwrap();

        assert_eq!(
            restored.layout().panel_positions.get("p1"),
            Some(&DockArea::Left)
        );
        assert_eq!(
            restored.layout().panel_positions.get("p2"),
            Some(&DockArea::Bottom)
        );
        assert_eq!(restored.layout().sizes.get(&DockArea::Left), Some(&0.25));
    }

    #[test]
    fn test_dock_layout_computation() {
        let mut workspace = Workspace::new();
        workspace.add_panel(Box::new(TestPanel {
            id: "p1".to_string(),
            title: "Panel 1".to_string(),
            area: DockArea::Left,
        }));

        let layout = workspace.compute_dock_layout(size(px(1000.0), px(800.0)));
        assert!(layout.left.is_some());
        assert!(layout.right.is_none());
        assert!(layout.bottom.is_none());
        assert_eq!(layout.center.origin.x.0, 200.0);
    }
}
