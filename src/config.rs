use anyhow::{Context, Result, bail};
use std::env;
use std::path::PathBuf;

pub struct Settings {
    pub notion_data_source_id: String,
    pub db_path: PathBuf,
}

impl Settings {
    pub fn load() -> Result<Self> {
        dotenvy::dotenv().ok();

        Ok(Self {
            notion_data_source_id: required("NOTION_DATA_SOURCE_ID")?,
            db_path: db_path(),
        })
    }
}

fn required(key: &str) -> Result<String> {
    let value = env::var(key).with_context(|| format!("Missing {key} environment variable"))?;
    let value = value.trim().to_string();

    if value.is_empty() {
        bail!("{key} must not be empty");
    }

    Ok(value)
}

fn db_path() -> PathBuf {
    match env::var("DB_PATH") {
        Ok(value) => {
            let trimmed = value.trim();
            if trimmed.is_empty() {
                default_db_path()
            } else {
                PathBuf::from(trimmed)
            }
        }
        Err(_) => default_db_path(),
    }
}

fn default_db_path() -> PathBuf {
    PathBuf::from("credentials.db")
}
