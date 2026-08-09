//! Metadata extraction.
//!
//! EXIF is the cheapest evidence in a file and the first thing to read — and
//! also trivially forged, so its absence proves nothing and its presence
//! proves nothing. Internal *inconsistency* is what carries information.

/// EXIF parsing and interpretation.
pub mod exif;
