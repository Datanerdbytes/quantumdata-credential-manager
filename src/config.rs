use anyhow::{Context, Result};
use std::env;

pub struct Settings {
    pub notion_api_key_op_ref: String,
    pub notion_data_source_id: String,
}

impl Settings {
    pub fn load() -> Result<Self> {
        dotenvy::dotenv().ok();

        let notion_api_key_op_ref = env::var("NOTION_API_KEY_OP_REF")
            .context("Missing NOTION_API_KEY_OP_REF environment variable")?;

        let notion_data_source_id = env::var("NOTION_DATA_SOURCE_ID")
            .context("Missing NOTION_DATA_SOURCE_ID environment variable")?;

        Ok(Self {
            notion_api_key_op_ref,
            notion_data_source_id,
        })
    }
}
