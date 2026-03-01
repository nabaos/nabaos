//! Multi-carrier parcel tracking module.
//!
//! Provides carrier detection from tracking number format, tracking status
//! lookup via 17track/AfterShip API (when API key is configured), and
//! structured fallback when no API key is present.
//!
//! Supported carriers (auto-detected):
//! - UPS: starts with "1Z"
//! - FedEx: 12 or 15 digits
//! - USPS: starts with "94", 20-22 digits
//! - DHL: 10 digits or starts with "JJD"
//! - Generic/unknown for all others

use serde::{Deserialize, Serialize};
use std::fmt;

// ---------------------------------------------------------------------------
// TrackingStatus enum
// ---------------------------------------------------------------------------

/// Parcel tracking status.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub enum TrackingStatus {
    Pending,
    InTransit,
    OutForDelivery,
    Delivered,
    Exception,
    Returned,
    Unknown,
}

impl fmt::Display for TrackingStatus {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            TrackingStatus::Pending => write!(f, "Pending"),
            TrackingStatus::InTransit => write!(f, "InTransit"),
            TrackingStatus::OutForDelivery => write!(f, "OutForDelivery"),
            TrackingStatus::Delivered => write!(f, "Delivered"),
            TrackingStatus::Exception => write!(f, "Exception"),
            TrackingStatus::Returned => write!(f, "Returned"),
            TrackingStatus::Unknown => write!(f, "Unknown"),
        }
    }
}

impl TrackingStatus {
    /// Parse a status string (case-insensitive) into a TrackingStatus.
    pub fn from_str_loose(s: &str) -> Self {
        match s.to_lowercase().as_str() {
            "pending" | "pre_transit" | "info_received" => TrackingStatus::Pending,
            "intransit" | "in_transit" | "transit" => TrackingStatus::InTransit,
            "outfordelivery" | "out_for_delivery" => TrackingStatus::OutForDelivery,
            "delivered" => TrackingStatus::Delivered,
            "exception" | "failed_attempt" | "alert" => TrackingStatus::Exception,
            "returned" | "return_to_sender" => TrackingStatus::Returned,
            _ => TrackingStatus::Unknown,
        }
    }
}

// ---------------------------------------------------------------------------
// TrackingResult
// ---------------------------------------------------------------------------

/// Result of a tracking lookup.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TrackingResult {
    /// Detected or specified carrier name.
    pub carrier: String,
    /// The tracking ID that was looked up.
    pub tracking_id: String,
    /// Current tracking status.
    pub status: TrackingStatus,
    /// Last known location (if available).
    pub last_location: Option<String>,
    /// Last status update timestamp (ISO 8601, if available).
    pub last_update: Option<String>,
    /// Estimated delivery date (ISO 8601, if available).
    pub estimated_delivery: Option<String>,
    /// Raw API response (if an API was called).
    pub raw_response: Option<String>,
}

// ---------------------------------------------------------------------------
// CarrierDetector
// ---------------------------------------------------------------------------

/// Detected carrier from tracking number format.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub enum DetectedCarrier {
    UPS,
    FedEx,
    USPS,
    DHL,
    Generic,
}

impl fmt::Display for DetectedCarrier {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            DetectedCarrier::UPS => write!(f, "UPS"),
            DetectedCarrier::FedEx => write!(f, "FedEx"),
            DetectedCarrier::USPS => write!(f, "USPS"),
            DetectedCarrier::DHL => write!(f, "DHL"),
            DetectedCarrier::Generic => write!(f, "Generic"),
        }
    }
}

/// Detect the carrier from a tracking number's format.
pub struct CarrierDetector;

