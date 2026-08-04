#![deny(missing_docs)]

//! Optional media/NLE engines for the Kael desktop framework.
//!
//! This crate is a **leaf domain stack**: it builds media-application capability
//! (timelines, compositing, audio mixing, export) *on top of* the general-purpose
//! framework, and nothing in the core `kael` crate depends on it. This keeps
//! media-specific engines out of the general framework dependency graph.

pub mod audio_mix;
pub mod automation;
pub mod compositor;
pub mod effects;
pub mod export;
pub mod frame_cache;
pub mod generators;
pub mod markers;
pub mod media;
pub mod playback;
pub mod project;
pub mod scopes;
pub mod subtitles;
pub mod timecode;
pub mod transform;
