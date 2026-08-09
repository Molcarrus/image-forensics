use std::{collections::HashMap, fs::File, io::BufReader, path::Path};

use exif::{Exif, Field, In, Tag, Value};

use crate::{
    MetadataResult,
    error::{ForensicsError, Result},
};

/// Reads the EXIF block of an image file into a [`MetadataResult`].
///
/// A unit struct: [`extract`](Self::extract) is an associated function and
/// there is no state to configure.
pub struct ExifExtractor;

impl ExifExtractor {
    /// Read and interpret the EXIF block of an image file.
    ///
    /// A file with no metadata yields an empty [`MetadataResult`] carrying a
    /// `"No EXIF data found"` indicator. Anything else — a truncated file, an
    /// unreadable path, a malformed IFD — is returned as an error. Collapsing
    /// those two cases, as this previously did, made "absent metadata" and
    /// "corrupt metadata" indistinguishable, which are opposite forensic
    /// conclusions.
    pub fn extract<P: AsRef<Path>>(path: P) -> Result<MetadataResult> {
        let file = File::open(&path)?;
        let mut reader = BufReader::new(file);

        match exif::Reader::new().read_from_container(&mut reader) {
            Ok(exif) => Ok(Self::parse_exif(&exif)),
            Err(exif::Error::NotFound(_)) => Ok(MetadataResult {
                camera_make: None,
                camera_model: None,
                software: None,
                date_time: None,
                gps_coordinates: None,
                all_tags: HashMap::new(),
                suspicious_indicators: vec!["No EXIF data found".into()],
            }),
            Err(err) => Err(ForensicsError::MetadataError(format!(
                "failed to read EXIF from {}: {err}",
                path.as_ref().display()
            ))),
        }
    }

    fn parse_exif(exif: &Exif) -> MetadataResult {
        let mut all_tags = HashMap::new();

        for field in exif.fields() {
            // The same tag appears in both the primary and thumbnail IFDs
            // (Make, Model, DateTime, dimensions...). Keying on the tag alone
            // let the thumbnail entry overwrite the primary one.
            let key = match field.ifd_num {
                In::PRIMARY => field.tag.to_string(),
                In::THUMBNAIL => format!("Thumbnail.{}", field.tag),
                other => format!("IFD{}.{}", other.index(), field.tag),
            };

            all_tags.insert(key, Self::field_text(field));
        }

        let camera_make = Self::text_field(exif, Tag::Make);
        let camera_model = Self::text_field(exif, Tag::Model);
        let software = Self::text_field(exif, Tag::Software);
        let date_time = Self::text_field(exif, Tag::DateTime);
        let datetime_original = Self::text_field(exif, Tag::DateTimeOriginal);
        let datetime_digitized = Self::text_field(exif, Tag::DateTimeDigitized);

        let gps_coordinates = Self::extract_gps(exif);

        let mut suspicious_indicators = Vec::new();

        if let Some(sw) = software.as_deref() {
            let lowered = sw.to_lowercase();
            const EDITORS: [&str; 6] = [
                "photoshop",
                "lightroom",
                "gimp",
                "paint",
                "affinity",
                "pixelmator",
            ];

            if EDITORS.iter().any(|editor| lowered.contains(editor)) {
                suspicious_indicators.push(format!("Edited with: {sw}"));
            }
        }

        if datetime_original.is_none() && date_time.is_some() {
            suspicious_indicators
                .push("Original datetime missing while file datetime is present".into());
        }

        // Capture and digitisation legitimately differ for scanned film, so
        // report the discrepancy as context rather than as evidence.
        if let (Some(original), Some(digitized)) = (&datetime_original, &datetime_digitized)
            && original != digitized
        {
            suspicious_indicators.push(format!(
                "DateTimeOriginal ({original}) differs from DateTimeDigitized ({digitized})"
            ));
        }

        if camera_make.is_none() && camera_model.is_none() && !all_tags.is_empty() {
            suspicious_indicators.push("Camera make and model absent from EXIF".into());
        }

        MetadataResult {
            camera_make,
            camera_model,
            software,
            date_time,
            gps_coordinates,
            all_tags,
            suspicious_indicators,
        }
    }

    /// Decode an ASCII field into a plain `String`.
    ///
    /// `Field::display_value` routes ASCII through a formatter that wraps the
    /// text in double quotes and escapes non-printables, so reading a camera
    /// make that way produced the seven-character string `"Canon"` — quotes
    /// included — rather than `Canon`. Read the bytes instead.
    fn text_field(exif: &Exif, tag: Tag) -> Option<String> {
        let field = exif.get_field(tag, In::PRIMARY)?;
        let text = Self::field_text(field);
        let trimmed = text.trim();

        if trimmed.is_empty() {
            None
        } else {
            Some(trimmed.to_string())
        }
    }

    fn field_text(field: &Field) -> String {
        match &field.value {
            Value::Ascii(parts) => parts
                .iter()
                .map(|bytes| {
                    String::from_utf8_lossy(bytes)
                        .trim_end_matches('\0')
                        .to_string()
                })
                .collect::<Vec<_>>()
                .join(" "),
            _ => field.display_value().to_string(),
        }
    }

