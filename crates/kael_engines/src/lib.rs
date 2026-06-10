#![deny(missing_docs)]

//! General-purpose workload engines for the Kael desktop framework.
//!
//! Domain-neutral building blocks any desktop application can use: Unicode text
//! correctness ([`bidi`] UAX#9, [`linebreak`] UAX#14), transactional [`undo`],
//! [`crash_report`] capture, and data models for common app shapes ([`canvas`],
//! [`dashboard`], [`ide`]).
//!
//! Media/NLE engines (timeline, compositing, export) live in the separate,
//! optional `kael_media_engines` crate — see the project's `VISION.md` for the
//! layering rule that keeps this crate domain-neutral.

pub mod bidi;
pub mod canvas;
pub mod crash_report;
pub mod dashboard;
pub mod ide;
pub mod linebreak;
pub mod undo;