impl CarrierDetector {
    /// Detect carrier based on tracking number format heuristics.
    ///
    /// Rules:
    /// - UPS: starts with "1Z" (case-insensitive)
    /// - FedEx: exactly 12 or 15 digits
    /// - USPS: starts with "94" and is 20-22 digits
    /// - DHL: exactly 10 digits, or starts with "JJD" (case-insensitive)
    /// - Generic: anything else
    pub fn detect(tracking_id: &str) -> DetectedCarrier {
        let trimmed = tracking_id.trim();
        let upper = trimmed.to_uppercase();

        // UPS: starts with "1Z"
        if upper.starts_with("1Z") {
            return DetectedCarrier::UPS;
        }

        // DHL: starts with "JJD"
        if upper.starts_with("JJD") {
            return DetectedCarrier::DHL;
        }

        // Check if all-digit for remaining checks
        let all_digits = trimmed.chars().all(|c| c.is_ascii_digit());

        if all_digits {
            let len = trimmed.len();

            // USPS: starts with "94", 20-22 digits
            if trimmed.starts_with("94") && (20..=22).contains(&len) {
                return DetectedCarrier::USPS;
            }

            // FedEx: exactly 12 or 15 digits
            if len == 12 || len == 15 {
                return DetectedCarrier::FedEx;
            }

            // DHL: exactly 10 digits
            if len == 10 {
                return DetectedCarrier::DHL;
            }
        }

        DetectedCarrier::Generic
    }

    /// Return the carrier name as a string, preferring a user-supplied carrier
    /// name over auto-detection.
    pub fn carrier_name(tracking_id: &str, carrier_override: Option<&str>) -> String {
        if let Some(c) = carrier_override {
            if !c.is_empty() {
                return c.to_string();
            }
        }
        Self::detect(tracking_id).to_string()
    }
}

// ---------------------------------------------------------------------------
// Tracking ID validation
// ---------------------------------------------------------------------------

/// Validate a tracking ID: alphanumeric + hyphens, 5-40 characters.
pub fn validate_tracking_id(tracking_id: &str) -> Result<(), String> {
    let trimmed = tracking_id.trim();
    if trimmed.is_empty() {
        return Err("Tracking ID is empty".to_string());
    }
    if trimmed.len() < 5 {
        return Err(format!(
            "Tracking ID '{}' is too short ({} chars, minimum 5)",
            trimmed,
            trimmed.len()
        ));
    }
    if trimmed.len() > 40 {
        return Err(format!(
            "Tracking ID '{}' is too long ({} chars, maximum 40)",
            trimmed,
            trimmed.len()
        ));
    }
    if !trimmed
        .chars()
        .all(|c| c.is_ascii_alphanumeric() || c == '-')
    {
        return Err(format!(
            "Tracking ID '{}' contains invalid characters (allowed: alphanumeric + hyphens)",
            trimmed
        ));
    }
    Ok(())
}

// ---------------------------------------------------------------------------
// check_tracking — main lookup function
// ---------------------------------------------------------------------------

/// Check tracking status for a parcel.
///
/// If `api_key` is provided (or `NABA_TRACKING_API_KEY` env is set), attempts
/// to call the 17track API. Otherwise, returns a structured "check manually"
/// result with the detected carrier and a link.
pub fn check_tracking(
    tracking_id: &str,
    carrier: Option<&str>,
    api_key: Option<&str>,
) -> Result<TrackingResult, String> {
    let trimmed = tracking_id.trim();
    validate_tracking_id(trimmed)?;

    let carrier_name = CarrierDetector::carrier_name(trimmed, carrier);

    // Resolve API key: explicit parameter > environment variable
    let resolved_key = api_key
        .map(|k| k.to_string())
        .or_else(|| std::env::var("NABA_TRACKING_API_KEY").ok());

    if let Some(key) = resolved_key {
        if !key.is_empty() {
            return call_tracking_api(trimmed, &carrier_name, &key);
        }
    }

    // No API key — return a manual-check result with carrier link
    Ok(TrackingResult {
        carrier: carrier_name.clone(),
        tracking_id: trimmed.to_string(),
        status: TrackingStatus::Unknown,
        last_location: None,
        last_update: None,
        estimated_delivery: None,
        raw_response: Some(format!(
            "No tracking API key configured. Check manually at: {}",
            manual_tracking_url(&carrier_name, trimmed)
        )),
    })
}

