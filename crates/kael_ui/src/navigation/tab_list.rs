//! TabList component - ASTRYX-named tab surface.

use crate::navigation::tabs::{TabItem, TabPanel, TabVariant, Tabs, TabsLayout, TabsSize};
use kael::{prelude::FluentBuilder as _, *};
use std::sync::Arc;

pub type Tab = TabItem<SharedString>;
pub type TabListSize = TabsSize;
pub type TabListLayout = TabsLayout;

#[derive(IntoElement)]
pub struct TabList {
    tabs: Vec<TabItem<SharedString>>,
    panels: Vec<TabPanel>,
    selected_id: Option<SharedString>,
    variant: TabVariant,
    size: TabListSize,
    layout: TabListLayout,
    has_divider: bool,
    on_change: Option<Arc<dyn Fn(&SharedString, &mut Window, &mut App) + Send + Sync + 'static>>,
    style: StyleRefinement,
}

impl TabList {
    pub fn new() -> Self {
        Self {
            tabs: Vec::new(),
            panels: Vec::new(),
            selected_id: None,
            variant: TabVariant::Underline,
            size: TabListSize::default(),
            layout: TabListLayout::default(),
            has_divider: false,
            on_change: None,
            style: StyleRefinement::default(),
        }
    }

    pub fn tab(mut self, id: impl Into<SharedString>, label: impl Into<SharedString>) -> Self {
        self.tabs.push(TabItem::new(id.into(), label));
        self
    }

    pub fn tabs(mut self, tabs: Vec<TabItem<SharedString>>) -> Self {
        self.tabs = tabs;
        self
    }

    pub fn panel(mut self, panel: TabPanel) -> Self {
        self.panels.push(panel);
        self
    }

    pub fn panels(mut self, panels: Vec<TabPanel>) -> Self {
        self.panels = panels;
        self
    }

    pub fn selected_id(mut self, id: impl Into<SharedString>) -> Self {
        self.selected_id = Some(id.into());
        self
    }

    pub fn value(self, id: impl Into<SharedString>) -> Self {
        self.selected_id(id)
    }

    pub fn variant(mut self, variant: TabVariant) -> Self {
        self.variant = variant;
        self
    }

    pub fn size(mut self, size: TabListSize) -> Self {
        self.size = size;
        self
    }

    pub fn layout(mut self, layout: TabListLayout) -> Self {
        self.layout = layout;
        self
    }

    pub fn has_divider(mut self, has_divider: bool) -> Self {
        self.has_divider = has_divider;
        self
    }

    #[allow(non_snake_case)]
    pub fn hasDivider(self, has_divider: bool) -> Self {
        self.has_divider(has_divider)
    }

    pub fn on_change(
        mut self,
        handler: impl Fn(&SharedString, &mut Window, &mut App) + Send + Sync + 'static,
    ) -> Self {
        self.on_change = Some(Arc::new(handler));
        self
    }
}

impl Default for TabList {
    fn default() -> Self {
        Self::new()
    }
}

impl Styled for TabList {
    fn style(&mut self) -> &mut StyleRefinement {
        &mut self.style
    }
}

impl RenderOnce for TabList {
    fn render(self, _window: &mut Window, _cx: &mut App) -> impl IntoElement {
        let ids: Vec<SharedString> = self.tabs.iter().map(|tab| tab.id.clone()).collect();
        let on_change = self.on_change.clone();
        let mut tabs = Tabs::new()
            .tabs(self.tabs)
            .panels(self.panels)
            .variant(self.variant)
            .size(self.size)
            .layout(self.layout)
            .has_divider(self.has_divider);

        if let Some(selected_id) = self.selected_id {
            tabs = tabs.selected_id(selected_id);
        }

        if let Some(handler) = on_change {
            tabs = tabs.on_change(move |index, window, cx| {
                if let Some(id) = ids.get(*index) {
                    handler(id, window, cx);
                }
            });
        }

        tabs.map(|this| {
            let mut tabs = this;
            tabs.style().refine(&self.style);
            tabs
        })
    }
}
