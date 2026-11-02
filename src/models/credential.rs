use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Credential {
    pub id: Option<String>,
    pub name: String,
    pub description: Option<String>,
    pub service: Option<String>,
    pub username: Option<String>,
    pub created_at: Option<String>,
    pub updated_at: Option<String>,
}
