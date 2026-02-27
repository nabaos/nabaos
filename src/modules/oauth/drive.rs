//! Google Drive API v3 client.

use crate::core::error::{NyayaError, Result};
use serde::{Deserialize, Serialize};
use std::sync::{Mutex, OnceLock};
use std::time::{SystemTime, UNIX_EPOCH};

const DRIVE_API_BASE: &str = "https://www.googleapis.com/drive/v3";

/// Maximum Drive API calls per minute.
const RATE_LIMIT_MAX: u64 = 10;

/// Maximum file download size in bytes (50 MB).
const DOWNLOAD_MAX_BYTES: usize = 50 * 1024 * 1024;

// ---------------------------------------------------------------------------
// Rate limiter — sliding-window counter protected by a Mutex
// ---------------------------------------------------------------------------

/// Rate-limit state: (window_start_secs, call_count_in_window).
static RATE_LIMIT_STATE: OnceLock<Mutex<(u64, u64)>> = OnceLock::new();

fn rate_limit_state() -> &'static Mutex<(u64, u64)> {
    RATE_LIMIT_STATE.get_or_init(|| Mutex::new((0, 0)))
}

/// Check and increment the rate limiter. Returns Err if the limit is exceeded.
fn check_rate_limit() -> Result<()> {
    let now = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs();

    let mut state = rate_limit_state()
        .lock()
        .map_err(|_| NyayaError::Config("Drive rate limiter mutex poisoned".into()))?;

    let (window_start, count) = &mut *state;

    // If more than 60 seconds have elapsed, reset the window.
    if now.saturating_sub(*window_start) >= 60 {
        *window_start = now;
        *count = 1;
        return Ok(());
    }

    if *count >= RATE_LIMIT_MAX {
        Err(NyayaError::Config(format!(
            "Drive API rate limit exceeded: {} calls in current 60s window (max {})",
            count, RATE_LIMIT_MAX
        )))
    } else {
        *count += 1;
        Ok(())
    }
}

// ---------------------------------------------------------------------------
// Data types
// ---------------------------------------------------------------------------

/// A file entry returned by the Google Drive API.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DriveFile {
    pub id: String,
    pub name: String,
    pub mime_type: String,
    pub size: Option<u64>,
    pub modified_time: Option<String>,
    pub parents: Vec<String>,
}

// ---------------------------------------------------------------------------
// Google Drive client
// ---------------------------------------------------------------------------

/// Google Drive API v3 client for listing, reading, and exporting files.
pub struct GoogleDriveClient {
    access_token: String,
    client: reqwest::blocking::Client,
}

impl GoogleDriveClient {
    pub fn new(access_token: &str) -> Self {
        Self {
            access_token: access_token.to_string(),
            client: reqwest::blocking::Client::new(),
        }
    }

    /// List files matching an optional query string.
    ///
    /// GET https://www.googleapis.com/drive/v3/files
    ///
    /// Uses the `fields` parameter for partial response to reduce payload size.
    pub fn list_files(&self, query: Option<&str>, max_results: usize) -> Result<Vec<DriveFile>> {
        check_rate_limit()?;

        let page_size = max_results.min(1000);
        let mut url = format!(
            "{}/files?pageSize={}&fields=files(id,name,mimeType,size,modifiedTime,parents)",
            DRIVE_API_BASE, page_size
        );
        if let Some(q) = query {
            url.push_str(&format!("&q={}", urlencoding::encode(q)));
        }

        let resp = self
            .client
            .get(&url)
            .bearer_auth(&self.access_token)
            .send()
            .map_err(|e| NyayaError::Config(format!("Drive API request failed: {}", e)))?;

        let json: serde_json::Value = resp
            .json()
            .map_err(|e| NyayaError::Config(format!("Drive API parse error: {}", e)))?;

        let files = json
            .get("files")
            .and_then(|f| f.as_array())
            .ok_or_else(|| NyayaError::Config("No files array in Drive response".into()))?;

        files.iter().map(parse_drive_file).collect()
    }

    /// Get metadata for a single file by ID.
    ///
    /// GET https://www.googleapis.com/drive/v3/files/{fileId}
    pub fn get_file_metadata(&self, file_id: &str) -> Result<DriveFile> {
        if file_id.is_empty() || file_id.len() > 128 {
            return Err(NyayaError::Config(format!(
                "Invalid Drive file ID: '{}'",
                file_id
            )));
        }

        check_rate_limit()?;

        let url = format!(
            "{}/files/{}?fields=id,name,mimeType,size,modifiedTime,parents",
            DRIVE_API_BASE,
            urlencoding::encode(file_id)
        );

        let resp = self
            .client
            .get(&url)
            .bearer_auth(&self.access_token)
            .send()
            .map_err(|e| NyayaError::Config(format!("Drive API request failed: {}", e)))?;

        let json: serde_json::Value = resp
            .json()
            .map_err(|e| NyayaError::Config(format!("Drive API parse error: {}", e)))?;

        parse_drive_file(&json)
    }

