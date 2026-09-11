use exif::{In, Reader, Tag, Value};
use reverse_geocoder::ReverseGeocoder;
use std::fs::File;
use std::io::BufReader;
use std::path::Path;

#[derive(Default, Debug)]
pub struct ExtractedMetadata {
    pub captured_at: Option<String>,
    pub year: Option<i32>,
    pub month: Option<i32>,
    pub day: Option<i32>,
    pub hour: Option<i32>,
    pub latitude: Option<f64>,
    pub longitude: Option<f64>,
    pub altitude: Option<f64>,
    pub city: Option<String>,
    pub subdivision: Option<String>,
    pub country: Option<String>,
    pub country_code: Option<String>,
    pub camera_make: Option<String>,
    pub camera_model: Option<String>,
}

pub struct MetadataService;

impl MetadataService {
    pub fn extract(path: &Path) -> ExtractedMetadata {
        let mut meta = ExtractedMetadata::default();

        let file = match File::open(path) {
            Ok(f) => f,
            Err(_) => return meta,
        };

        let mut bufreader = BufReader::new(file);
        let reader = Reader::new();
        let exif_data = match reader.read_from_container(&mut bufreader) {
            Ok(e) => e,
            Err(_) => return meta,
        };

        // 1. Camera Make & Model
        if let Some(field) = exif_data.get_field(Tag::Make, In::PRIMARY) {
            meta.camera_make = Some(field.display_value().to_string().trim_matches('"').to_string());
        }
        if let Some(field) = exif_data.get_field(Tag::Model, In::PRIMARY) {
            meta.camera_model = Some(field.display_value().to_string().trim_matches('"').to_string());
        }

        // 2. Timestamp Breakdown
        if let Some(field) = exif_data.get_field(Tag::DateTimeOriginal, In::PRIMARY) {
            let dt_str = field.display_value().to_string();
            meta.captured_at = Some(dt_str.clone());

            let parts: Vec<&str> = dt_str.split(|c| c == '-' || c == ' ' || c == ':').collect();
            if parts.len() >= 4 {
                meta.year = parts[0].parse().ok();
                meta.month = parts[1].parse().ok();
                meta.day = parts[2].parse().ok();
                meta.hour = parts[3].parse().ok();
            }
        }

        // 3. Altitude
        if let Some(alt_field) = exif_data.get_field(Tag::GPSAltitude, In::PRIMARY) {
            if let Value::Rational(ref rats) = alt_field.value {
                if let Some(rat) = rats.first() {
                    meta.altitude = Some(rat.to_f64());
                }
            }
        }

        // 4. GPS Coordinates & Offline Reverse Geocoding
        let lat_ref = exif_data.get_field(Tag::GPSLatitudeRef, In::PRIMARY);
        let lat = exif_data.get_field(Tag::GPSLatitude, In::PRIMARY);
        let lon_ref = exif_data.get_field(Tag::GPSLongitudeRef, In::PRIMARY);
        let lon = exif_data.get_field(Tag::GPSLongitude, In::PRIMARY);

        if let (Some(lat), Some(lat_ref), Some(lon), Some(lon_ref)) = (lat, lat_ref, lon, lon_ref) {
            if let (Some(parsed_lat), Some(parsed_lon)) = (
                Self::parse_gps_coord(&lat.value, &lat_ref.display_value().to_string()),
                Self::parse_gps_coord(&lon.value, &lon_ref.display_value().to_string()),
            ) {
                meta.latitude = Some(parsed_lat);
                meta.longitude = Some(parsed_lon);

                let geocoder = ReverseGeocoder::new();
                let search_result = geocoder.search((parsed_lat, parsed_lon));
                meta.city = Some(search_result.record.name.to_string());
                meta.subdivision = Some(search_result.record.admin1.to_string());
                meta.country_code = Some(search_result.record.cc.to_string());
            }
        }

        meta
    }

    fn parse_gps_coord(val: &Value, ref_val: &str) -> Option<f64> {
        if let Value::Rational(ref rats) = *val {
            if rats.len() >= 3 {
                let deg = rats[0].to_f64();
                let min = rats[1].to_f64();
                let sec = rats[2].to_f64();
                let mut coord = deg + (min / 60.0) + (sec / 3600.0);
                if ref_val.contains('S') || ref_val.contains('W') {
                    coord = -coord;
                }
                return Some(coord);
            }
        }
        None
    }
}