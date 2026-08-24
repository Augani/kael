use super::PlatformAudioSupport;

pub(crate) const SUPPORT: PlatformAudioSupport = PlatformAudioSupport {
    playback_backend: "HTMLAudioElement + bounded AudioWorklet mixer",
    capture_backend: "getUserMedia + credit-bounded AudioWorklet capture",
    session_backend: "application state model (Media Session not integrated)",
    spatial_backend: "offline stereo scene processor",
};
