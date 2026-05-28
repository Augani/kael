use crate::{BiometricKind, BiometricStatus};
use block2::RcBlock;
use objc2::msg_send;
use objc2::runtime::{AnyClass, AnyObject, Bool};
use objc2_foundation::NSString;

const LA_POLICY_BIOMETRICS: i64 = 1;

#[link(name = "LocalAuthentication", kind = "framework")]
unsafe extern "C" {}

fn la_context_class() -> Option<&'static AnyClass> {
    AnyClass::get(c"LAContext")
}

pub fn biometric_status() -> BiometricStatus {
    let Some(class) = la_context_class() else {
        return BiometricStatus::Unavailable;
    };
    unsafe {
        let context: *mut AnyObject = msg_send![class, new];
        let mut error: *mut AnyObject = std::ptr::null_mut();
        let can_evaluate: Bool = msg_send![
            context,
            canEvaluatePolicy: LA_POLICY_BIOMETRICS,
            error: &mut error
        ];
        let _: () = msg_send![context, release];
        if can_evaluate.as_bool() {
            BiometricStatus::Available(BiometricKind::TouchId)
        } else {
            BiometricStatus::Unavailable
        }
    }
}

pub fn authenticate_biometric(reason: &str, callback: Box<dyn FnOnce(bool) + Send>) {
    let Some(class) = la_context_class() else {
        callback(false);
        return;
    };
    unsafe {
        let context: *mut AnyObject = msg_send![class, new];
        let reason_ns = NSString::from_str(reason);

        let callback = std::sync::Mutex::new(Some(callback));
        let block = RcBlock::new(move |success: Bool, _error: *mut AnyObject| {
            if let Some(cb) = callback.lock().ok().and_then(|mut g| g.take()) {
                cb(success.as_bool());
            }
        });

        let _: () = msg_send![
            context,
            evaluatePolicy: LA_POLICY_BIOMETRICS,
            localizedReason: &*reason_ns,
            reply: &*block
        ];

        let _: () = msg_send![context, release];
    }
}
