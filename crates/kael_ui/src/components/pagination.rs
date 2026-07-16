//! Pagination component for navigating through paginated content.

use kael::{prelude::FluentBuilder as _, *};
use std::panic::Location;
use std::rc::Rc;

use crate::components::button::{Button, ButtonSize, ButtonVariant};
use crate::theme::Theme;

#[derive(Clone, Copy, Debug, PartialEq, Eq, Default)]
pub enum PaginationSize {
    Sm,
    #[default]
    Md,
}

impl PaginationSize {
    fn button_size(self) -> ButtonSize {
        match self {
            Self::Sm => ButtonSize::Sm,
            Self::Md => ButtonSize::Md,
        }
    }

    fn gap(self) -> Pixels {
        match self {
            Self::Sm => px(3.0),
            Self::Md => px(4.0),
        }
    }

    fn ellipsis_size(self) -> Pixels {
        match self {
            Self::Sm => px(28.0),
            Self::Md => px(32.0),
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Default)]
pub enum PaginationVariant {
    #[default]
    Pages,
    Count,
    Compact,
    Dots,
    None,
    Default,
    Minimal,
}

pub type PaginationProps = Pagination;
pub type PaginationVariantMap = PaginationVariant;

#[derive(Clone, Copy)]
struct PaginationRuntime {
    page: usize,
    page_size: usize,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum PageRangeItem {
    Page(usize),
    Ellipsis,
}

pub fn generate_page_range(
    current_page: usize,
    total_pages: usize,
    sibling_count: usize,
) -> Vec<PageRangeItem> {
    let sibling_count = sibling_count.min(10);
    let current_page = current_page.max(1).min(total_pages.max(1));
    let total_pages = total_pages.max(1);
    let total_slots = 5usize.saturating_add(2usize.saturating_mul(sibling_count));

    if total_pages <= total_slots {
        return (1..=total_pages).map(PageRangeItem::Page).collect();
    }

    let left_sibling_index = current_page.saturating_sub(sibling_count).max(1);
    let right_sibling_index = current_page.saturating_add(sibling_count).min(total_pages);
    let show_left_ellipsis = left_sibling_index > 2;
    let show_right_ellipsis = right_sibling_index < total_pages.saturating_sub(1);

    if !show_left_ellipsis && show_right_ellipsis {
        let left_range = 3usize.saturating_add(2usize.saturating_mul(sibling_count));
        let mut pages = (1..=left_range)
            .map(PageRangeItem::Page)
            .collect::<Vec<_>>();
        pages.push(PageRangeItem::Ellipsis);
        pages.push(PageRangeItem::Page(total_pages));
        return pages;
    }

    if show_left_ellipsis && !show_right_ellipsis {
        let right_range = 3usize.saturating_add(2usize.saturating_mul(sibling_count));
        let mut pages = vec![PageRangeItem::Page(1), PageRangeItem::Ellipsis];
        for page in total_pages.saturating_sub(right_range)..=total_pages {
            if page >= 1 {
                pages.push(PageRangeItem::Page(page));
            }
        }
        return pages;
    }

    let mut pages = vec![PageRangeItem::Page(1), PageRangeItem::Ellipsis];
    for page in left_sibling_index..=right_sibling_index {
        pages.push(PageRangeItem::Page(page));
    }
    pages.push(PageRangeItem::Ellipsis);
    pages.push(PageRangeItem::Page(total_pages));
    pages
}

fn generate_dot_range(current_page: usize, total_pages: usize) -> std::ops::RangeInclusive<usize> {
    const MAX_DOTS: usize = 9;
    let total_pages = total_pages.max(1);
    let current_page = current_page.clamp(1, total_pages);
    if total_pages <= MAX_DOTS {
        return 1..=total_pages;
    }

    let half = MAX_DOTS / 2;
    let start = current_page
        .saturating_sub(half)
        .clamp(1, total_pages - MAX_DOTS + 1);
    start..=start + MAX_DOTS - 1
}

#[allow(non_snake_case)]
pub fn generatePageRange(
    current_page: usize,
    total_pages: usize,
    sibling_count: usize,
) -> Vec<PageRangeItem> {
    generate_page_range(current_page, total_pages, sibling_count)
}

#[derive(IntoElement)]
pub struct Pagination {
    id: ElementId,
    current_page: usize,
    total_pages: usize,
    total_items: Option<usize>,
    has_more: Option<bool>,
    page_size: usize,
    page_size_options: Vec<usize>,
    siblings: usize,
    show_edges: bool,
    size: PaginationSize,
    variant: PaginationVariant,
    disabled: bool,
    label: SharedString,
    on_page_change: Option<Rc<dyn Fn(usize, &mut Window, &mut App)>>,
    on_page_size_change: Option<Rc<dyn Fn(usize, &mut Window, &mut App)>>,
    style: StyleRefinement,
}

impl Pagination {
    #[track_caller]
    pub fn new() -> Self {
        let caller = Location::caller();
        Self {
            id: ElementId::Name(
                format!(
                    "pagination:{}:{}:{}",
                    caller.file(),
                    caller.line(),
                    caller.column()
                )
                .into(),
            ),
            current_page: 1,
            total_pages: 1,
            total_items: None,
            has_more: None,
            page_size: 10,
            page_size_options: Vec::new(),
            siblings: 1,
            show_edges: true,
            size: PaginationSize::default(),
            variant: PaginationVariant::default(),
            disabled: false,
            label: "Pagination".into(),
            on_page_change: None,
            on_page_size_change: None,
            style: StyleRefinement::default(),
        }
    }