    /// Download a file's content as raw bytes.
    ///
    /// GET https://www.googleapis.com/drive/v3/files/{fileId}?alt=media
    ///
    /// Maximum download size: 50 MB.
    pub fn download_file(&self, file_id: &str) -> Result<Vec<u8>> {
        if file_id.is_empty() || file_id.len() > 128 {
            return Err(NyayaError::Config(format!(
                "Invalid Drive file ID: '{}'",
                file_id
            )));
        }

        check_rate_limit()?;

        let url = format!(
            "{}/files/{}?alt=media",
            DRIVE_API_BASE,
            urlencoding::encode(file_id)
        );

        let resp = self
            .client
            .get(&url)
            .bearer_auth(&self.access_token)
            .send()
            .map_err(|e| NyayaError::Config(format!("Drive download failed: {}", e)))?;

        let bytes = resp
            .bytes()
            .map_err(|e| NyayaError::Config(format!("Drive download read error: {}", e)))?;

        if bytes.len() > DOWNLOAD_MAX_BYTES {
            return Err(NyayaError::Config(format!(
                "Drive file too large: {} bytes (max {} bytes)",
                bytes.len(),
                DOWNLOAD_MAX_BYTES
            )));
        }

        Ok(bytes.to_vec())
    }

    /// Export a Google Docs/Sheets/Slides file to the specified MIME type.
    ///
    /// GET https://www.googleapis.com/drive/v3/files/{fileId}/export?mimeType=...
    pub fn export_google_doc(&self, file_id: &str, export_mime: &str) -> Result<Vec<u8>> {
        if file_id.is_empty() || file_id.len() > 128 {
            return Err(NyayaError::Config(format!(
                "Invalid Drive file ID: '{}'",
                file_id
            )));
        }

        check_rate_limit()?;

        let url = format!(
            "{}/files/{}/export?mimeType={}",
            DRIVE_API_BASE,
            urlencoding::encode(file_id),
            urlencoding::encode(export_mime)
        );

        let resp = self
            .client
            .get(&url)
            .bearer_auth(&self.access_token)
            .send()
            .map_err(|e| NyayaError::Config(format!("Drive export failed: {}", e)))?;

        let bytes = resp
            .bytes()
            .map_err(|e| NyayaError::Config(format!("Drive export read error: {}", e)))?;

        Ok(bytes.to_vec())
    }
}

// ---------------------------------------------------------------------------
// Helpers
// ---------------------------------------------------------------------------

/// Parse a Drive file JSON object into a DriveFile struct.
fn parse_drive_file(val: &serde_json::Value) -> Result<DriveFile> {
    let id = val
        .get("id")
        .and_then(|v| v.as_str())
        .ok_or_else(|| NyayaError::Config("Drive file missing 'id' field".into()))?
        .to_string();

    let name = val
        .get("name")
        .and_then(|v| v.as_str())
        .ok_or_else(|| NyayaError::Config("Drive file missing 'name' field".into()))?
        .to_string();

    let mime_type = val
        .get("mimeType")
        .and_then(|v| v.as_str())
        .unwrap_or("application/octet-stream")
        .to_string();

    let size = val.get("size").and_then(|v| {
        v.as_u64()
            .or_else(|| v.as_str().and_then(|s| s.parse::<u64>().ok()))
    });

    let modified_time = val
        .get("modifiedTime")
        .and_then(|v| v.as_str())
        .map(String::from);

    let parents = val
        .get("parents")
        .and_then(|v| v.as_array())
        .map(|arr| {
            arr.iter()
                .filter_map(|v| v.as_str().map(String::from))
                .collect()
        })
        .unwrap_or_default();

    Ok(DriveFile {
        id,
        name,
        mime_type,
        size,
        modified_time,
        parents,
    })
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_parse_drive_file_valid() {
        let json = serde_json::json!({
            "id": "file123",
            "name": "document.pdf",
            "mimeType": "application/pdf",
            "size": "2048",
            "modifiedTime": "2026-02-25T10:00:00Z",
            "parents": ["folder1", "folder2"]
        });
        let file = parse_drive_file(&json).unwrap();
        assert_eq!(file.id, "file123");
        assert_eq!(file.name, "document.pdf");
        assert_eq!(file.mime_type, "application/pdf");
        assert_eq!(file.size, Some(2048));
        assert_eq!(file.modified_time.as_deref(), Some("2026-02-25T10:00:00Z"));
        assert_eq!(file.parents, vec!["folder1", "folder2"]);
    }

    #[test]
    fn test_parse_drive_file_minimal() {
        let json = serde_json::json!({
            "id": "file456",
            "name": "notes.txt"
        });
        let file = parse_drive_file(&json).unwrap();
        assert_eq!(file.id, "file456");
        assert_eq!(file.name, "notes.txt");
        assert_eq!(file.mime_type, "application/octet-stream");
        assert_eq!(file.size, None);
        assert_eq!(file.modified_time, None);
        assert!(file.parents.is_empty());
    }

    #[test]
    fn test_get_file_metadata_empty_id() {
        let client = GoogleDriveClient::new("fake_token");
        let result = client.get_file_metadata("");
        assert!(result.is_err());
        let err = format!("{}", result.unwrap_err());
        assert!(err.contains("Invalid Drive file ID"));
    }

    #[test]
    fn test_download_file_empty_id() {
        let client = GoogleDriveClient::new("fake_token");
        let result = client.download_file("");
        assert!(result.is_err());
        let err = format!("{}", result.unwrap_err());
        assert!(err.contains("Invalid Drive file ID"));
    }

    #[test]
    fn test_export_google_doc_empty_id() {
        let client = GoogleDriveClient::new("fake_token");
        let result = client.export_google_doc("", "text/plain");
        assert!(result.is_err());
        let err = format!("{}", result.unwrap_err());
        assert!(err.contains("Invalid Drive file ID"));
    }
}
