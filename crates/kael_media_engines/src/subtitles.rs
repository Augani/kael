//! SubRip (`.srt`) subtitle parsing, querying, and serialization.
//!
//! A [`SubtitleTrack`] holds time-ordered [`SubtitleCue`]s and answers "which caption is
//! on screen at time T". Parsing is tolerant — malformed blocks are skipped rather than
//! failing the whole file — and `to_srt` round-trips cleanly.

use serde::{Deserialize, Deserializer, Serialize};

/// One subtitle caption: a text span shown over a half-open time range, in milliseconds.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct SubtitleCue {
    /// Start time in milliseconds (inclusive).
    pub start_ms: u64,
    /// End time in milliseconds (exclusive).
    pub end_ms: u64,
    /// Caption text; may contain embedded newlines.
    pub text: String,
}

/// A time-ordered set of subtitle cues.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize)]
pub struct SubtitleTrack {
    cues: Vec<SubtitleCue>,
}

impl<'de> Deserialize<'de> for SubtitleTrack {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: Deserializer<'de>,
    {
        #[derive(Deserialize)]
        struct RawTrack {
            #[serde(default)]
            cues: Vec<SubtitleCue>,
        }

        let mut cues = RawTrack::deserialize(deserializer)?.cues;
        cues.retain(|cue| cue.start_ms < cue.end_ms && !cue.text.is_empty());
        cues.sort_by_key(|cue| (cue.start_ms, cue.end_ms));
        Ok(Self { cues })
    }
}

impl SubtitleTrack {
    /// Parse a SubRip document into a track (cues sorted by start time).
    pub fn from_srt(input: &str) -> Self {
        let mut cues = parse_srt(input);
        cues.sort_by_key(|cue| (cue.start_ms, cue.end_ms));
        Self { cues }
    }

    /// Parse a WebVTT document into a track (cues sorted by start time).
    pub fn from_webvtt(input: &str) -> Self {
        let mut cues = parse_webvtt(input);
        cues.sort_by_key(|cue| (cue.start_ms, cue.end_ms));
        Self { cues }
    }

    /// The cues, in start-time order.
    pub fn cues(&self) -> &[SubtitleCue] {
        &self.cues
    }

    /// The caption visible at `time_ms`, if any. When cues overlap, the earliest-
    /// starting one wins.
    pub fn active_at(&self, time_ms: u64) -> Option<&SubtitleCue> {
        self.cues
            .iter()
            .find(|cue| time_ms >= cue.start_ms && time_ms < cue.end_ms)
    }

    /// Serialize back to a SubRip document.
    pub fn to_srt(&self) -> String {
        let mut out = String::new();
        for (index, cue) in self.cues.iter().enumerate() {
            out.push_str(&format!("{}\n", index + 1));
            out.push_str(&format!(
                "{} --> {}\n",
                format_timestamp(cue.start_ms),
                format_timestamp(cue.end_ms)
            ));
            out.push_str(&cue.text);
            out.push_str("\n\n");
        }
        out
    }

    /// Serialize to a WebVTT document.
    pub fn to_webvtt(&self) -> String {
        let mut out = String::from("WEBVTT\n\n");
        for cue in &self.cues {
            out.push_str(&format!(
                "{} --> {}\n",
                format_timestamp_sep(cue.start_ms, '.'),
                format_timestamp_sep(cue.end_ms, '.')
            ));
            out.push_str(&cue.text);
            out.push_str("\n\n");
        }
        out
    }

    /// Shift every cue by `delta_ms` (negative moves earlier, clamped at zero) to
    /// re-sync the track. Cues are re-sorted afterwards.
    pub fn shift(&mut self, delta_ms: i64) {
        for cue in &mut self.cues {
            if delta_ms >= 0 {
                let delta = delta_ms as u64;
                cue.start_ms = cue.start_ms.saturating_add(delta);
                cue.end_ms = cue.end_ms.saturating_add(delta);
            } else {
                let delta = delta_ms.unsigned_abs();
                cue.start_ms = cue.start_ms.saturating_sub(delta);
                cue.end_ms = cue.end_ms.saturating_sub(delta);
            }
        }
        self.cues.sort_by_key(|cue| (cue.start_ms, cue.end_ms));
    }
}

