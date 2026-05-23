use super::PlatformAudioSupport;

pub(crate) const SUPPORT: PlatformAudioSupport = PlatformAudioSupport {
    playback_backend: "rodio+coreaudio",
    session_backend: "avaudio-session-planned",
    spatial_backend: "avaudioenvironment-planned",
};