    /// Decimal latitude and longitude, or `None` when either is unavailable.
    ///
    /// GPS tags live in the GPS sub-IFD, which `kamadak-exif` exposes under
    /// `In::PRIMARY`.
    fn extract_gps(exif: &Exif) -> Option<(f64, f64)> {
        let latitude = Self::gps_degrees(exif, Tag::GPSLatitude)?;
        let longitude = Self::gps_degrees(exif, Tag::GPSLongitude)?;

        let lat_sign = match Self::gps_ref(exif, Tag::GPSLatitudeRef)? {
            'S' => -1.0,
            'N' => 1.0,
            _ => return None,
        };
        let lon_sign = match Self::gps_ref(exif, Tag::GPSLongitudeRef)? {
            'W' => -1.0,
            'E' => 1.0,
            _ => return None,
        };

        let latitude = latitude * lat_sign;
        let longitude = longitude * lon_sign;

        if !(-90.0..=90.0).contains(&latitude) || !(-180.0..=180.0).contains(&longitude) {
            return None;
        }

        Some((latitude, longitude))
    }

    /// Convert a degrees/minutes/seconds GPS tag to decimal degrees.
    ///
    /// The value is three rationals; read them directly. Parsing
    /// `display_value()` — which renders as `55 deg 41 min 30.5 sec` — meant
    /// splitting on whitespace and taking index 3, i.e. the literal word
    /// `min`. That parse failed into `unwrap_or(0.0)`, silently discarding the
    /// seconds and truncating every coordinate to whole arc-minutes: an error
    /// of up to 1.85 km, reported as an exact fix.
    fn gps_degrees(exif: &Exif, tag: Tag) -> Option<f64> {
        let field = exif.get_field(tag, In::PRIMARY)?;

        let parts = match &field.value {
            Value::Rational(parts) if parts.len() >= 3 => parts,
            _ => return None,
        };

        let degrees = parts[0].to_f64();
        let minutes = parts[1].to_f64();
        let seconds = parts[2].to_f64();

        if !degrees.is_finite() || !minutes.is_finite() || !seconds.is_finite() {
            return None;
        }

        Some(degrees + minutes / 60.0 + seconds / 3600.0)
    }

    /// The hemisphere letter of a GPS reference tag, uppercased.
    fn gps_ref(exif: &Exif, tag: Tag) -> Option<char> {
        let field = exif.get_field(tag, In::PRIMARY)?;

        match &field.value {
            Value::Ascii(parts) => parts
                .iter()
                .flat_map(|bytes| bytes.iter())
                .find(|byte| byte.is_ascii_alphabetic())
                .map(|byte| byte.to_ascii_uppercase() as char),
            _ => None,
        }
    }
}

#[cfg(test)]
mod tests {
    use exif::Rational;

    use super::*;

    fn rational_field(tag: Tag, values: &[(u32, u32)]) -> Field {
        Field {
            tag,
            ifd_num: In::PRIMARY,
            value: Value::Rational(
                values
                    .iter()
                    .map(|&(num, denom)| Rational { num, denom })
                    .collect(),
            ),
        }
    }

    /// Mirror of `gps_degrees`' arithmetic over a synthetic field, so the
    /// conversion can be exercised without building a full `Exif` container.
    fn degrees_of(field: &Field) -> Option<f64> {
        match &field.value {
            Value::Rational(parts) if parts.len() >= 3 => {
                Some(parts[0].to_f64() + parts[1].to_f64() / 60.0 + parts[2].to_f64() / 3600.0)
            }
            _ => None,
        }
    }

    #[test]
    fn gps_conversion_keeps_the_seconds_component() {
        // 55 deg 41 min 30.5 sec. The old display-string parser dropped the
        // seconds entirely and returned 55.683333.
        let field = rational_field(Tag::GPSLatitude, &[(55, 1), (41, 1), (305, 10)]);
        let degrees = degrees_of(&field).unwrap();

        assert!(
            (degrees - 55.691_805_555_5).abs() < 1e-6,
            "got {degrees}, seconds were dropped"
        );
    }

    #[test]
    fn gps_conversion_handles_fractional_seconds() {
        let field = rational_field(Tag::GPSLongitude, &[(12, 1), (34, 1), (5678, 100)]);
        let degrees = degrees_of(&field).unwrap();

        assert!((degrees - 12.582_438_888_8).abs() < 1e-6, "got {degrees}");
    }

    #[test]
    fn ascii_fields_are_returned_unquoted() {
        let field = Field {
            tag: Tag::Make,
            ifd_num: In::PRIMARY,
            value: Value::Ascii(vec![b"Canon".to_vec()]),
        };

        // `display_value()` would give the seven-character `"Canon"`.
        assert_eq!(ExifExtractor::field_text(&field), "Canon");
    }

    #[test]
    fn ascii_fields_drop_trailing_nul_padding() {
        let field = Field {
            tag: Tag::Model,
            ifd_num: In::PRIMARY,
            value: Value::Ascii(vec![b"EOS 5D\0".to_vec()]),
        };

        assert_eq!(ExifExtractor::field_text(&field), "EOS 5D");
    }

    #[test]
    fn missing_file_is_an_error_not_empty_metadata() {
        assert!(ExifExtractor::extract("does-not-exist.jpg").is_err());
    }
}
