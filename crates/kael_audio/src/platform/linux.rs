use super::PlatformAudioSupport;

pub(crate) const SUPPORT: PlatformAudioSupport = PlatformAudioSupport {
    playback_backend: "default host via cpal and rodio",
    capture_backend: "default host via cpal",
    session_backend: "application state model",
    spatial_backend: "stereo scene processor",
};
