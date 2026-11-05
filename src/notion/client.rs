use anyhow::{Context, Result, anyhow, bail};
use reqwest::{Method, blocking::Client};
use serde_json::{Value, json};
use std::time::Duration;

use crate::config::Settings;
use crate::secrets::Database;

const NOTION_API_BASE: &str = "https://api.notion.com/v1";
const NOTION_API_VERSION: &str = "2025-09-03";

pub struct NotionClient {
    client: Client,
    token: String,
    data_source_id: String,
}

impl NotionClient {
    pub fn new(database: &Database) -> Result<Self> {
        let settings = Settings::load()?;

        let token = database
            .get_notion_api_key()
            .context("Failed to retrieve Notion API key from SQLite database")?
            .ok_or_else(|| {
                anyhow!("Notion API key not found in database. Run the app to store it first.")
            })?;

        let client = Client::builder()
            .connect_timeout(Duration::from_secs(10))
            .timeout(Duration::from_secs(20))
            .build()
            .context("Failed to initialize HTTP client")?;

        Ok(Self {
            client,
            token,
            data_source_id: settings.notion_data_source_id,
        })
    }

    fn send_json(&self, method: Method, url: &str, payload: &Value) -> Result<Value> {
        let response = self
            .client
            .request(method, url)
            .bearer_auth(&self.token)
            .header("Notion-Version", NOTION_API_VERSION)
            .json(payload)
            .send()
            .context("Failed to send request to Notion")?;

        let status = response.status();

        let body: Value = response
            .json()
            .context("Failed to parse Notion API response")?;

        if !status.is_success() {
            let code = body
                .get("code")
                .and_then(Value::as_str)
                .unwrap_or("unknown");

            let message = body
                .get("message")
                .and_then(Value::as_str)
                .unwrap_or("No error details provided");

            bail!("Notion API error ({code}): {message}");
        }

        Ok(body)
    }

    fn post_json(&self, url: &str, payload: &Value) -> Result<Value> {
        self.send_json(Method::POST, url, payload)
    }

    pub fn query_data_source(&self, start_cursor: Option<&str>) -> Result<Value> {
        let url = format!(
            "{}/data_sources/{}/query",
            NOTION_API_BASE, self.data_source_id
        );

        let mut payload = serde_json::json!({
            "page_size": 100
        });

        if let Some(cursor) = start_cursor {
            payload["start_cursor"] = serde_json::json!(cursor);
        }

        self.post_json(&url, &payload)
    }

    pub fn create_page(&self, properties: &Value) -> Result<Value> {
        let url = format!("{}/pages", NOTION_API_BASE);
        let payload = self.page_payload(properties);
        self.post_json(&url, &payload)
    }

    pub fn update_page(&self, page_id: &str, properties: &Value) -> Result<Value> {
        let url = format!("{}/pages/{}", NOTION_API_BASE, page_id);
        self.send_json(Method::PATCH, &url, &update_payload(properties))
    }

    pub fn archive_page(&self, page_id: &str) -> Result<Value> {
        let url = format!("{}/pages/{}", NOTION_API_BASE, page_id);
        self.send_json(Method::PATCH, &url, &archive_payload())
    }

    fn page_payload(&self, properties: &Value) -> Value {
        serde_json::json!({
            "parent": {
                "type": "data_source_id",
                "data_source_id": self.data_source_id
            },
            "properties": properties
        })
    }
}

fn update_payload(properties: &Value) -> Value {
    json!({ "properties": properties })
}

fn archive_payload() -> Value {
    json!({ "archived": true })
}

#[cfg(test)]
mod tests {
    use super::*;

    const DATA_SOURCE_ID: &str = "data-source-123";

    fn client() -> NotionClient {
        NotionClient {
            client: Client::new(),
            token: "test-token".to_string(),
            data_source_id: DATA_SOURCE_ID.to_string(),
        }
    }

    fn properties() -> Value {
        serde_json::json!({
            "Name": {
                "title": [{ "type": "text", "text": { "content": "Test Credential" } }]
            },
            "Credential Reference": {
                "rich_text": [{
                    "type": "text",
                    "text": { "content": "op://Developer Credentials/item/credential" }
                }]
            }
        })
    }

    #[test]
    fn page_payload_uses_the_data_source_parent_without_secrets() {
        let secret = "sk-test-value-that-must-only-live-in-1password";
        let properties = properties();
        let payload = client().page_payload(&properties);
        let serialized = payload.to_string();

        assert_eq!(
            payload["parent"],
            serde_json::json!({
                "type": "data_source_id",
                "data_source_id": DATA_SOURCE_ID
            })
        );
        assert!(payload["parent"].get("database_id").is_none());
        assert!(payload.get("database_id").is_none());
        assert!(!serialized.contains("database_id"));
        assert!(!serialized.contains(secret));
        assert_eq!(payload.as_object().map(|object| object.len()), Some(2));
        assert_eq!(payload["properties"], properties);
    }

    #[test]
    fn update_payload_sends_only_properties() {
        let properties = properties();
        let payload = update_payload(&properties);

        assert_eq!(payload, serde_json::json!({ "properties": properties }));
        assert!(payload.get("archived").is_none());
        assert_eq!(payload.as_object().map(|object| object.len()), Some(1));
    }

    #[test]
    fn archive_payload_trashes_the_page() {
        assert_eq!(archive_payload(), serde_json::json!({ "archived": true }));
    }
}
