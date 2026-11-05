use anyhow::{Context, Result};
use serde_json::{Value, json};

use crate::models::Credential;
use crate::notion::client::NotionClient;
use crate::secrets::Database;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct NewCredential {
    pub name: String,
    pub purpose: String,
    pub credential_reference: String,
    pub environment_variable: String,
    pub environment: String,
    pub repository: String,
    pub owner: String,
    pub rotation_frequency: String,
    pub last_rotated: Option<String>,
    pub next_rotation_due: Option<String>,
}

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

pub fn list_credentials(database: &Database) -> Result<Vec<Credential>> {
    let client = NotionClient::new(database)?;

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

fn title_property(content: &str) -> Value {
    json!({
        "title": [
            {
                "type": "text",
                "text": { "content": content }
            }
        ]
    })
}

fn text_property(content: &str) -> Value {
    json!({
        "rich_text": [
            {
                "type": "text",
                "text": { "content": content }
            }
        ]
    })
}

fn select_property(name: &str) -> Value {
    json!({
        "select": { "name": name }
    })
}

fn date_property(value: &str) -> Value {
    json!({
        "date": { "start": value }
    })
}

fn credential_properties(credential: &NewCredential) -> Value {
    let mut properties = serde_json::Map::new();

    properties.insert("Name".into(), title_property(&credential.name));
    properties.insert("Status".into(), select_property("Active"));
    properties.insert("Purpose".into(), text_property(&credential.purpose));
    properties.insert(
        "Credential Reference".into(),
        text_property(&credential.credential_reference),
    );
    properties.insert(
        "Environment Variable".into(),
        text_property(&credential.environment_variable),
    );

    if !credential.environment.trim().is_empty() {
        properties.insert(
            "Environment".into(),
            select_property(&credential.environment),
        );
    }

    properties.insert("Repository".into(), text_property(&credential.repository));
    properties.insert("Owner".into(), text_property(&credential.owner));

    if !credential.rotation_frequency.trim().is_empty() {
        properties.insert(
            "Rotation Frequency".into(),
            select_property(&credential.rotation_frequency),
        );
    }

    if let Some(value) = credential.last_rotated.as_deref() {
        properties.insert("Last Rotated".into(), date_property(value));
    }

    if let Some(value) = credential.next_rotation_due.as_deref() {
        properties.insert("Next Rotation Due".into(), date_property(value));
    }

    Value::Object(properties)
}

fn text_property_or_clear(content: &str) -> Value {
    if content.trim().is_empty() {
        json!({ "rich_text": [] })
    } else {
        text_property(content)
    }
}

fn select_property_or_clear(name: &str) -> Value {
    if name.trim().is_empty() {
        json!({ "select": null })
    } else {
        select_property(name)
    }
}

fn date_property_or_clear(value: Option<&str>) -> Value {
    match value {
        Some(value) if !value.trim().is_empty() => date_property(value),
        _ => json!({ "date": null }),
    }
}

// Updates leave Status alone and explicitly clear emptied fields, because
// Notion keeps the previous value when a property is omitted from a PATCH.
fn update_properties(credential: &NewCredential) -> Value {
    let mut properties = serde_json::Map::new();

    properties.insert("Name".into(), title_property(&credential.name));
    properties.insert(
        "Purpose".into(),
        text_property_or_clear(&credential.purpose),
    );
    properties.insert(
        "Credential Reference".into(),
        text_property_or_clear(&credential.credential_reference),
    );
    properties.insert(
        "Environment Variable".into(),
        text_property_or_clear(&credential.environment_variable),
    );
    properties.insert(
        "Environment".into(),
        select_property_or_clear(&credential.environment),
    );
    properties.insert(
        "Repository".into(),
        text_property_or_clear(&credential.repository),
    );
    properties.insert("Owner".into(), text_property_or_clear(&credential.owner));
    properties.insert(
        "Rotation Frequency".into(),
        select_property_or_clear(&credential.rotation_frequency),
    );
    properties.insert(
        "Last Rotated".into(),
        date_property_or_clear(credential.last_rotated.as_deref()),
    );
    properties.insert(
        "Next Rotation Due".into(),
        date_property_or_clear(credential.next_rotation_due.as_deref()),
    );

    Value::Object(properties)
}

pub fn create_credential(credential: &NewCredential, database: &Database) -> Result<()> {
    let client = NotionClient::new(database)?;
    client.create_page(&credential_properties(credential))?;
    Ok(())
}

pub fn update_credential(
    page_id: &str,
    credential: &NewCredential,
    database: &Database,
) -> Result<()> {
    let client = NotionClient::new(database)?;
    client.update_page(page_id, &update_properties(credential))?;
    Ok(())
}

pub fn archive_credential(page_id: &str, database: &Database) -> Result<()> {
    let client = NotionClient::new(database)?;
    client.archive_page(page_id)?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn credential() -> NewCredential {
        NewCredential {
            name: "Test Credential".into(),
            purpose: "Testing".into(),
            credential_reference: "sqlite://Developer Credentials/item/credential".into(),
            environment_variable: "TEST_API_KEY".into(),
            environment: "Local".into(),
            repository: "example/repo".into(),
            owner: "Owner".into(),
            rotation_frequency: "Every 90 days".into(),
            last_rotated: None,
            next_rotation_due: None,
        }
    }

    #[test]
    fn maps_required_properties() {
        let properties = credential_properties(&credential());

        assert_eq!(
            properties["Name"]["title"][0]["text"]["content"],
            "Test Credential"
        );
        assert_eq!(properties["Status"]["select"]["name"], "Active");
        assert_eq!(
            properties["Credential Reference"]["rich_text"][0]["text"]["content"],
            "sqlite://Developer Credentials/item/credential"
        );
        assert_eq!(
            properties["Environment Variable"]["rich_text"][0]["text"]["content"],
            "TEST_API_KEY"
        );
        assert_eq!(properties["Environment"]["select"]["name"], "Local");
        assert_eq!(
            properties["Rotation Frequency"]["select"]["name"],
            "Every 90 days"
        );
    }

    #[test]
    fn omits_optional_dates_when_absent() {
        let properties = credential_properties(&credential());

        assert!(properties.get("Last Rotated").is_none());
        assert!(properties.get("Next Rotation Due").is_none());
    }

    #[test]
    fn includes_optional_dates_when_present() {
        let mut credential = credential();
        credential.last_rotated = Some("2026-01-01".into());
        credential.next_rotation_due = Some("2026-04-01".into());

        let properties = credential_properties(&credential);

        assert_eq!(properties["Last Rotated"]["date"]["start"], "2026-01-01");
        assert_eq!(
            properties["Next Rotation Due"]["date"]["start"],
            "2026-04-01"
        );
    }

    #[test]
    fn properties_never_contain_secret_values() {
        let properties = credential_properties(&credential());

        assert!(properties.get("Secret").is_none());
        assert!(properties.get("credential").is_none());
    }

    #[test]
    fn update_properties_do_not_touch_status() {
        let properties = update_properties(&credential());

        assert!(properties.get("Status").is_none());
        assert_eq!(
            properties["Name"]["title"][0]["text"]["content"],
            "Test Credential"
        );
    }

    #[test]
    fn update_properties_clear_emptied_fields() {
        let mut credential = credential();
        credential.purpose.clear();
        credential.environment.clear();
        credential.rotation_frequency.clear();
        credential.last_rotated = None;
        credential.next_rotation_due = None;

        let properties = update_properties(&credential);

        assert_eq!(properties["Purpose"]["rich_text"], json!([]));
        assert_eq!(properties["Environment"]["select"], Value::Null);
        assert_eq!(properties["Rotation Frequency"]["select"], Value::Null);
        assert_eq!(properties["Last Rotated"]["date"], Value::Null);
        assert_eq!(properties["Next Rotation Due"]["date"], Value::Null);
    }

    #[test]
    fn update_properties_never_contain_secret_values() {
        let properties = update_properties(&credential());

        assert!(properties.get("Secret").is_none());
        assert!(properties.get("credential").is_none());
    }
}