/// Build a manual tracking URL for the given carrier.
fn manual_tracking_url(carrier: &str, tracking_id: &str) -> String {
    match carrier.to_uppercase().as_str() {
        "UPS" => format!("https://www.ups.com/track?tracknum={}", tracking_id),
        "FEDEX" => format!("https://www.fedex.com/fedextrack/?trknbr={}", tracking_id),
        "USPS" => format!(
            "https://tools.usps.com/go/TrackConfirmAction?tLabels={}",
            tracking_id
        ),
        "DHL" => format!(
            "https://www.dhl.com/en/express/tracking.html?AWB={}",
            tracking_id
        ),
        _ => format!("https://www.17track.net/en/track#nums={}", tracking_id),
    }
}

/// Call the 17track register+track API.
///
/// 17track API v2:
///   POST https://api.17track.net/track/v2.2/register
///   POST https://api.17track.net/track/v2.2/gettrackinfo
///
/// We use a blocking reqwest client since host functions are synchronous.
fn call_tracking_api(
    tracking_id: &str,
    carrier_name: &str,
    api_key: &str,
) -> Result<TrackingResult, String> {
    let client = reqwest::blocking::Client::builder()
        .timeout(std::time::Duration::from_secs(15))
        .build()
        .map_err(|e| format!("Failed to build HTTP client: {}", e))?;

    // Step 1: Register the tracking number
    let register_body = serde_json::json!([{
        "number": tracking_id,
        "carrier": carrier_code_17track(carrier_name),
    }]);

    let _register_resp = client
        .post("https://api.17track.net/track/v2.2/register")
        .header("17token", api_key)
        .header("Content-Type", "application/json")
        .json(&register_body)
        .send()
        .map_err(|e| format!("17track register request failed: {}", e))?;

    // Step 2: Get tracking info
    let track_body = serde_json::json!([{
        "number": tracking_id,
        "carrier": carrier_code_17track(carrier_name),
    }]);

    let resp = client
        .post("https://api.17track.net/track/v2.2/gettrackinfo")
        .header("17token", api_key)
        .header("Content-Type", "application/json")
        .json(&track_body)
        .send()
        .map_err(|e| format!("17track gettrackinfo request failed: {}", e))?;

    let status_code = resp.status();
    let body_text = resp
        .text()
        .map_err(|e| format!("Failed to read 17track response: {}", e))?;

    if !status_code.is_success() {
        return Err(format!(
            "17track API returned HTTP {}: {}",
            status_code, body_text
        ));
    }

    // Parse the response
    let json: serde_json::Value = serde_json::from_str(&body_text)
        .map_err(|e| format!("Failed to parse 17track response: {}", e))?;

    // Extract tracking data from 17track response structure
    let accepted = json
        .get("data")
        .and_then(|d| d.get("accepted"))
        .and_then(|a| a.as_array())
        .and_then(|arr| arr.first());

    if let Some(track_data) = accepted {
        let track_info = track_data.get("track_info").unwrap_or(track_data);

        let latest_event = track_info.get("latest_event").or_else(|| {
            track_info
                .get("tracking")
                .and_then(|t| t.get("providers"))
                .and_then(|p| p.as_array())
                .and_then(|arr| arr.first())
                .and_then(|prov| prov.get("events"))
                .and_then(|e| e.as_array())
                .and_then(|arr| arr.first())
        });

        let status_str = track_info
            .get("latest_status")
            .and_then(|s| s.get("status"))
            .and_then(|s| s.as_str())
            .unwrap_or("Unknown");

        let last_location = latest_event
            .and_then(|e| e.get("location"))
            .and_then(|l| l.as_str())
            .map(|s| s.to_string());

        let last_update = latest_event
            .and_then(|e| e.get("time_iso"))
            .or_else(|| latest_event.and_then(|e| e.get("time")))
            .and_then(|t| t.as_str())
            .map(|s| s.to_string());

        let estimated_delivery = track_info
            .get("time_metrics")
            .and_then(|tm| tm.get("estimated_delivery_date"))
            .and_then(|d| {
                if d.is_null() {
                    None
                } else {
                    d.as_str().map(|s| s.to_string())
                }
            });

        Ok(TrackingResult {
            carrier: carrier_name.to_string(),
            tracking_id: tracking_id.to_string(),
            status: TrackingStatus::from_str_loose(status_str),
            last_location,
            last_update,
            estimated_delivery,
            raw_response: Some(body_text),
        })
    } else {
        // No accepted tracking data — possibly not yet registered or invalid
        Ok(TrackingResult {
            carrier: carrier_name.to_string(),
            tracking_id: tracking_id.to_string(),
            status: TrackingStatus::Unknown,
            last_location: None,
            last_update: None,
            estimated_delivery: None,
            raw_response: Some(body_text),
        })
    }
}