/// Parse a SubRip document into cues (unsorted, malformed blocks skipped).
pub fn parse_srt(input: &str) -> Vec<SubtitleCue> {
    let normalized = input.replace("\r\n", "\n").replace('\r', "\n");
    let mut cues = Vec::new();
    for block in normalized.split("\n\n") {
        let lines: Vec<&str> = block
            .lines()
            .filter(|line| !line.trim().is_empty())
            .collect();
        let Some(timing_index) = lines.iter().position(|line| line.contains("-->")) else {
            continue;
        };
        let Some((start_text, end_text)) = lines[timing_index].split_once("-->") else {
            continue;
        };
        let (Some(start_ms), Some(end_ms)) =
            (parse_timestamp(start_text), parse_timestamp(end_text))
        else {
            continue;
        };
        let text = lines[timing_index + 1..].join("\n");
        if text.is_empty() {
            continue;
        }
        if start_ms < end_ms {
            cues.push(SubtitleCue {
                start_ms,
                end_ms,
                text,
            });
        }
    }
    cues
}

/// Parse a WebVTT document into cues (unsorted; header, `NOTE`/`STYLE`/`REGION` blocks,
/// and malformed blocks are skipped; trailing cue settings on the timing line are ignored).
pub fn parse_webvtt(input: &str) -> Vec<SubtitleCue> {
    let normalized = input.replace("\r\n", "\n").replace('\r', "\n");
    let mut cues = Vec::new();
    for block in normalized.split("\n\n") {
        let lines: Vec<&str> = block
            .lines()
            .filter(|line| !line.trim().is_empty())
            .collect();
        let Some(first) = lines.first() else {
            continue;
        };
        if first.starts_with("WEBVTT")
            || block_directive(first, "NOTE")
            || block_directive(first, "STYLE")
            || block_directive(first, "REGION")
        {
            continue;
        }
        let Some(timing_index) = lines.iter().position(|line| line.contains("-->")) else {
            continue;
        };
        let Some((start_text, rest)) = lines[timing_index].split_once("-->") else {
            continue;
        };
        // The end timestamp is the first token after `-->`; cue settings follow it.
        let end_text = rest.split_whitespace().next().unwrap_or_default();
        let (Some(start_ms), Some(end_ms)) =
            (parse_timestamp(start_text), parse_timestamp(end_text))
        else {
            continue;
        };
        let text = lines[timing_index + 1..].join("\n");
        if text.is_empty() {
            continue;
        }
        if start_ms < end_ms {
            cues.push(SubtitleCue {
                start_ms,
                end_ms,
                text,
            });
        }
    }
    cues
}

fn block_directive(line: &str, directive: &str) -> bool {
    line == directive
        || line
            .strip_prefix(directive)
            .is_some_and(|rest| rest.starts_with(' ') || rest.starts_with('\t'))
}

fn parse_timestamp(text: &str) -> Option<u64> {
    // Accept both SubRip (`HH:MM:SS,mmm`) and WebVTT (`HH:MM:SS.mmm` or `MM:SS.mmm`).
    let (clock, millis) = text.trim().split_once(['.', ','])?;
    let milliseconds: u64 = millis.parse().ok()?;
    if milliseconds >= 1000 {
        return None;
    }
    let parts: Vec<&str> = clock.split(':').collect();
    let (hours, minutes, seconds): (u64, u64, u64) = match parts.len() {
        3 => (
            parts[0].trim().parse().ok()?,
            parts[1].parse().ok()?,
            parts[2].parse().ok()?,
        ),
        2 => (0, parts[0].trim().parse().ok()?, parts[1].parse().ok()?),
        _ => return None,
    };
    if minutes >= 60 || seconds >= 60 {
        return None;
    }
    hours
        .checked_mul(3_600_000)?
        .checked_add(minutes.checked_mul(60_000)?)?
        .checked_add(seconds.checked_mul(1_000)?)?
        .checked_add(milliseconds)
}

