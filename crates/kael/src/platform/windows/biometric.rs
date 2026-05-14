use crate::{BiometricKind, BiometricStatus};
use windows::{
    Security::Credentials::UI::{
        UserConsentVerificationResult, UserConsentVerifier, UserConsentVerifierAvailability,
    },
    core::HSTRING,
};

pub fn biometric_status() -> BiometricStatus {
    let availability = match UserConsentVerifier::CheckAvailabilityAsync().and_then(|op| op.get()) {
        Ok(availability) => availability,
        Err(_) => return BiometricStatus::Unavailable,
    };

    if availability == UserConsentVerifierAvailability::Available {
        BiometricStatus::Available(BiometricKind::WindowsHello)
    } else {
        BiometricStatus::Unavailable
    }
}

pub fn authenticate_biometric(reason: &str, callback: Box<dyn FnOnce(bool) + Send>) {
    let message = HSTRING::from(reason);
    let result = UserConsentVerifier::RequestVerificationAsync(&message).and_then(|op| op.get());

    match result {
        Ok(verification_result) => {
            callback(verification_result == UserConsentVerificationResult::Verified);
        }
        Err(_) => {
            callback(false);
        }
    }
}