/// Map carrier name to 17track carrier code.
/// Returns 0 for auto-detect.
fn carrier_code_17track(carrier_name: &str) -> i32 {
    match carrier_name.to_uppercase().as_str() {
        "UPS" => 100002,
        "FEDEX" => 100003,
        "USPS" => 100001,
        "DHL" => 100004,
        _ => 0, // auto-detect
    }
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    // --- CarrierDetector tests ---

    #[test]
    fn test_detect_ups() {
        assert_eq!(
            CarrierDetector::detect("1Z12345E0205271688"),
            DetectedCarrier::UPS
        );
        assert_eq!(
            CarrierDetector::detect("1z12345E0205271688"),
            DetectedCarrier::UPS
        );
    }

    #[test]
    fn test_detect_fedex_12() {
        assert_eq!(
            CarrierDetector::detect("123456789012"),
            DetectedCarrier::FedEx
        );
    }

    #[test]
    fn test_detect_fedex_15() {
        assert_eq!(
            CarrierDetector::detect("123456789012345"),
            DetectedCarrier::FedEx
        );
    }

    #[test]
    fn test_detect_usps() {
        assert_eq!(
            CarrierDetector::detect("94001234567890123456"),
            DetectedCarrier::USPS
        );
        assert_eq!(
            CarrierDetector::detect("9400123456789012345678"),
            DetectedCarrier::USPS
        );
    }

    #[test]
    fn test_detect_dhl_10_digits() {
        assert_eq!(CarrierDetector::detect("1234567890"), DetectedCarrier::DHL);
    }

    #[test]
    fn test_detect_dhl_jjd() {
        assert_eq!(
            CarrierDetector::detect("JJD000123456789"),
            DetectedCarrier::DHL
        );
        assert_eq!(
            CarrierDetector::detect("jjd000123456789"),
            DetectedCarrier::DHL
        );
    }

    #[test]
    fn test_detect_generic() {
        assert_eq!(
            CarrierDetector::detect("ABCDE12345"),
            DetectedCarrier::Generic
        );
        assert_eq!(
            CarrierDetector::detect("MY-PARCEL-123"),
            DetectedCarrier::Generic
        );
    }

    // --- Tracking ID validation tests ---

    #[test]
    fn test_valid_tracking_id() {
        assert!(validate_tracking_id("1Z12345E0205271688").is_ok());
        assert!(validate_tracking_id("ABCDE").is_ok());
        assert!(validate_tracking_id("123-456-789").is_ok());
    }

    #[test]
    fn test_tracking_id_too_short() {
        assert!(validate_tracking_id("ABC").is_err());
        assert!(validate_tracking_id("1234").is_err());
    }

    #[test]
    fn test_tracking_id_too_long() {
        let long_id = "A".repeat(41);
        assert!(validate_tracking_id(&long_id).is_err());
    }

    #[test]
    fn test_tracking_id_empty() {
        assert!(validate_tracking_id("").is_err());
        assert!(validate_tracking_id("   ").is_err());
    }

    #[test]
    fn test_tracking_id_invalid_chars() {
        assert!(validate_tracking_id("ABC@123").is_err());
        assert!(validate_tracking_id("ABC 123").is_err());
        assert!(validate_tracking_id("ABC!123").is_err());
    }

    // --- TrackingStatus tests ---

    #[test]
    fn test_status_display() {
        assert_eq!(TrackingStatus::Pending.to_string(), "Pending");
        assert_eq!(TrackingStatus::InTransit.to_string(), "InTransit");
        assert_eq!(TrackingStatus::OutForDelivery.to_string(), "OutForDelivery");
        assert_eq!(TrackingStatus::Delivered.to_string(), "Delivered");
        assert_eq!(TrackingStatus::Exception.to_string(), "Exception");
        assert_eq!(TrackingStatus::Returned.to_string(), "Returned");
        assert_eq!(TrackingStatus::Unknown.to_string(), "Unknown");
    }

    #[test]
    fn test_status_from_str_loose() {
        assert_eq!(
            TrackingStatus::from_str_loose("pending"),
            TrackingStatus::Pending
        );
        assert_eq!(
            TrackingStatus::from_str_loose("Pending"),
            TrackingStatus::Pending
        );
        assert_eq!(
            TrackingStatus::from_str_loose("in_transit"),
            TrackingStatus::InTransit
        );
        assert_eq!(
            TrackingStatus::from_str_loose("delivered"),
            TrackingStatus::Delivered
        );
        assert_eq!(
            TrackingStatus::from_str_loose("out_for_delivery"),
            TrackingStatus::OutForDelivery
        );
        assert_eq!(
            TrackingStatus::from_str_loose("exception"),
            TrackingStatus::Exception
        );
        assert_eq!(
            TrackingStatus::from_str_loose("returned"),
            TrackingStatus::Returned
        );
        assert_eq!(
            TrackingStatus::from_str_loose("gibberish"),
            TrackingStatus::Unknown
        );
    }

    #[test]
    fn test_status_serialize_deserialize() {
        let status = TrackingStatus::InTransit;
        let json = serde_json::to_string(&status).unwrap();
        assert_eq!(json, "\"InTransit\"");
        let back: TrackingStatus = serde_json::from_str(&json).unwrap();
        assert_eq!(back, TrackingStatus::InTransit);
    }

    // --- check_tracking tests (no API key) ---

    #[test]
    fn test_check_tracking_no_api_key() {
        // Temporarily unset env var if present
        let prev = std::env::var("NABA_TRACKING_API_KEY").ok();
        unsafe { std::env::remove_var("NABA_TRACKING_API_KEY"); }

        let result = check_tracking("1Z12345E0205271688", None, None).unwrap();
        assert_eq!(result.carrier, "UPS");
        assert_eq!(result.tracking_id, "1Z12345E0205271688");
        assert_eq!(result.status, TrackingStatus::Unknown);
        assert!(result.raw_response.as_ref().unwrap().contains("ups.com"));

        // Restore env var
        if let Some(val) = prev {
            unsafe { std::env::set_var("NABA_TRACKING_API_KEY", val); }
        }
    }

    #[test]
    fn test_check_tracking_with_carrier_override() {
        let prev = std::env::var("NABA_TRACKING_API_KEY").ok();
        unsafe { std::env::remove_var("NABA_TRACKING_API_KEY"); }

        let result = check_tracking("ABCDE12345", Some("DHL"), None).unwrap();
        assert_eq!(result.carrier, "DHL");
        assert!(result.raw_response.as_ref().unwrap().contains("dhl.com"));

        if let Some(val) = prev {
            unsafe { std::env::set_var("NABA_TRACKING_API_KEY", val); }
        }
    }

    #[test]
    fn test_check_tracking_invalid_id() {
        assert!(check_tracking("AB", None, None).is_err());
        assert!(check_tracking("", None, None).is_err());
    }

    // --- CarrierDetector::carrier_name tests ---

    #[test]
    fn test_carrier_name_with_override() {
        assert_eq!(
            CarrierDetector::carrier_name("1Z123", Some("MyCarrier")),
            "MyCarrier"
        );
    }

    #[test]
    fn test_carrier_name_auto_detect() {
        assert_eq!(
            CarrierDetector::carrier_name("1Z12345E0205271688", None),
            "UPS"
        );
    }

    // --- manual_tracking_url tests ---

    #[test]
    fn test_manual_tracking_urls() {
        assert!(manual_tracking_url("UPS", "1Z123").contains("ups.com"));
        assert!(manual_tracking_url("FedEx", "123").contains("fedex.com"));
        assert!(manual_tracking_url("USPS", "940").contains("usps.com"));
        assert!(manual_tracking_url("DHL", "123").contains("dhl.com"));
        assert!(manual_tracking_url("Other", "123").contains("17track.net"));
    }
}
