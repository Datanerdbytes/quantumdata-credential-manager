use anyhow::{Context, Result, bail};
use reqwest::blocking::Client;
use serde_json::Value;
use std::process::Command;
use std::time::Duration;

use crate::config::Settings;

const NOTION_API_BASE: &str = "https://api.notion.com/v1";
const NOTION_API_VERSION: &str = "2025-09-03";

pub struct NotionClient {
    client: Client,
    token: String,
    data_source_id: String,
}

impl NotionClient {
    pub fn new() -> Result<Self> {
        let settings = Settings::load()?;

        let output = Command::new("op")
            .args(["read", settings.notion_api_key_op_ref.as_str()])
            .output()
            .context("Failed to execute the 1Password CLI")?;

        if !output.status.success() {
            let stderr = String::from_utf8_lossy(&output.stderr);
            let detail = stderr.trim();
            bail!("1Password could not retrieve the Notion API credential: {detail}");
        }

        let token = String::from_utf8(output.stdout)
            .context("1Password returned an invalid credential format")?
            .trim()
            .to_string();

        if token.is_empty() {
            bail!("The Notion API credential is empty");
        }

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

        let response = self
            .client
            .post(url)
            .bearer_auth(&self.token)
            .header("Notion-Version", NOTION_API_VERSION)
            .json(&payload)
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
}