    pub fn id(mut self, id: impl Into<ElementId>) -> Self {
        self.id = id.into();
        self
    }

    pub fn page(self, page: usize) -> Self {
        self.current_page(page)
    }

    pub fn current_page(mut self, page: usize) -> Self {
        self.current_page = page.max(1);
        self
    }

    pub fn total_pages(mut self, total: usize) -> Self {
        self.total_pages = total.max(1);
        self
    }

    pub fn total_items(mut self, total: usize) -> Self {
        self.total_items = Some(total);
        self.total_pages = total.div_ceil(self.page_size).max(1);
        self
    }

    #[allow(non_snake_case)]
    pub fn totalItems(self, total: usize) -> Self {
        self.total_items(total)
    }

    pub fn has_more(mut self, has_more: bool) -> Self {
        self.has_more = Some(has_more);
        self
    }

    #[allow(non_snake_case)]
    pub fn hasMore(self, has_more: bool) -> Self {
        self.has_more(has_more)
    }

    pub fn page_size(mut self, page_size: usize) -> Self {
        self.page_size = page_size.max(1);
        if let Some(total_items) = self.total_items {
            self.total_pages = total_items.div_ceil(self.page_size).max(1);
        }
        self
    }

    #[allow(non_snake_case)]
    pub fn pageSize(self, page_size: usize) -> Self {
        self.page_size(page_size)
    }

    pub fn page_size_options(mut self, options: Vec<usize>) -> Self {
        self.page_size_options.clear();
        for option in options.into_iter().filter(|option| *option > 0) {
            if !self.page_size_options.contains(&option) {
                self.page_size_options.push(option);
            }
        }
        self
    }

    #[allow(non_snake_case)]
    pub fn pageSizeOptions(self, options: Vec<usize>) -> Self {
        self.page_size_options(options)
    }

    pub fn siblings(mut self, siblings: usize) -> Self {
        self.siblings = siblings.min(10);
        self
    }

    pub fn sibling_count(self, siblings: usize) -> Self {
        self.siblings(siblings)
    }

    #[allow(non_snake_case)]
    pub fn siblingCount(self, siblings: usize) -> Self {
        self.sibling_count(siblings)
    }

    pub fn show_edges(mut self, show: bool) -> Self {
        self.show_edges = show;
        self
    }

    pub fn size(mut self, size: PaginationSize) -> Self {
        self.size = size;
        self
    }

    pub fn variant(mut self, variant: PaginationVariant) -> Self {
        self.variant = variant;
        self
    }

    pub fn disabled(mut self, disabled: bool) -> Self {
        self.disabled = disabled;
        self
    }

    pub fn is_disabled(self, disabled: bool) -> Self {
        self.disabled(disabled)
    }

    #[allow(non_snake_case)]
    pub fn isDisabled(self, disabled: bool) -> Self {
        self.disabled(disabled)
    }

    pub fn label(mut self, label: impl Into<SharedString>) -> Self {
        let label = label.into();
        self.label = if label.trim().is_empty() {
            "Pagination".into()
        } else {
            label
        };
        self
    }

    pub fn on_page_change<F>(mut self, handler: F) -> Self
    where
        F: Fn(usize, &mut Window, &mut App) + 'static,
    {
        self.on_page_change = Some(Rc::new(handler));
        self
    }

    pub fn on_change<F>(self, handler: F) -> Self
    where
        F: Fn(usize, &mut Window, &mut App) + 'static,
    {
        self.on_page_change(handler)
    }

    #[allow(non_snake_case)]
    pub fn onChange<F>(self, handler: F) -> Self
    where
        F: Fn(usize, &mut Window, &mut App) + 'static,
    {
        self.on_change(handler)
    }

