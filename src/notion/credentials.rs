use anyhow::{Context, Result};
use serde_json::Value;

use crate::models::Credential;
use crate::notion::client::NotionClient;

fn rich_text_value(property: &Value) -> Option<String> {
    let items = property.get("rich_text")?.as_array()?;

    let value: String = items
        .iter()
        .filter_map(|item| item.get("plain_text")?.as_str())
        .collect();

    if value.is_empty() { None } else { Some(value) }
}

fn title_value(property: &Value) -> Option<String> {
    let items = property.get("title")?.as_array()?;

    let value: String = items
        .iter()
        .filter_map(|item| item.get("plain_text")?.as_str())
        .collect();

    if value.is_empty() { None } else { Some(value) }
}

fn select_value(property: &Value) -> Option<String> {
    property
        .get("select")?
        .get("name")?
        .as_str()
        .map(str::to_string)
}

fn date_value(property: &Value) -> Option<String> {
    property
        .get("date")?
        .get("start")?
        .as_str()
        .map(str::to_string)
}

fn property<'a>(properties: &'a Value, name: &str) -> &'a Value {
    &properties[name]
}

fn map_page(page: &Value) -> Result<Credential> {
    let properties = page
        .get("properties")
        .context("Notion page is missing properties")?;

    let name = title_value(property(properties, "Name"))
        .unwrap_or_else(|| "Untitled Credential".to_string());

    let purpose = rich_text_value(property(properties, "Purpose"));

    Ok(Credential {
        id: page.get("id").and_then(Value::as_str).map(str::to_string),

        name,

        // Compatibility fields
        description: purpose.clone(),
        service: None,
        username: None,

        // Notion properties
        status: select_value(property(properties, "Status")),
        purpose,
        credential_reference: rich_text_value(property(properties, "Credential Reference")),
        environment_variable: rich_text_value(property(properties, "Environment Variable")),
        environment: select_value(property(properties, "Environment")),
        repository: rich_text_value(property(properties, "Repository")),
        owner: rich_text_value(property(properties, "Owner")),
        rotation_frequency: select_value(property(properties, "Rotation Frequency")),
        last_rotated: date_value(property(properties, "Last Rotated")),
        next_rotation_due: date_value(property(properties, "Next Rotation Due")),

        created_at: page
            .get("created_time")
            .and_then(Value::as_str)
            .map(str::to_string),

        updated_at: page
            .get("last_edited_time")
            .and_then(Value::as_str)
            .map(str::to_string),
    })
}

pub fn list_credentials() -> Result<Vec<Credential>> {
    let client = NotionClient::new()?;

    let mut credentials = Vec::new();
    let mut cursor: Option<String> = None;

    loop {
        let response = client.query_data_source(cursor.as_deref())?;

        let results = response
            .get("results")
            .and_then(Value::as_array)
            .context("Notion response is missing the results array")?;

        for page in results {
            credentials.push(map_page(page)?);
        }

        let has_more = response
            .get("has_more")
            .and_then(Value::as_bool)
            .unwrap_or(false);

        if !has_more {
            break;
        }

        cursor = response
            .get("next_cursor")
            .and_then(Value::as_str)
            .map(str::to_string);

        if cursor.is_none() {
            anyhow::bail!("Notion indicated more results but did not provide a pagination cursor");
        }
    }

    Ok(credentials)
}
