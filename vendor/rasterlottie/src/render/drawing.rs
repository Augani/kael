use tiny_skia::Pixmap;

use super::{
    drawing_paths::{
        build_shape_source_path, build_supported_merge_path, draw_path, repeater_copy_order,
        repeater_transform_for_copy,
    },
    renderer::{
        RenderTransform, ShapeGroupPlan, ShapeRenderState, ShapeRenderableItem,
        ShapeRenderableKind, ShapeStyles,
    },
    values::{
        decode_fill_style, decode_repeater_style, decode_shape_transform, decode_stroke_style,
        decode_trim_style,
    },
};
use crate::ShapeItem;

pub(super) fn render_shape_items(
    items: &[ShapeItem],
    frame: f32,
    pixmap: &mut Pixmap,
    inherited_transform: RenderTransform,
    inherited: ShapeRenderState<'_>,
) -> Result<(), crate::RasterlottieError> {
    span_enter!(
        tracing::Level::TRACE,
        "render_shape_items",
        frame = frame,
        item_count = items.len()
    );
    let plan = inherited
        .shape_plan_cache
        .and_then(|cache| cache.for_items(items));
    let local_transform = plan
        .and_then(|plan| plan.static_local_transform)
        .unwrap_or_else(|| {
            plan.and_then(|plan| plan.local_transform)
                .or_else(|| find_visible_item(items, "tr"))
                .map(|index| {
                    decode_shape_transform(&items[index], frame, inherited.timeline_sample_cache)
                })
                .unwrap_or_default()
        });
    let transform = inherited_transform.concat(local_transform);
    let trim = plan
        .and_then(|plan| plan.static_trim)
        .or_else(|| {
            plan.and_then(|plan| plan.trim)
                .or_else(|| find_visible_item(items, "tm"))
                .and_then(|index| {
                    decode_trim_style(&items[index], frame, inherited.timeline_sample_cache)
                })
        })
        .or_else(|| inherited.trim.copied());
    let repeater = plan.and_then(|plan| plan.static_repeater).or_else(|| {
        plan.and_then(|plan| plan.repeater)
            .or_else(|| find_visible_item(items, "rp"))
            .and_then(|index| {
                decode_repeater_style(&items[index], frame, inherited.timeline_sample_cache)
            })
    });

    let styles = ShapeStyles {
        fill: plan
            .and_then(|plan| plan.static_fill.clone())
            .or_else(|| {
                plan.and_then(|plan| plan.fill)
                    .or_else(|| find_first_visible_fill(items))
                    .and_then(|index| {
                        decode_fill_style(&items[index], frame, inherited.timeline_sample_cache)
                    })
            })
            .or_else(|| inherited.styles.fill.clone()),
        stroke: plan
            .and_then(|plan| plan.static_stroke.clone())
            .or_else(|| {
                plan.and_then(|plan| plan.stroke)
                    .or_else(|| find_first_visible_stroke(items))
                    .and_then(|index| {
                        decode_stroke_style(&items[index], frame, inherited.timeline_sample_cache)
                    })
            })
            .or_else(|| inherited.styles.stroke.clone()),
    };

    if let Some(path) = build_supported_merge_path(
        items,
        frame,
        inherited.static_path_cache,
        inherited.timeline_sample_cache,
        plan.and_then(|plan| plan.merge_index),
    ) {
        if let Some(repeater) = repeater {
            for copy_index in repeater_copy_order(&repeater) {
                let copy_transform =
                    transform.concat(repeater_transform_for_copy(&repeater, copy_index));
                draw_path(&path, &styles, trim.as_ref(), copy_transform, pixmap);
            }
        } else {
            draw_path(&path, &styles, trim.as_ref(), transform, pixmap);
        }

        return Ok(());
    }

    if let Some(repeater) = repeater {
        for copy_index in repeater_copy_order(&repeater) {
            let copy_transform =
                transform.concat(repeater_transform_for_copy(&repeater, copy_index));
            render_shape_renderables(
                items,
                plan,
                frame,
                pixmap,
                copy_transform,
                ShapeRenderState {
                    styles: &styles,
                    trim: trim.as_ref(),
                    static_path_cache: inherited.static_path_cache,
                    shape_plan_cache: inherited.shape_plan_cache,
                    timeline_sample_cache: inherited.timeline_sample_cache,
                },
            )?;
        }
    } else {
        render_shape_renderables(
            items,
            plan,
            frame,
            pixmap,
            transform,
            ShapeRenderState {
                styles: &styles,
                trim: trim.as_ref(),
                static_path_cache: inherited.static_path_cache,
                shape_plan_cache: inherited.shape_plan_cache,
                timeline_sample_cache: inherited.timeline_sample_cache,
            },
        )?;
    }

    Ok(())
}

