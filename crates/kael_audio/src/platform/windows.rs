use super::PlatformAudioSupport;

pub(crate) const SUPPORT: PlatformAudioSupport = PlatformAudioSupport {
    playback_backend: "rodio+wasapi",
    session_backend: "audiosessioncontrol-planned",
    spatial_backend: "windows-spatial-audio-planned",
};