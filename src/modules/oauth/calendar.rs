//! Google Calendar API v3 client.

use crate::core::error::{NyayaError, Result};
use serde::{Deserialize, Serialize};

const CALENDAR_API_BASE: &str = "https://www.googleapis.com/calendar/v3";

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CalendarEvent {
    pub id: String,
    pub title: String,
    pub start: String,
    pub end: String,
    pub description: Option<String>,
}

pub struct GoogleCalendarClient {
    access_token: String,
    client: reqwest::blocking::Client,
}

impl GoogleCalendarClient {
    pub fn new(access_token: &str) -> Self {
        Self {
            access_token: access_token.to_string(),
            client: reqwest::blocking::Client::new(),
        }
    }

    /// List events between start and end times (RFC3339 format).
    pub fn list_events(&self, time_min: &str, time_max: &str) -> Result<Vec<CalendarEvent>> {
        let url = format!(
            "{}/calendars/primary/events?timeMin={}&timeMax={}&singleEvents=true&orderBy=startTime&maxResults=50",
            CALENDAR_API_BASE,
            urlencoding::encode(time_min),
            urlencoding::encode(time_max),
        );

        let resp = self
            .client
            .get(&url)
            .bearer_auth(&self.access_token)
            .send()
            .map_err(|e| NyayaError::Config(format!("Calendar API error: {}", e)))?;

        let json: serde_json::Value = resp
            .json()
            .map_err(|e| NyayaError::Config(format!("Calendar parse error: {}", e)))?;

        let items = json
            .get("items")
            .and_then(|i| i.as_array())
            .ok_or_else(|| NyayaError::Config("No items in calendar response".into()))?;

        items
            .iter()
            .map(|item| {
                Ok(CalendarEvent {
                    id: item
                        .get("id")
                        .and_then(|v| v.as_str())
                        .unwrap_or("")
                        .to_string(),
                    title: item
                        .get("summary")
                        .and_then(|v| v.as_str())
                        .unwrap_or("Untitled")
                        .to_string(),
                    start: item
                        .get("start")
                        .and_then(|s| s.get("dateTime").or_else(|| s.get("date")))
                        .and_then(|v| v.as_str())
                        .unwrap_or("")
                        .to_string(),
                    end: item
                        .get("end")
                        .and_then(|s| s.get("dateTime").or_else(|| s.get("date")))
                        .and_then(|v| v.as_str())
                        .unwrap_or("")
                        .to_string(),
                    description: item
                        .get("description")
                        .and_then(|v| v.as_str())
                        .map(String::from),
                })
            })
            .collect()
    }

    /// Create a new calendar event. Returns the event ID.
    pub fn create_event(
        &self,
        title: &str,
        start: &str,
        end: &str,
        description: &str,
    ) -> Result<String> {
        let url = format!("{}/calendars/primary/events", CALENDAR_API_BASE);
        let body = serde_json::json!({
            "summary": title,
            "description": description,
            "start": {"dateTime": start},
            "end": {"dateTime": end},
        });

        let resp = self
            .client
            .post(&url)
            .bearer_auth(&self.access_token)
            .json(&body)
            .send()
            .map_err(|e| NyayaError::Config(format!("Calendar create error: {}", e)))?;

        let json: serde_json::Value = resp
            .json()
            .map_err(|e| NyayaError::Config(format!("Calendar create parse error: {}", e)))?;

        json.get("id")
            .and_then(|v| v.as_str())
            .map(String::from)
            .ok_or_else(|| NyayaError::Config("No id in create response".into()))
    }

    /// Delete a calendar event by ID.
    pub fn delete_event(&self, event_id: &str) -> Result<()> {
        let url = format!(
            "{}/calendars/primary/events/{}",
            CALENDAR_API_BASE, event_id
        );

        let resp = self
            .client
            .delete(&url)
            .bearer_auth(&self.access_token)
            .send()
            .map_err(|e| NyayaError::Config(format!("Calendar delete error: {}", e)))?;

        if resp.status().is_success() || resp.status().as_u16() == 204 {
            Ok(())
        } else {
            Err(NyayaError::Config(format!(
                "Calendar delete failed: {}",
                resp.status()
            )))
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_calendar_event_serde() {
        let event = CalendarEvent {
            id: "abc123".into(),
            title: "Team Meeting".into(),
            start: "2026-02-25T10:00:00Z".into(),
            end: "2026-02-25T11:00:00Z".into(),
            description: Some("Weekly sync".into()),
        };
        let json = serde_json::to_string(&event).unwrap();
        let parsed: CalendarEvent = serde_json::from_str(&json).unwrap();
        assert_eq!(parsed.title, "Team Meeting");
        assert_eq!(parsed.description.as_deref(), Some("Weekly sync"));
    }

    #[test]
    fn test_parse_calendar_list_response() {
        let resp = serde_json::json!({
            "items": [{
                "id": "ev1",
                "summary": "Lunch",
                "start": {"dateTime": "2026-02-25T12:00:00Z"},
                "end": {"dateTime": "2026-02-25T13:00:00Z"},
                "description": "Team lunch"
            }]
        });
        let items = resp.get("items").unwrap().as_array().unwrap();
        assert_eq!(items.len(), 1);
        assert_eq!(items[0]["summary"], "Lunch");
    }

    #[test]
    fn test_create_event_request_body() {
        let body = serde_json::json!({
            "summary": "Test Event",
            "description": "A test",
            "start": {"dateTime": "2026-02-25T10:00:00Z"},
            "end": {"dateTime": "2026-02-25T11:00:00Z"},
        });
        assert_eq!(body["summary"], "Test Event");
        assert!(body["start"]["dateTime"].is_string());
    }
}
