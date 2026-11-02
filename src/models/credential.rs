use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Credential {
    pub id: Option<String>,
    pub name: String,

    // Existing compatibility fields
    pub description: Option<String>,
    pub service: Option<String>,
    pub username: Option<String>,

    // API Credential Inventory properties
    pub status: Option<String>,
    pub purpose: Option<String>,
    pub credential_reference: Option<String>,
    pub environment_variable: Option<String>,
    pub environment: Option<String>,
    pub repository: Option<String>,
    pub owner: Option<String>,
    pub rotation_frequency: Option<String>,
    pub last_rotated: Option<String>,
    pub next_rotation_due: Option<String>,

    // Notion timestamps
    pub created_at: Option<String>,
    pub updated_at: Option<String>,
}