fn format_timestamp(ms: u64) -> String {
    format_timestamp_sep(ms, ',')
}

fn format_timestamp_sep(ms: u64, separator: char) -> String {
    let hours = ms / 3_600_000;
    let minutes = (ms % 3_600_000) / 60_000;
    let seconds = (ms % 60_000) / 1_000;
    let millis = ms % 1_000;
    format!("{hours:02}:{minutes:02}:{seconds:02}{separator}{millis:03}")
}

#[cfg(test)]
mod tests {
    use super::*;

    const SAMPLE: &str = "1\n00:00:01,000 --> 00:00:04,000\nHello world\n\n2\n00:00:05,500 --> 00:00:08,000\nSecond\nline\n";

    #[test]
    fn parses_cues_with_timestamps_and_text() {
        let track = SubtitleTrack::from_srt(SAMPLE);
        assert_eq!(track.cues().len(), 2);
        assert_eq!(track.cues()[0].start_ms, 1000);
        assert_eq!(track.cues()[0].end_ms, 4000);
        assert_eq!(track.cues()[0].text, "Hello world");
        assert_eq!(track.cues()[1].start_ms, 5500);
        // Multi-line caption text is preserved.
        assert_eq!(track.cues()[1].text, "Second\nline");
    }

    #[test]
    fn active_at_resolves_the_on_screen_caption() {
        let track = SubtitleTrack::from_srt(SAMPLE);
        assert_eq!(
            track.active_at(2000).map(|c| c.text.as_str()),
            Some("Hello world")
        );
        // Gap between cues -> nothing.
        assert_eq!(track.active_at(4500), None);
        assert_eq!(
            track.active_at(6000).map(|c| c.text.as_str()),
            Some("Second\nline")
        );
        // End is exclusive.
        assert_eq!(track.active_at(4000), None);
    }

    #[test]
    fn tolerates_crlf_and_skips_malformed_blocks() {
        let input = "1\r\n00:00:01,000 --> 00:00:02,000\r\nok\r\n\r\ngarbage block\r\n\r\n3\r\n00:00:03,000 --> 00:00:04,000\r\nalso ok\r\n";
        let track = SubtitleTrack::from_srt(input);
        assert_eq!(track.cues().len(), 2);
        assert_eq!(track.cues()[0].text, "ok");
        assert_eq!(track.cues()[1].text, "also ok");
    }

    #[test]
    fn rejects_out_of_range_timestamp_fields() {
        assert!(parse_timestamp("00:60:00,000").is_none());
        assert!(parse_timestamp("00:00:00,1000").is_none());
        assert!(parse_timestamp("not a time").is_none());
        assert_eq!(parse_timestamp("01:02:03,004"), Some(3_723_004));
        assert!(parse_timestamp("999999999999999999:00:00,000").is_none());
    }

    #[test]
    fn round_trips_through_srt() {
        let track = SubtitleTrack::from_srt(SAMPLE);
        let reparsed = SubtitleTrack::from_srt(&track.to_srt());
        assert_eq!(reparsed, track);
    }

    #[test]
    fn format_timestamp_pads_fields() {
        assert_eq!(format_timestamp(3_723_004), "01:02:03,004");
        assert_eq!(format_timestamp(0), "00:00:00,000");
    }

    const VTT: &str = "WEBVTT\n\nNOTE this is a comment\n\n1\n00:00:01.000 --> 00:00:04.000 position:50%\nHello world\n\n00:05.500 --> 00:08.000\nSecond\nline\n";