    pub fn on_page_size_change<F>(mut self, handler: F) -> Self
    where
        F: Fn(usize, &mut Window, &mut App) + 'static,
    {
        self.on_page_size_change = Some(Rc::new(handler));
        self
    }

    #[allow(non_snake_case)]
    pub fn onPageSizeChange<F>(self, handler: F) -> Self
    where
        F: Fn(usize, &mut Window, &mut App) + 'static,
    {
        self.on_page_size_change(handler)
    }
}

impl Default for Pagination {
    #[track_caller]
    fn default() -> Self {
        Self::new()
    }
}

impl Styled for Pagination {
    fn style(&mut self) -> &mut StyleRefinement {
        &mut self.style
    }
}

impl RenderOnce for Pagination {
    fn render(self, window: &mut Window, cx: &mut App) -> impl IntoElement {
        let pagination_id = self.id.clone();
        let pagination_id_for_sizes = pagination_id.clone();
        let pagination_id_for_previous = pagination_id.clone();
        let pagination_id_for_number_pages = pagination_id.clone();
        let pagination_id_for_dots = pagination_id.clone();
        let pagination_id_for_next = pagination_id.clone();
        let configured_total_pages = self
            .total_items
            .map(|total| total.div_ceil(self.page_size).max(1))
            .unwrap_or(self.total_pages);
        let initial_runtime = PaginationRuntime {
            page: self.current_page.max(1).min(configured_total_pages.max(1)),
            page_size: self.page_size,
        };
        let runtime = window.use_keyed_state(
            ElementId::NamedChild(Box::new(pagination_id.clone()), "runtime".into()),
            cx,
            move |_, _| initial_runtime,
        );
        let controlled_page = self.on_page_change.is_some();
        let controlled_page_size = self.on_page_size_change.is_some();
        runtime.update(cx, |runtime, _| {
            if controlled_page {
                runtime.page = initial_runtime.page;
            }
            if controlled_page_size {
                runtime.page_size = initial_runtime.page_size;
            }
            runtime.page_size = runtime.page_size.max(1);
            let total_pages = self
                .total_items
                .map(|total| total.div_ceil(runtime.page_size).max(1))
                .unwrap_or(self.total_pages);
            runtime.page = runtime.page.max(1).min(total_pages.max(1));
        });
        let runtime_value = *runtime.read(cx);
        let page_size = runtime_value.page_size;
        let total_pages = self
            .total_items
            .map(|total| total.div_ceil(page_size).max(1))
            .unwrap_or(self.total_pages);
        let current_page = runtime_value.page.max(1).min(total_pages.max(1));
        let user_on_change = self.on_page_change.clone();
        let runtime_for_page = runtime.clone();
        let on_change: Option<Rc<dyn Fn(usize, &mut Window, &mut App)>> =
            Some(Rc::new(move |page, window, cx| {
                if !controlled_page {
                    runtime_for_page.update(cx, |runtime, cx| {
                        runtime.page = page.max(1).min(total_pages.max(1));
                        cx.notify();
                    });
                }
                if let Some(handler) = user_on_change.as_ref() {
                    handler(page.max(1).min(total_pages.max(1)), window, cx);
                }
            }));
        let user_on_page_size_change = self.on_page_size_change.clone();
        let runtime_for_size = runtime.clone();
        let on_page_size_change: Option<Rc<dyn Fn(usize, &mut Window, &mut App)>> =
            Some(Rc::new(move |new_page_size, window, cx| {
                let new_page_size = new_page_size.max(1);
                if !controlled_page_size {
                    runtime_for_size.update(cx, |runtime, cx| {
                        runtime.page_size = new_page_size;
                        runtime.page = 1;
                        cx.notify();
                    });
                }
                if let Some(handler) = user_on_page_size_change.as_ref() {
                    handler(new_page_size, window, cx);
                }
            }));
        let theme = Theme::of(cx).clone();
        let page_range = if self.show_edges {
            generate_page_range(current_page, total_pages, self.siblings)
        } else {
            let start = current_page.saturating_sub(self.siblings).max(1);
            let end = current_page.saturating_add(self.siblings).min(total_pages);
            (start..=end).map(PageRangeItem::Page).collect()
        };
        let user_style = self.style;
        let size = self.size;
        let inactive_variant = ButtonVariant::Ghost;
        let variant = match self.variant {
            PaginationVariant::Default => PaginationVariant::Pages,
            PaginationVariant::Minimal => PaginationVariant::None,
            other => other,
        };

        let has_prev = current_page > 1;
        let has_next = self.has_more.unwrap_or(current_page < total_pages);
        let range_start = self
            .total_items
            .filter(|total| *total == 0)
            .map(|_| 0)
            .unwrap_or_else(|| {
                current_page
                    .saturating_sub(1)
                    .saturating_mul(page_size)
                    .saturating_add(1)
            });
        let range_end = self
            .total_items
            .map(|total| current_page.saturating_mul(page_size).min(total))
            .unwrap_or_else(|| current_page.saturating_mul(page_size));

        div()
            .id(pagination_id.clone())
            .accessibility(
                AccessibilityAttributes::new(AccessibilityRole::Group)
                    .label(self.label.to_string()),
            )
            .flex()
            .flex_wrap()
            .items_center()
            .justify_between()
            .gap(px(16.0))
            .when(!self.page_size_options.is_empty(), |this| {
                let size_handler = on_page_size_change.clone();
                let page_handler = on_change.clone();
                this.child(
                    div()
                        .flex()
                        .items_center()
                        .gap(px(8.0))
                        .text_size(px(13.0))
                        .text_color(theme.tokens.muted_foreground)
                        .child(format!("{} / page", page_size))
                        .children(self.page_size_options.into_iter().map(move |option| {
                            let size_handler = size_handler.clone();
                            let page_handler = page_handler.clone();
                            Button::new(
                                ElementId::NamedChild(
                                    Box::new(pagination_id_for_sizes.clone()),
                                    format!("size-{option}").into(),
                                ),
                                option.to_string(),
                            )
                            .variant(if option == page_size {
                                ButtonVariant::Default
                            } else {
                                ButtonVariant::Ghost
                            })
                            .size(ButtonSize::Sm)
                            .disabled(self.disabled || option == page_size)
                            .when(
                                !self.disabled
                                    && (size_handler.is_some() || page_handler.is_some()),
                                |button| {
                                    let size_handler = size_handler.clone();
                                    let page_handler = page_handler.clone();
                                    button.on_click(move |_, window, cx| {
                                        if let Some(handler) = size_handler.as_ref() {
                                            handler(option, window, cx);
                                        }
                                        if let Some(handler) = page_handler.as_ref() {
                                            handler(1, window, cx);
                                        }
                                    })
                                },
                            )
                        })),
                )
            })
            .child(
                div()
                    .flex()
                    .items_center()
                    .gap(size.gap())
                    .child({
                        let handler = on_change.clone();
                        Button::new(
                            ElementId::NamedChild(
                                Box::new(pagination_id_for_previous.clone()),
                                "previous".into(),
                            ),
                            "Go to previous page",
                        )
                        .variant(inactive_variant)
                        .size(ButtonSize::Icon)
                        .icon("chevron-left")
                        .tooltip("Go to previous page")
                        .disabled(self.disabled || !has_prev)
                        .when_some(
                            handler.filter(|_| !self.disabled && has_prev),
                            |btn, handler| {
                                btn.on_click(move |_, window, cx| {
                                    handler(current_page - 1, window, cx);
                                })
                            },
                        )
                    })
                    .children(match variant {
                        PaginationVariant::Pages => {
                            let on_change_for_pages = on_change.clone();
                            page_range
                                .into_iter()
                                .map(move |item| match item {
                                    PageRangeItem::Page(page) => {
                                        let is_current = page == current_page;
                                        let handler = on_change_for_pages.clone();

                                        div()
                                            .child(
                                                Button::new(
                                                    ElementId::NamedChild(
                                                        Box::new(
                                                            pagination_id_for_number_pages.clone(),
                                                        ),
                                                        format!("page-{page}").into(),
                                                    ),
                                                    page.to_string(),
                                                )
                                                .variant(inactive_variant)
                                                .size(size.button_size())
                                                .disabled(self.disabled)
                                                .pressed(is_current)
                                                .when(is_current, |btn| {
                                                    btn.bg(theme.tokens.secondary)
                                                        .text_color(theme.tokens.foreground)
                                                })
                                                .when_some(
                                                    handler
                                                        .filter(|_| !self.disabled && !is_current),
                                                    |btn, handler| {
                                                        btn.on_click(move |_, window, cx| {
                                                            handler(page, window, cx);
                                                        })
                                                    },
                                                ),
                                            )
                                            .into_any_element()
                                    }
                                    PageRangeItem::Ellipsis => div()
                                        .flex()
                                        .items_center()
                                        .justify_center()
                                        .h(size.ellipsis_size())
                                        .w(size.ellipsis_size())
                                        .text_color(theme.tokens.muted_foreground)
                                        .child("...")
                                        .into_any_element(),
                                })
                                .collect::<Vec<_>>()
                        }
                        PaginationVariant::Count => self
                            .total_items
                            .map(|total| {
                                vec![div()
                                    .px(px(8.0))
                                    .text_size(px(13.0))
                                    .text_color(theme.tokens.muted_foreground)
                                    .child(format!("{range_start}-{range_end} of {total}"))
                                    .into_any_element()]
                            })
                            .unwrap_or_default(),
                        PaginationVariant::Compact => vec![div()
                            .px(px(8.0))
                            .text_size(px(13.0))
                            .text_color(theme.tokens.muted_foreground)
                            .child(format!("Page {current_page} of {total_pages}"))
                            .into_any_element()],
                        PaginationVariant::Dots => generate_dot_range(current_page, total_pages)
                            .map(|page| {
                                let is_current = page == current_page;
                                let handler = on_change.clone();
                                button(ElementId::NamedChild(
                                    Box::new(pagination_id_for_dots.clone()),
                                    format!("dot-{page}").into(),
                                ))
                                .label(if is_current {
                                    format!("Page {page}, current page")
                                } else {
                                    format!("Go to page {page}")
                                })
                                .selected(is_current)
                                .when(self.disabled, |button| button.disabled())
                                .when_some(
                                    handler.filter(|_| !self.disabled && !is_current),
                                    |button, handler| {
                                        button.on_click(move |_, window, cx| {
                                            handler(page, window, cx);
                                        })
                                    },
                                )
                                .render_with(move |button_state, _, _| {
                                    div()
                                        .size(size.ellipsis_size())
                                        .flex()
                                        .items_center()
                                        .justify_center()
                                        .rounded_full()
                                        .when(button_state.focused, |this| {
                                            this.shadow(smallvec::smallvec![
                                                crate::astryx::focus_ring_outer(theme.tokens.ring)
                                            ])
                                        })
                                        .child(
                                            div()
                                                .size(if size == PaginationSize::Sm {
                                                    px(7.0)
                                                } else {
                                                    px(9.0)
                                                })
                                                .rounded_full()
                                                .bg(if is_current {
                                                    theme.tokens.primary
                                                } else {
                                                    theme.tokens.muted_foreground.opacity(0.35)
                                                }),
                                        )
                                        .into_any_element()
                                })
                                .into_any_element()
                            })
                            .collect::<Vec<_>>(),
                        PaginationVariant::None
                        | PaginationVariant::Default
                        | PaginationVariant::Minimal => Vec::new(),
                    })
                    .child({
                        let handler = on_change;
                        Button::new(
                            ElementId::NamedChild(
                                Box::new(pagination_id_for_next.clone()),
                                "next".into(),
                            ),
                            "Go to next page",
                        )
                        .variant(inactive_variant)
                        .size(ButtonSize::Icon)
                        .icon("chevron-right")
                        .tooltip("Go to next page")
                        .disabled(self.disabled || !has_next)
                        .when_some(
                            handler.filter(|_| !self.disabled && has_next),
                            |btn, handler| {
                                btn.on_click(move |_, window, cx| {
                                    handler(current_page.saturating_add(1), window, cx);
                                })
                            },
                        )
                    }),
            )
            .map(|this| {
                let mut div = this;
                div.style().refine(&user_style);
                div
            })
    }
}

#[cfg(test)]
mod tests {
    use super::{generate_dot_range, generate_page_range, PageRangeItem, Pagination};

    #[test]
    fn page_range_is_bounded_for_extreme_sibling_counts() {
        let range = generate_page_range(5, 10, usize::MAX);
        assert_eq!(range.first(), Some(&PageRangeItem::Page(1)));
        assert_eq!(range.last(), Some(&PageRangeItem::Page(10)));
        assert_eq!(range.len(), 10);
    }

    #[test]
    fn page_range_clamps_out_of_bounds_current_pages() {
        assert_eq!(
            generate_page_range(usize::MAX, 3, 1),
            vec![
                PageRangeItem::Page(1),
                PageRangeItem::Page(2),
                PageRangeItem::Page(3),
            ]
        );
    }

    #[test]
    fn huge_dot_ranges_stay_bounded_and_center_the_current_page() {
        assert_eq!(generate_dot_range(50, usize::MAX).count(), 9);
        assert!(generate_dot_range(50, 100).contains(&50));
        assert_eq!(generate_dot_range(1, 100), 1..=9);
    }

    #[test]
    fn public_sibling_counts_and_labels_are_normalized() {
        let pagination = Pagination::new().siblings(usize::MAX).label("  ");
        assert_eq!(pagination.siblings, 10);
        assert_eq!(pagination.label.as_ref(), "Pagination");
    }
}
