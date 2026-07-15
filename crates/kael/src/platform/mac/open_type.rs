#![allow(unused, non_upper_case_globals)]

use crate::{FontFallbacks, FontFeatures};
use core_foundation::{
    array::{
        CFArray, CFArrayAppendArray, CFArrayAppendValue, CFArrayCreateMutable, CFArrayGetCount,
        CFArrayGetValueAtIndex, CFArrayRef, CFMutableArrayRef, kCFTypeArrayCallBacks,
    },
    base::{CFRelease, TCFType, kCFAllocatorDefault},
    dictionary::{
        CFDictionaryCreate, kCFTypeDictionaryKeyCallBacks, kCFTypeDictionaryValueCallBacks,
    },
    number::CFNumber,
    string::{CFString, CFStringRef},
};
use core_foundation_sys::locale::CFLocaleCopyPreferredLanguages;
use core_graphics::base::CGFloat;
use core_graphics::{display::CFDictionary, geometry::CGAffineTransform};
use core_text::{
    font::{CTFont, CTFontRef, cascade_list_for_languages},
    font_descriptor::{
        CTFontDescriptor, CTFontDescriptorCopyAttributes, CTFontDescriptorCreateCopyWithFeature,
        CTFontDescriptorCreateWithAttributes, CTFontDescriptorCreateWithNameAndSize,
        CTFontDescriptorRef, kCTFontCascadeListAttribute, kCTFontFeatureSettingsAttribute,
    },
};
use font_kit::font::Font as FontKitFont;
use std::ptr;

const MAX_FONT_FEATURES: usize = 256;
const MAX_FONT_FALLBACKS: usize = 256;
const MAX_FONT_FAMILY_BYTES: usize = 1_024;

pub fn apply_features_and_fallbacks(
    font: &mut FontKitFont,
    features: &FontFeatures,
    fallbacks: Option<&FontFallbacks>,
) -> anyhow::Result<()> {
    unsafe {
        anyhow::ensure!(
            features.tag_value_list().len() <= MAX_FONT_FEATURES,
            "too many OpenType features"
        );
        for (tag, value) in features.tag_value_list() {
            anyhow::ensure!(
                tag.len() == 4 && tag.bytes().all(|byte| byte.is_ascii_alphanumeric()),
                "invalid OpenType feature tag"
            );
            anyhow::ensure!(
                *value <= i32::MAX as u32,
                "OpenType feature value is too large"
            );
        }
        if let Some(fallbacks) = fallbacks {
            anyhow::ensure!(
                fallbacks.fallback_list().len() <= MAX_FONT_FALLBACKS,
                "too many font fallbacks"
            );
            anyhow::ensure!(
                fallbacks.fallback_list().iter().all(|family| {
                    !family.is_empty()
                        && family.len() <= MAX_FONT_FAMILY_BYTES
                        && !family.chars().any(char::is_control)
                }),
                "invalid font fallback family"
            );
        }

        let mut keys = vec![kCTFontFeatureSettingsAttribute];
        let mut values = vec![generate_feature_array(features)?];
        if let Some(fallbacks) = fallbacks
            && !fallbacks.fallback_list().is_empty()
        {
            keys.push(kCTFontCascadeListAttribute);
            match generate_fallback_array(fallbacks, font.native_font().as_concrete_TypeRef()) {
                Ok(value) => values.push(value),
                Err(error) => {
                    values.into_iter().for_each(|value| CFRelease(value as _));
                    return Err(error);
                }
            }
        }
        let attrs = CFDictionaryCreate(
            kCFAllocatorDefault,
            keys.as_ptr() as _,
            values.as_ptr() as _,
            keys.len() as isize,
            &kCFTypeDictionaryKeyCallBacks,
            &kCFTypeDictionaryValueCallBacks,
        );
        values.into_iter().for_each(|value| CFRelease(value as _));
        anyhow::ensure!(
            !attrs.is_null(),
            "failed to create font attribute dictionary"
        );
        let new_descriptor = CTFontDescriptorCreateWithAttributes(attrs);
        CFRelease(attrs as _);
        anyhow::ensure!(
            !new_descriptor.is_null(),
            "failed to create font descriptor"
        );
        let new_descriptor = CTFontDescriptor::wrap_under_create_rule(new_descriptor);
        let new_font = CTFontCreateCopyWithAttributes(
            font.native_font().as_concrete_TypeRef(),
            0.0,
            std::ptr::null(),
            new_descriptor.as_concrete_TypeRef(),
        );
        anyhow::ensure!(!new_font.is_null(), "failed to create configured font");
        let new_font = CTFont::wrap_under_create_rule(new_font);
        *font = font_kit::font::Font::from_native_font(&new_font);

        Ok(())
    }
}

