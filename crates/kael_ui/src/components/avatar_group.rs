use crate::components::avatar::{Avatar, AvatarSize};
use crate::components::tooltip::{TooltipFocusTrigger, tooltip};
use crate::theme::use_theme;
use kael::{prelude::FluentBuilder as _, *};

#[derive(Debug, Clone)]
pub struct AvatarItem {
    pub name: Option<SharedString>,
    pub src: Option<SharedString>,
    pub fallback_text: Option<SharedString>,
}

impl AvatarItem {
    pub fn new() -> Self {
        Self {
            name: None,
            src: None,
            fallback_text: None,
        }
    }

    pub fn name(mut self, name: impl Into<SharedString>) -> Self {
        self.name = Some(name.into());
        self
    }

    pub fn src(mut self, src: impl Into<SharedString>) -> Self {
        self.src = Some(src.into());
        self
    }

    pub fn fallback_text(mut self, text: impl Into<SharedString>) -> Self {
        self.fallback_text = Some(text.into());
        self
    }
}

impl Default for AvatarItem {
    fn default() -> Self {
        Self::new()
    }
}

#[derive(IntoElement)]
pub struct AvatarGroupOverflow {
    count: usize,
    size: AvatarSize,
    style: StyleRefinement,
}

impl AvatarGroupOverflow {
    pub fn new(count: usize) -> Self {
        Self {
            count,
            size: AvatarSize::default(),
            style: StyleRefinement::default(),
        }
    }

    pub fn size(mut self, size: AvatarSize) -> Self {
        self.size = size;
        self
    }
}

impl Styled for AvatarGroupOverflow {
    fn style(&mut self) -> &mut StyleRefinement {
        &mut self.style
    }
}

impl RenderOnce for AvatarGroupOverflow {
    fn render(self, _window: &mut Window, _cx: &mut App) -> impl IntoElement {
        let theme = use_theme();
        let size_px = get_size_px(self.size);
        let text_size = get_text_size(self.size);
        let user_style = self.style;

        div()
            .size(px(size_px))
            .flex()
            .items_center()
            .justify_center()
            .rounded_full()
            .bg(theme.tokens.muted)
            .text_color(theme.tokens.muted_foreground)
            .text_size(px(text_size))
            .font_weight(FontWeight::MEDIUM)
            .font_family(theme.tokens.font_family.clone())
            .border_2()
            .border_color(theme.tokens.card)
            .child(format!("+{}", self.count))
            .map(|this| {
                let mut div = this;
                div.style().refine(&user_style);
                div
            })
    }
}

fn get_overlap(size: AvatarSize, spacing: Option<f32>) -> f32 {
    if let Some(spacing) = spacing.filter(|spacing| spacing.is_finite()) {
        let size = get_size_px(size);
        return spacing.clamp(-size + 1.0, size * 4.0);
    }

    match size {
        AvatarSize::Xs => -(get_size_px(size) * 0.25).round(),
        AvatarSize::Sm => -(get_size_px(size) * 0.25).round(),
        AvatarSize::Md => -(get_size_px(size) * 0.25).round(),
        AvatarSize::Lg => -(get_size_px(size) * 0.25).round(),
        AvatarSize::Xl => -(get_size_px(size) * 0.25).round(),
    }
}

fn get_size_px(size: AvatarSize) -> f32 {
    match size {
        AvatarSize::Xs => 20.0,
        AvatarSize::Sm => 24.0,
        AvatarSize::Md => 36.0,
        AvatarSize::Lg => 48.0,
        AvatarSize::Xl => 128.0,
    }
}

fn get_text_size(size: AvatarSize) -> f32 {
    get_size_px(size) * 0.35
}

fn create_avatar(item: &AvatarItem, size: AvatarSize) -> Avatar {
    let mut avatar = Avatar::new().size(size);

    if let Some(ref src) = item.src {
        avatar = avatar.src(src.clone());
    }
    if let Some(ref name) = item.name {
        avatar = avatar.name(name.clone());
    }
    if let Some(ref fallback) = item.fallback_text {
        avatar = avatar.fallback_text(fallback.clone());
    }

    avatar
}

#[derive(IntoElement)]
pub struct AvatarGroup {
    items: Vec<AvatarItem>,
    children: Vec<AnyElement>,
    size: AvatarSize,
    aria_label: SharedString,
    max_visible: Option<usize>,
    show_tooltips: bool,
    spacing: Option<f32>,
    style: StyleRefinement,
}

impl AvatarGroup {
    pub fn new(items: Vec<AvatarItem>) -> Self {
        Self {
            items,
            children: Vec::new(),
            size: AvatarSize::default(),
            aria_label: "Avatars".into(),
            max_visible: None,
            show_tooltips: false,
            spacing: None,
            style: StyleRefinement::default(),
        }
    }

    pub fn size(mut self, size: AvatarSize) -> Self {
        self.size = size;
        self
    }

    pub fn child(mut self, child: impl IntoElement) -> Self {
        self.children.push(child.into_any_element());
        self
    }

    pub fn children(mut self, children: impl IntoIterator<Item = impl IntoElement>) -> Self {
        self.children
            .extend(children.into_iter().map(|child| child.into_any_element()));
        self
    }

    pub fn aria_label(mut self, label: impl Into<SharedString>) -> Self {
        let label = label.into();
        self.aria_label = if label.trim().is_empty() {
            "Avatars".into()
        } else {
            label
        };
        self
    }

    #[allow(non_snake_case)]
    pub fn ariaLabel(self, label: impl Into<SharedString>) -> Self {
        self.aria_label(label)
    }

    pub fn max_visible(mut self, max: usize) -> Self {
        self.max_visible = Some(max);
        self
    }

    pub fn show_tooltips(mut self, show: bool) -> Self {
        self.show_tooltips = show;
        self
    }