    #[test]
    fn parses_webvtt_with_header_notes_settings_and_short_timestamps() {
        let track = SubtitleTrack::from_webvtt(VTT);
        assert_eq!(track.cues().len(), 2);
        // Dot-separated milliseconds, cue settings stripped from the timing line.
        assert_eq!(track.cues()[0].start_ms, 1000);
        assert_eq!(track.cues()[0].end_ms, 4000);
        assert_eq!(track.cues()[0].text, "Hello world");
        // MM:SS.mmm form (no hours) is accepted.
        assert_eq!(track.cues()[1].start_ms, 5500);
        assert_eq!(track.cues()[1].end_ms, 8000);
        assert_eq!(track.cues()[1].text, "Second\nline");
    }

    #[test]
    fn webvtt_active_at_resolves_caption() {
        let track = SubtitleTrack::from_webvtt(VTT);
        assert_eq!(
            track.active_at(2000).map(|c| c.text.as_str()),
            Some("Hello world")
        );
        assert_eq!(track.active_at(4500), None);
    }

    #[test]
    fn timestamp_parser_accepts_both_separators() {
        // SubRip comma and WebVTT dot resolve to the same value.
        assert_eq!(parse_timestamp("00:00:01,500"), Some(1500));
        assert_eq!(parse_timestamp("00:00:01.500"), Some(1500));
        assert_eq!(parse_timestamp("01:30.250"), Some(90_250));
    }

    #[test]
    fn webvtt_round_trips_through_serialization() {
        let track = SubtitleTrack::from_srt(SAMPLE);
        let vtt = track.to_webvtt();
        assert!(vtt.starts_with("WEBVTT\n\n"));
        assert!(vtt.contains("00:00:01.000 --> 00:00:04.000"));
        assert_eq!(SubtitleTrack::from_webvtt(&vtt), track);
    }

    #[test]
    fn shift_resyncs_cue_timing() {
        let mut track = SubtitleTrack::from_srt(SAMPLE);
        track.shift(500);
        assert_eq!(track.cues()[0].start_ms, 1500);
        assert_eq!(track.cues()[0].end_ms, 4500);

        // Negative shift clamps at zero.
        track.shift(-100_000);
        assert_eq!(track.cues()[0].start_ms, 0);
        assert_eq!(track.cues()[0].end_ms, 0);
    }

    #[test]
    fn shift_handles_the_full_u64_and_i64_domains() {
        let mut track = SubtitleTrack {
            cues: vec![SubtitleCue {
                start_ms: u64::MAX - 1,
                end_ms: u64::MAX,
                text: "late".into(),
            }],
        };
        track.shift(i64::MAX);
        assert_eq!(track.cues[0].start_ms, u64::MAX);
        assert_eq!(track.cues[0].end_ms, u64::MAX);
        track.shift(i64::MIN);
        assert_eq!(track.cues[0].start_ms, u64::MAX - (1u64 << 63));
    }

    #[test]
    fn parsers_reject_empty_or_reversed_ranges() {
        assert!(parse_srt("1\n00:00:02,000 --> 00:00:01,000\nbad\n").is_empty());
        assert!(parse_webvtt("WEBVTT\n\n00:02.000 --> 00:01.000\nbad\n").is_empty());
    }

    #[test]
    fn deserialization_restores_invariants() {
        let json = r#"{"cues":[
            {"start_ms":10,"end_ms":20,"text":"later"},
            {"start_ms":0,"end_ms":5,"text":"first"},
            {"start_ms":8,"end_ms":8,"text":"empty range"},
            {"start_ms":6,"end_ms":7,"text":""}
        ]}"#;
        let track: SubtitleTrack = serde_json::from_str(json).unwrap();
        assert_eq!(track.cues.len(), 2);
        assert_eq!(track.cues[0].text, "first");
        assert_eq!(track.cues[1].text, "later");
    }

    #[test]
    fn cue_identifiers_that_begin_with_directive_names_are_not_dropped() {
        let input = "WEBVTT\n\nNOTEBOOK\n00:00.000 --> 00:01.000\nkept\n";
        assert_eq!(SubtitleTrack::from_webvtt(input).cues[0].text, "kept");
    }
}
