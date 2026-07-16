use crate::WindowAppearance;
use objc2::runtime::AnyObject;
use objc2_app_kit::NSAppearance;

#[allow(non_camel_case_types)]
type id = *mut AnyObject;

impl WindowAppearance {
    pub(crate) unsafe fn from_native(appearance: id) -> Self {
        if appearance.is_null() {
            log::error!("macOS returned no effective window appearance");
            return Self::Light;
        }
        let appearance: &NSAppearance = unsafe { &*(appearance as *const NSAppearance) };
        let native_name = appearance.name();
        if native_name.length() > 256 {
            return Self::Light;
        }
        let name = native_name.to_string();
        match name.as_str() {
            "NSAppearanceNameVibrantLight" => Self::VibrantLight,
            "NSAppearanceNameVibrantDark" => Self::VibrantDark,
            "NSAppearanceNameAqua" => Self::Light,
            "NSAppearanceNameDarkAqua" => Self::Dark,
            _ => Self::Light,
        }
    }
}