    pub fn spacing(mut self, spacing: Pixels) -> Self {
        let spacing = f32::from(spacing);
        if spacing.is_finite() {
            self.spacing = Some(spacing);
        }
        self
    }
}

#[cfg(test)]
#[allow(clippy::items_after_test_module)]
mod tests {
    use super::*;

    #[::core::prelude::v1::test]
    fn overlap_is_finite_and_bounded_for_the_final_avatar_size() {
        assert_eq!(get_overlap(AvatarSize::Md, None), -9.0);
        assert_eq!(get_overlap(AvatarSize::Xs, Some(-500.0)), -19.0);
        assert_eq!(get_overlap(AvatarSize::Xs, Some(500.0)), 80.0);
        assert_eq!(get_overlap(AvatarSize::Md, Some(f32::NAN)), -9.0);
    }

    #[::core::prelude::v1::test]
    fn empty_group_labels_and_invalid_spacing_use_safe_values() {
        let group = AvatarGroup::default().aria_label(" ").spacing(px(f32::NAN));
        assert_eq!(group.aria_label.as_ref(), "Avatars");
        assert_eq!(group.spacing, None);
    }
}

impl Default for AvatarGroup {
    fn default() -> Self {
        Self::new(Vec::new())
    }
}

impl Styled for AvatarGroup {
    fn style(&mut self) -> &mut StyleRefinement {
        &mut self.style
    }
}

impl RenderOnce for AvatarGroup {
    fn render(self, _: &mut Window, _: &mut App) -> impl IntoElement {
        let theme = use_theme();

        let size = self.size;
        let show_tooltips = self.show_tooltips;
        let spacing = self.spacing;
        let max_visible = self.max_visible;
        let items = self.items;
        let composed_children = self.children;
        let aria_label = self.aria_label;
        let user_style = self.style;

        let overlap = get_overlap(size, spacing);
        let size_px = get_size_px(size);
        let text_size = get_text_size(size);

        if !composed_children.is_empty() {
            let step = size_px + overlap;
            let total_w = (composed_children.len() as f32 - 1.0).max(0.0) * step + size_px;
            return div()
                .accessibility(
                    AccessibilityAttributes::new(AccessibilityRole::Group)
                        .label(aria_label.to_string()),
                )
                .relative()
                .h(px(size_px))
                .w(px(total_w))
                .children(composed_children.into_iter().enumerate().rev().map(
                    move |(index, child)| {
                        div()
                            .absolute()
                            .left(px(index as f32 * step))
                            .top_0()
                            .rounded_full()
                            .border_2()
                            .border_color(theme.tokens.card)
                            .child(child)
                            .into_any_element()
                    },
                ))
                .map(|this| {
                    let mut div = this;
                    div.style().refine(&user_style);
                    div
                });
        }

        let total_count = items.len();
        let max_vis = max_visible.unwrap_or(total_count);
        let visible_count = max_vis.min(total_count);
        let overflow_count = total_count.saturating_sub(visible_count);

        let visible_items: Vec<_> = items.iter().take(visible_count).cloned().collect();
        let overflow_names: Vec<String> = items
            .iter()
            .skip(visible_count)
            .filter_map(|item| item.name.as_ref().map(|n| n.to_string()))
            .collect();

        let card = theme.tokens.card;
        let step = size_px + overlap;
        let slots = visible_count + if overflow_count > 0 { 1 } else { 0 };
        let total_w = if slots == 0 {
            0.0
        } else {
            (slots as f32 - 1.0).max(0.0) * step + size_px
        };

        // Painted rightmost-first so the leftmost avatar ends up on top.
        let mut children: Vec<AnyElement> = Vec::new();

        if overflow_count > 0 {
            let left = visible_count as f32 * step;
            let visual = div()
                .accessibility(
                    AccessibilityAttributes::new(AccessibilityRole::StaticText)
                        .label(format!("{overflow_count} more avatars")),
                )
                .size(px(size_px))
                .flex()
                .items_center()
                .justify_center()
                .rounded_full()
                .bg(theme.tokens.muted)
                .text_color(theme.tokens.muted_foreground)
                .text_size(px(text_size))
                .font_weight(FontWeight::MEDIUM)
                .font_family(theme.tokens.font_family.clone())
                .border_2()
                .border_color(card)
                .child(StyledText::new(format!("+{}", overflow_count)).accessibility_hidden(true));
            children.push(if show_tooltips && !overflow_names.is_empty() {
                div()
                    .absolute()
                    .left(px(left))
                    .top_0()
                    .child(
                        tooltip(visual, overflow_names.join(", "))
                            .focus_trigger(TooltipFocusTrigger::Never),
                    )
                    .into_any_element()
            } else {
                visual.absolute().left(px(left)).top_0().into_any_element()
            });
        }

        for (index, item) in visible_items.iter().enumerate().rev() {
            let left = index as f32 * step;
            let visual = div()
                .rounded_full()
                .border_2()
                .border_color(card)
                .child(create_avatar(item, size));
            let el = match (show_tooltips, item.name.clone()) {
                (true, Some(name)) => div()
                    .absolute()
                    .left(px(left))
                    .top_0()
                    .child(tooltip(visual, name).focus_trigger(TooltipFocusTrigger::Never))
                    .into_any_element(),
                _ => visual.absolute().left(px(left)).top_0().into_any_element(),
            };
            children.push(el);
        }

        div()
            .accessibility(
                AccessibilityAttributes::new(AccessibilityRole::Group)
                    .label(aria_label.to_string()),
            )
            .relative()
            .h(px(size_px))
            .w(px(total_w))
            .map(|this| {
                let mut div = this;
                div.style().refine(&user_style);
                div
            })
            .children(children)
    }
}