fn generate_feature_array(features: &FontFeatures) -> anyhow::Result<CFMutableArrayRef> {
    unsafe {
        let feature_array = CFArrayCreateMutable(kCFAllocatorDefault, 0, &kCFTypeArrayCallBacks);
        anyhow::ensure!(
            !feature_array.is_null(),
            "failed to create font feature array"
        );
        for (tag, value) in features.tag_value_list() {
            let keys = [kCTFontOpenTypeFeatureTag, kCTFontOpenTypeFeatureValue];
            let tag = CFString::new(tag);
            let value = CFNumber::from(*value as i32);
            let values = [tag.as_CFTypeRef(), value.as_CFTypeRef()];
            let dict = CFDictionaryCreate(
                kCFAllocatorDefault,
                &keys as *const _ as _,
                &values as *const _ as _,
                2,
                &kCFTypeDictionaryKeyCallBacks,
                &kCFTypeDictionaryValueCallBacks,
            );
            if dict.is_null() {
                CFRelease(feature_array as _);
                return Err(anyhow::anyhow!("failed to create font feature dictionary"));
            }
            CFArrayAppendValue(feature_array, dict as _);
            CFRelease(dict as _);
        }
        Ok(feature_array)
    }
}

fn generate_fallback_array(
    fallbacks: &FontFallbacks,
    font_ref: CTFontRef,
) -> anyhow::Result<CFMutableArrayRef> {
    unsafe {
        let fallback_array = CFArrayCreateMutable(kCFAllocatorDefault, 0, &kCFTypeArrayCallBacks);
        anyhow::ensure!(
            !fallback_array.is_null(),
            "failed to create font fallback array"
        );
        for user_fallback in fallbacks.fallback_list() {
            let name = CFString::from(user_fallback.as_str());
            let fallback_desc =
                CTFontDescriptorCreateWithNameAndSize(name.as_concrete_TypeRef(), 0.0);
            if fallback_desc.is_null() {
                CFRelease(fallback_array as _);
                return Err(anyhow::anyhow!("failed to create fallback font descriptor"));
            }
            CFArrayAppendValue(fallback_array, fallback_desc as _);
            CFRelease(fallback_desc as _);
        }
        if let Err(error) = append_system_fallbacks(fallback_array, font_ref) {
            CFRelease(fallback_array as _);
            return Err(error);
        }
        Ok(fallback_array)
    }
}

fn append_system_fallbacks(
    fallback_array: CFMutableArrayRef,
    font_ref: CTFontRef,
) -> anyhow::Result<()> {
    unsafe {
        let preferred_languages = CFLocaleCopyPreferredLanguages();
        anyhow::ensure!(
            !preferred_languages.is_null(),
            "failed to read preferred languages"
        );
        let preferred_languages: CFArray<CFString> =
            CFArray::wrap_under_create_rule(preferred_languages);

        let default_fallbacks = CTFontCopyDefaultCascadeListForLanguages(
            font_ref,
            preferred_languages.as_concrete_TypeRef(),
        );
        anyhow::ensure!(
            !default_fallbacks.is_null(),
            "failed to read system font fallbacks"
        );
        let default_fallbacks: CFArray<CTFontDescriptor> =
            CFArray::wrap_under_create_rule(default_fallbacks);

        default_fallbacks
            .iter()
            .take(MAX_FONT_FALLBACKS)
            .filter(|desc| desc.font_path().is_some())
            .for_each(|desc| {
                CFArrayAppendValue(fallback_array, desc.as_concrete_TypeRef() as _);
            });
        Ok(())
    }
}

#[link(name = "CoreText", kind = "framework")]
unsafe extern "C" {
    static kCTFontOpenTypeFeatureTag: CFStringRef;
    static kCTFontOpenTypeFeatureValue: CFStringRef;

    fn CTFontCreateCopyWithAttributes(
        font: CTFontRef,
        size: CGFloat,
        matrix: *const CGAffineTransform,
        attributes: CTFontDescriptorRef,
    ) -> CTFontRef;
    fn CTFontCopyDefaultCascadeListForLanguages(
        font: CTFontRef,
        languagePrefList: CFArrayRef,
    ) -> CFArrayRef;
}
