//! Breadcrumb navigation component for hierarchical navigation.

use crate::components::icon::Icon;
use crate::components::icon_source::IconSource;
use crate::theme::Theme;
use kael::{prelude::FluentBuilder as _, *};
use std::sync::Arc;

pub struct BreadcrumbItem<T> {
    pub id: T,
    pub label: SharedString,
    pub icon: Option<IconSource>,
    pub href: Option<SharedString>,
    pub is_current: bool,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Default)]
pub enum BreadcrumbsVariant {
    #[default]
    Default,
    Supporting,
}

pub type BreadcrumbsVariantMap = BreadcrumbsVariant;
pub type BreadcrumbsProps<T> = Breadcrumbs<T>;
pub type BreadcrumbItemProps<T> = BreadcrumbItem<T>;

impl<T> BreadcrumbItem<T> {
    pub fn new(id: T, label: impl Into<SharedString>) -> Self {
        Self {
            id,
            label: label.into(),
            icon: None,
            href: None,
            is_current: false,
        }
    }

    pub fn icon(mut self, icon: impl Into<IconSource>) -> Self {
        self.icon = Some(icon.into());
        self
    }

    pub fn start_icon(self, icon: impl Into<IconSource>) -> Self {
        self.icon(icon)
    }

    #[allow(non_snake_case)]
    pub fn startIcon(self, icon: impl Into<IconSource>) -> Self {
        self.start_icon(icon)
    }

    pub fn href(mut self, href: impl Into<SharedString>) -> Self {
        self.href = Some(href.into());
        self
    }

    pub fn is_current(mut self, is_current: bool) -> Self {
        self.is_current = is_current;
        self
    }

    #[allow(non_snake_case)]
    pub fn isCurrent(self, is_current: bool) -> Self {
        self.is_current(is_current)
    }
}

#[derive(IntoElement)]
pub struct Breadcrumbs<T: Clone + 'static> {
    items: Vec<BreadcrumbItem<T>>,
    variant: BreadcrumbsVariant,
    separator: SharedString,
    label: SharedString,
    on_click: Option<Arc<dyn Fn(&T, &mut Window, &mut App) + Send + Sync + 'static>>,
    style: StyleRefinement,
}

impl<T: Clone + 'static> Breadcrumbs<T> {
    pub fn new(_cx: &mut App) -> Self {
        Self {
            items: Vec::new(),
            variant: BreadcrumbsVariant::default(),
            separator: "/".into(),
            label: "Breadcrumb".into(),
            on_click: None,
            style: StyleRefinement::default(),
        }
    }

    pub fn items(mut self, items: Vec<BreadcrumbItem<T>>) -> Self {
        self.items = items;
        self
    }

    pub fn variant(mut self, variant: BreadcrumbsVariant) -> Self {
        self.variant = variant;
        self
    }

    pub fn separator(mut self, separator: impl Into<SharedString>) -> Self {
        self.separator = separator.into();
        self
    }

    pub fn label(mut self, label: impl Into<SharedString>) -> Self {
        self.label = label.into();
        self
    }

    pub fn on_click<F: Fn(&T, &mut Window, &mut App) + Send + Sync + 'static>(
        mut self,
        f: F,
    ) -> Self {
        self.on_click = Some(Arc::new(f));
        self
    }
}

impl<T: Clone + 'static> Styled for Breadcrumbs<T> {
    fn style(&mut self) -> &mut StyleRefinement {
        &mut self.style
    }
}

impl<T: Clone + 'static> RenderOnce for Breadcrumbs<T> {
    fn render(self, _window: &mut Window, cx: &mut App) -> impl IntoElement {
        let theme = Theme::of(cx);
        let user_style = self.style;
        let text_size = match self.variant {
            BreadcrumbsVariant::Default => px(14.0),
            BreadcrumbsVariant::Supporting => px(12.0),
        };
        let line_height = match self.variant {
            BreadcrumbsVariant::Default => px(20.0),
            BreadcrumbsVariant::Supporting => px(16.0),
        };
        let icon_size = match self.variant {
            BreadcrumbsVariant::Default => px(14.0),
            BreadcrumbsVariant::Supporting => px(12.0),
        };
        let separator = self.separator.clone();

        if self.items.is_empty() {
            return div();
        }

        let mut elements: Vec<kael::Div> = Vec::new();
        let on_click = self.on_click.clone();

        for (index, item) in self.items.iter().enumerate() {
            let item_id = item.id.clone();
            let is_last = index == self.items.len() - 1;

            if index > 0 {
                let separator = div()
                    .flex()
                    .items_center()
                    .py(px(4.0))
                    .text_size(text_size)
                    .line_height(line_height)
                    .text_color(theme.tokens.muted_foreground)
                    .child(separator.clone());
                elements.push(separator);
            }

            let mut breadcrumb_element = div()
                .flex()
                .items_center()
                .gap(px(4.0))
                .py(px(4.0))
                .text_size(text_size)
                .line_height(line_height)
                .font_family(theme.tokens.font_family.clone());

            if let Some(icon_source) = &item.icon {
                breadcrumb_element =
                    breadcrumb_element.child(Icon::new(icon_source.clone()).size(icon_size).color(
                        if is_last {
                            match self.variant {
                                BreadcrumbsVariant::Default => theme.tokens.foreground,
                                BreadcrumbsVariant::Supporting => theme.tokens.muted_foreground,
                            }
                        } else {
                            theme.tokens.muted_foreground
                        },
                    ));
            }

            let is_current = item.is_current || is_last;
            if is_current {
                breadcrumb_element = breadcrumb_element
                    .text_color(match self.variant {
                        BreadcrumbsVariant::Default => theme.tokens.foreground,
                        BreadcrumbsVariant::Supporting => theme.tokens.muted_foreground,
                    })
                    .font_weight(FontWeight::NORMAL)
                    .child(item.label.clone());
            } else {
                let item_id_clone = item_id.clone();
                let on_click_clone = on_click.clone();

                breadcrumb_element = breadcrumb_element
                    .text_color(theme.tokens.muted_foreground)
                    .cursor(CursorStyle::PointingHand)
                    .transition(theme.tokens.transition_fast)
                    .hover(|style| style.underline())
                    .on_mouse_down(MouseButton::Left, {
                        let on_click_clone = on_click_clone.clone();
                        let item_id_clone = item_id_clone.clone();
                        move |_, window, cx| {
                            if let Some(on_click) = on_click_clone.clone() {
                                on_click(&item_id_clone, window, cx);
                            }
                        }
                    })
                    .child(item.label.clone());
            }

            elements.push(breadcrumb_element);
        }
        div()
            .flex()
            .items_center()
            .flex_wrap()
            .gap(px(4.0))
            .children(elements)
            .map(|this| {
                let mut div = this;
                div.style().refine(&user_style);
                div
            })
    }
}
