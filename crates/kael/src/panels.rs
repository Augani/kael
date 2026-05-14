use crate::workspace::{DockArea, Panel};

/// A panel docked to the left side of the workspace.
pub struct SidebarPanel {
    id: String,
    title: String,
}

impl SidebarPanel {
    /// Creates a new sidebar panel with the given identifier and title.
    pub fn new(id: impl Into<String>, title: impl Into<String>) -> Self {
        Self {
            id: id.into(),
            title: title.into(),
        }
    }
}

impl Panel for SidebarPanel {
    fn id(&self) -> &str {
        &self.id
    }

    fn title(&self) -> &str {
        &self.title
    }

    fn dock_area(&self) -> DockArea {
        DockArea::Left
    }
}

/// A panel docked to the right side of the workspace.
pub struct InspectorPanel {
    id: String,
    title: String,
}

impl InspectorPanel {
    /// Creates a new inspector panel with the given identifier and title.
    pub fn new(id: impl Into<String>, title: impl Into<String>) -> Self {
        Self {
            id: id.into(),
            title: title.into(),
        }
    }
}

impl Panel for InspectorPanel {
    fn id(&self) -> &str {
        &self.id
    }

    fn title(&self) -> &str {
        &self.title
    }

    fn dock_area(&self) -> DockArea {
        DockArea::Right
    }
}

/// A panel docked to the bottom of the workspace.
pub struct BottomPanel {
    id: String,
    title: String,
}

impl BottomPanel {
    /// Creates a new bottom panel with the given identifier and title.
    pub fn new(id: impl Into<String>, title: impl Into<String>) -> Self {
        Self {
            id: id.into(),
            title: title.into(),
        }
    }
}

impl Panel for BottomPanel {
    fn id(&self) -> &str {
        &self.id
    }

    fn title(&self) -> &str {
        &self.title
    }

    fn dock_area(&self) -> DockArea {
        DockArea::Bottom
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_sidebar_panel() {
        let panel = SidebarPanel::new("sidebar", "Sidebar");
        assert_eq!(panel.id(), "sidebar");
        assert_eq!(panel.title(), "Sidebar");
        assert!(matches!(panel.dock_area(), DockArea::Left));
    }

    #[test]
    fn test_inspector_panel() {
        let panel = InspectorPanel::new("inspector", "Inspector");
        assert_eq!(panel.id(), "inspector");
        assert_eq!(panel.title(), "Inspector");
        assert!(matches!(panel.dock_area(), DockArea::Right));
    }

    #[test]
    fn test_bottom_panel() {
        let panel = BottomPanel::new("bottom", "Bottom");
        assert_eq!(panel.id(), "bottom");
        assert_eq!(panel.title(), "Bottom");
        assert!(matches!(panel.dock_area(), DockArea::Bottom));
    }
}
