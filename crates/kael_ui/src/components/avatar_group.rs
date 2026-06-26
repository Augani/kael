use crate::components::avatar::{Avatar, AvatarSize};
use crate::components::tooltip::tooltip;
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

fn get_overlap(size: AvatarSize, spacing: Option<f32>) -> f32 {
    if let Some(spacing) = spacing {
        return spacing;
    }

    match size {
        AvatarSize::Xs => -8.0,
        AvatarSize::Sm => -10.0,
        AvatarSize::Md => -12.0,
        AvatarSize::Lg => -14.0,
        AvatarSize::Xl => -18.0,
    }
}

fn get_size_px(size: AvatarSize) -> f32 {
    match size {
        AvatarSize::Xs => 24.0,
        AvatarSize::Sm => 32.0,
        AvatarSize::Md => 40.0,
        AvatarSize::Lg => 48.0,
        AvatarSize::Xl => 64.0,
    }
}

fn get_text_size(size: AvatarSize) -> f32 {
    match size {
        AvatarSize::Xs => 9.0,
        AvatarSize::Sm => 11.0,
        AvatarSize::Md => 13.0,
        AvatarSize::Lg => 15.0,
        AvatarSize::Xl => 18.0,
    }
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
    size: AvatarSize,
    max_visible: Option<usize>,
    show_tooltips: bool,
    spacing: Option<f32>,
    style: StyleRefinement,
}

impl AvatarGroup {
    pub fn new(items: Vec<AvatarItem>) -> Self {
        Self {
            items,
            size: AvatarSize::default(),
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

    pub fn max_visible(mut self, max: usize) -> Self {
        self.max_visible = Some(max);
        self
    }

    pub fn show_tooltips(mut self, show: bool) -> Self {
        self.show_tooltips = show;
        self
    }

    pub fn spacing(mut self, spacing: Pixels) -> Self {
        self.spacing = Some(f32::from(spacing));
        self
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
        let user_style = self.style;

        let overlap = get_overlap(size, spacing);
        let size_px = get_size_px(size);
        let text_size = get_text_size(size);

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
                .child(format!("+{}", overflow_count));
            children.push(if show_tooltips && !overflow_names.is_empty() {
                tooltip(visual, overflow_names.join(", "))
                    .absolute()
                    .left(px(left))
                    .top_0()
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
                (true, Some(name)) => tooltip(visual, name)
                    .absolute()
                    .left(px(left))
                    .top_0()
                    .into_any_element(),
                _ => visual.absolute().left(px(left)).top_0().into_any_element(),
            };
            children.push(el);
        }

        div()
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