fn find_visible_item(items: &[ShapeItem], item_type: &str) -> Option<usize> {
    items
        .iter()
        .position(|item| !item.hidden && item.item_type == item_type)
}

fn find_first_visible_fill(items: &[ShapeItem]) -> Option<usize> {
    items
        .iter()
        .position(|item| !item.hidden && matches!(item.item_type.as_str(), "fl" | "gf"))
}

fn find_first_visible_stroke(items: &[ShapeItem]) -> Option<usize> {
    items
        .iter()
        .position(|item| !item.hidden && matches!(item.item_type.as_str(), "st" | "gs"))
}

fn render_shape_renderables(
    items: &[ShapeItem],
    plan: Option<&ShapeGroupPlan>,
    frame: f32,
    pixmap: &mut Pixmap,
    transform: RenderTransform,
    state: ShapeRenderState<'_>,
) -> Result<(), crate::RasterlottieError> {
    if let Some(plan) = plan {
        for renderable in plan.renderables.iter().rev() {
            render_shape_item_planned(renderable, items, frame, pixmap, transform, state)?;
        }
    } else {
        for item in renderable_shape_items(items).rev() {
            render_shape_item(item, frame, pixmap, transform, state)?;
        }
    }

    Ok(())
}

fn render_shape_item_planned(
    renderable: &ShapeRenderableItem,
    items: &[ShapeItem],
    frame: f32,
    pixmap: &mut Pixmap,
    transform: RenderTransform,
    state: ShapeRenderState<'_>,
) -> Result<(), crate::RasterlottieError> {
    let item = &items[renderable.index];
    span_enter!(
        tracing::Level::TRACE,
        "render_shape_item",
        frame = frame,
        item_type = item.item_type.as_str()
    );
    match renderable.kind {
        ShapeRenderableKind::Group => {
            render_shape_items(&item.items, frame, pixmap, transform, state)?;
        }
        ShapeRenderableKind::Geometry => {
            if let Some(path) = build_shape_source_path(
                item,
                frame,
                state.static_path_cache,
                state.timeline_sample_cache,
            ) {
                draw_path(&path, state.styles, state.trim, transform, pixmap);
            }
        }
    }

    Ok(())
}

fn renderable_shape_items(
    items: &[ShapeItem],
) -> impl DoubleEndedIterator<Item = &ShapeItem> + Clone {
    items.iter().filter(|item| {
        !item.hidden && matches!(item.item_type.as_str(), "gr" | "rc" | "el" | "sh" | "sr")
    })
}

fn render_shape_item(
    item: &ShapeItem,
    frame: f32,
    pixmap: &mut Pixmap,
    transform: RenderTransform,
    state: ShapeRenderState<'_>,
) -> Result<(), crate::RasterlottieError> {
    span_enter!(
        tracing::Level::TRACE,
        "render_shape_item",
        frame = frame,
        item_type = item.item_type.as_str()
    );
    match item.item_type.as_str() {
        "gr" => render_shape_items(&item.items, frame, pixmap, transform, state)?,
        "rc" | "el" | "sh" | "sr" => {
            if let Some(path) = build_shape_source_path(
                item,
                frame,
                state.static_path_cache,
                state.timeline_sample_cache,
            ) {
                draw_path(&path, state.styles, state.trim, transform, pixmap);
            }
        }
        _ => {}
    }

    Ok(())
}
