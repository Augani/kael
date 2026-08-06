use super::PlatformAudioSupport;

pub(crate) const SUPPORT: PlatformAudioSupport = PlatformAudioSupport {
    playback_backend: "wasapi via cpal and rodio",
    capture_backend: "wasapi via cpal",
    session_backend: "application state model",
    spatial_backend: "stereo scene processor",
};
