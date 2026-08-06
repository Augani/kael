use super::PlatformAudioSupport;

pub(crate) const SUPPORT: PlatformAudioSupport = PlatformAudioSupport {
    playback_backend: "coreaudio via cpal and rodio",
    capture_backend: "coreaudio via cpal",
    session_backend: "application state model",
    spatial_backend: "stereo scene processor",
};
