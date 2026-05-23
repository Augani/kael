use super::PlatformAudioSupport;

pub(crate) const SUPPORT: PlatformAudioSupport = PlatformAudioSupport {
    playback_backend: "rodio+cpal",
    session_backend: "pipewire-pulseaudio-planned",
    spatial_backend: "openal-planned",
};