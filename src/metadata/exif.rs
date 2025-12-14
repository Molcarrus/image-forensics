use std::{collections::HashMap, fs::File, io::BufReader, path::Path};

use crate::{MetadataResult, error::Result};

pub struct ExifExtractor;

impl ExifExtractor {
    pub fn extract<P: AsRef<Path>>(path: P) -> Result<MetadataResult> {
        let file = File::open(&path)?;
        let mut reader = BufReader::new(file);
        
        let exif_reader = exif::Reader::new();
        
        match exif_reader.read_from_container(&mut reader) {
            Ok(exif_data) => Self::parse_exif(exif_data),
            Err(_) => Ok(MetadataResult { 
                camera_make: None, 
                camera_model: None, 
                software: None, 
                date_time: None, 
                gps_coordinates: None, 
                all_tags: HashMap::new(), 
                suspicious_indicators: vec!["No EXIF data found".into()] 
            }),
        }
    }
    
    fn parse_exif(exif: exif::Exif) -> Result<MetadataResult> {
        let mut all_tags = HashMap::new();
        let mut suspicious_indicators = Vec::new();
      
        for field in exif.fields() {
            let tag_name = format!("{}", field.tag);
            let value = field.display_value().to_string();
            all_tags.insert(tag_name, value);
        } 
      
        let camera_make = exif.get_field(exif::Tag::Make, exif::In::PRIMARY).map(|f| f.display_value().to_string());
      
        let camera_model = exif.get_field(exif::Tag::Model, exif::In::PRIMARY).map(|f| f.display_value().to_string());
      
        let software = exif.get_field(exif::Tag::Software, exif::In::PRIMARY).map(|f| f.display_value().to_string());
      
        let date_time = exif.get_field(exif::Tag::DateTime, exif::In::PRIMARY).map(|f| f.display_value().to_string());
     
        let gps_coordinates = Self::extract_gps(&exif);
      
        if let Some(ref sw) = software {
            let sw_lower = sw.to_lowercase();
            if sw_lower.contains("photoshop") ||
                sw_lower.contains("paint") ||
                sw_lower.contains("gimp") {
                    suspicious_indicators.push(format!("Edited with: {}", sw));
                }
        }  
        
        if exif.get_field(exif::Tag::DateTimeOriginal, exif::In::PRIMARY).is_none() {
            if date_time.is_some() {
                suspicious_indicators.push("Original datetime missing (may be stripped)".into());
            }
        }
        
        let datetime_original = exif.get_field(exif::Tag::DateTimeOriginal, exif::In::PRIMARY).map(|f| f.display_value().to_string());
        let datetime_digitized = exif.get_field(exif::Tag::DateTimeDigitized, exif::In::PRIMARY).map(|f| f.display_value().to_string());
        
        if let (Some(orig), Some(digi)) = (&datetime_original, &datetime_digitized) {
            if orig != digi {
                suspicious_indicators.push("Inconsistent date time values".into());
            }
        }
        
        Ok(MetadataResult { 
            camera_make, 
            camera_model, 
            software, 
            date_time, 
            gps_coordinates, 
            all_tags, 
            suspicious_indicators 
        })
    }
    
    fn extract_gps(exif: &exif::Exif) -> Option<(f64, f64)> {
        let lat = exif.get_field(exif::Tag::GPSLatitude, exif::In::PRIMARY)?;
        let lat_ref = exif.get_field(exif::Tag::GPSLatitudeRef, exif::In::PRIMARY)?;
        let lon = exif.get_field(exif::Tag::GPSLongitude, exif::In::PRIMARY)?;
        let lon_ref = exif.get_field(exif::Tag::GPSLongitudeRef, exif::In::PRIMARY)?;
        
        let lat_val = Self::parse_gps_coordinate(&lat.display_value().to_string())?;
        let lon_val = Self::parse_gps_coordinate(&lon.display_value().to_string())?;
        
        let lat_sign = if lat_ref.display_value().to_string().contains('S') { -1.0 } else { 1.0 };
        let lon_sign = if lon_ref.display_value().to_string().contains('W') { -1.0 } else { 1.0 };
        
        Some((lat_val * lat_sign, lon_val * lon_sign))
    }
    
    fn parse_gps_coordinate(s: &str) -> Option<f64> {        
        let parts = s.split_whitespace().collect::<Vec<_>>();
        
        if parts.len() >= 4 {
            let degrees = parts[0].parse::<f64>().ok()?;
            let minutes = parts[2].trim_end_matches('\'').parse::<f64>().ok()?;
            let seconds = parts[3].trim_end_matches('"').parse::<f64>().unwrap_or(0.0);
            
            Some(degrees + minutes / 60.0 + seconds / 3600.0)
        } else {
            None 
        }
    }
}