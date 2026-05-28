use crate::WindowAppearance;
use objc2::runtime::AnyObject;
use objc2_app_kit::NSAppearance;

#[allow(non_camel_case_types)]
type id = *mut AnyObject;

impl WindowAppearance {
    pub(crate) unsafe fn from_native(appearance: id) -> Self {
        let appearance: &NSAppearance = unsafe { &*(appearance as *const NSAppearance) };
        let name = appearance.name().to_string();
        match name.as_str() {
            "NSAppearanceNameVibrantLight" => Self::VibrantLight,
            "NSAppearanceNameVibrantDark" => Self::VibrantDark,
            "NSAppearanceNameAqua" => Self::Light,
            "NSAppearanceNameDarkAqua" => Self::Dark,
            other => {
                println!("unknown appearance: {other}");
                Self::Light
            }
        }
    }
}
