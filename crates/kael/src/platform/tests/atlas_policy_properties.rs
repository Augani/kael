use crate::{
    AtlasAllocationClass, AtlasKey, DevicePixels, ImageId, RenderImageParams,
    SMALL_IMAGE_ATLAS_PAGE_SIZE, Size,
};

fn image_key() -> AtlasKey {
    AtlasKey::Image(RenderImageParams {
        image_id: ImageId(42),
        frame_index: 0,
    })
}

#[test]
fn small_images_use_shared_thumbnail_pages() {
    let key = image_key();
    let size = Size {
        width: DevicePixels(64),
        height: DevicePixels(64),
    };

    assert_eq!(
        key.allocation_class(size),
        AtlasAllocationClass::SharedSmallImage
    );
    assert_eq!(
        AtlasAllocationClass::SharedSmallImage.texture_size(
            size,
            Size {
                width: DevicePixels(1024),
                height: DevicePixels(1024),
            },
            Size {
                width: DevicePixels(16384),
                height: DevicePixels(16384),
            },
        ),
        SMALL_IMAGE_ATLAS_PAGE_SIZE,
    );
}

#[test]
fn large_images_use_dedicated_textures() {
    let key = image_key();
    let size = Size {
        width: DevicePixels(512),
        height: DevicePixels(320),
    };

    assert_eq!(
        key.allocation_class(size),
        AtlasAllocationClass::DedicatedLargeImage
    );
    assert_eq!(
        AtlasAllocationClass::DedicatedLargeImage.texture_size(
            size,
            Size {
                width: DevicePixels(1024),
                height: DevicePixels(1024),
            },
            Size {
                width: DevicePixels(16384),
                height: DevicePixels(16384),
            },
        ),
        size,
    );
}
