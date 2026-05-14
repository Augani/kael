use crate::{AtlasTile, Bounds, DevicePixels, RenderIconAtlasParams, TileId, hash, point, size};

pub(crate) struct GeneratedIconMetadata {
    pub(crate) name: &'static str,
    pub(crate) x: u32,
    pub(crate) y: u32,
    pub(crate) width: u32,
    pub(crate) height: u32,
}

pub(crate) struct GeneratedIconAtlas {
    pub(crate) edge: u32,
    pub(crate) width: u32,
    pub(crate) height: u32,
    pub(crate) bytes: &'static [u8],
    pub(crate) icons: &'static [GeneratedIconMetadata],
}

pub(crate) struct ResolvedIconAtlas {
    pub(crate) atlas_params: RenderIconAtlasParams,
    pub(crate) atlas_size: crate::Size<DevicePixels>,
    pub(crate) bytes: &'static [u8],
    pub(crate) rect: Bounds<DevicePixels>,
}

include!(concat!(env!("OUT_DIR"), "/generated_icon_atlases.rs"));

pub(crate) fn resolve_generated_icon(
    name: &str,
    requested_size: crate::Size<DevicePixels>,
) -> Option<ResolvedIconAtlas> {
    let atlas = select_icon_atlas(requested_size)?;
    let icon = atlas.icons.iter().find(|icon| icon.name == name)?;

    Some(ResolvedIconAtlas {
        atlas_params: RenderIconAtlasParams {
            edge: DevicePixels(atlas.edge as i32),
        },
        atlas_size: size(
            DevicePixels(atlas.width as i32),
            DevicePixels(atlas.height as i32),
        ),
        bytes: atlas.bytes,
        rect: Bounds {
            origin: point(DevicePixels(icon.x as i32), DevicePixels(icon.y as i32)),
            size: size(
                DevicePixels(icon.width as i32),
                DevicePixels(icon.height as i32),
            ),
        },
    })
}

pub(crate) fn icon_asset_path(name: &str) -> crate::SharedString {
    if name.ends_with(".svg") {
        name.to_string().into()
    } else {
        format!("icons/{name}.svg").into()
    }
}

pub(crate) fn icon_subtile(base_tile: AtlasTile, rect: Bounds<DevicePixels>) -> AtlasTile {
    AtlasTile {
        texture_id: base_tile.texture_id,
        tile_id: TileId(
            (hash(&(
                base_tile.texture_id.index,
                base_tile.tile_id.0,
                rect.origin.x.0,
                rect.origin.y.0,
                rect.size.width.0,
                rect.size.height.0,
            )) & u64::from(u32::MAX)) as u32,
        ),
        padding: 0,
        bounds: Bounds {
            origin: point(
                base_tile.bounds.origin.x + rect.origin.x,
                base_tile.bounds.origin.y + rect.origin.y,
            ),
            size: rect.size,
        },
    }
}

fn select_icon_atlas(
    requested_size: crate::Size<DevicePixels>,
) -> Option<&'static GeneratedIconAtlas> {
    let requested_edge = requested_size.width.0.max(requested_size.height.0).max(1) as u32;

    GENERATED_ICON_ATLASES
        .iter()
        .find(|atlas| atlas.edge >= requested_edge)
        .or_else(|| GENERATED_ICON_ATLASES.last())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn icon_atlas_rounds_up_to_the_next_bucket() {
        let icon =
            resolve_generated_icon("chevron_right", size(DevicePixels(17), DevicePixels(12)))
                .unwrap();

        assert_eq!(icon.atlas_params.edge, DevicePixels(20));
    }

    #[test]
    fn generated_icons_include_default_assets() {
        let icon =
            resolve_generated_icon("close", size(DevicePixels(16), DevicePixels(16))).unwrap();

        assert_eq!(icon.rect.size, size(DevicePixels(16), DevicePixels(16)));
        assert_eq!(icon.atlas_size.height, DevicePixels(16));
    }

    #[test]
    fn icon_asset_path_uses_icons_directory_by_default() {
        assert_eq!(
            icon_asset_path("brand/logo").as_ref(),
            "icons/brand/logo.svg"
        );
    }

    #[test]
    fn icon_asset_path_preserves_explicit_svg_paths() {
        assert_eq!(
            icon_asset_path("icons/brand/logo.svg").as_ref(),
            "icons/brand/logo.svg"
        );
    }
}
