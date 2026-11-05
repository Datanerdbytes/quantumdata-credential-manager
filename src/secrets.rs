use anyhow::{Context, Result, anyhow};
use rusqlite::{Connection, params};
use std::path::Path;

const NOTION_API_KEY_ID: &str = "notion-api-key";

#[derive(Debug, Clone)]
pub struct SecretItem {
    pub id: String,
    pub title: String,
}

pub struct Database {
    connection: Connection,
}

impl Database {
    pub fn open(path: &Path) -> Result<Self> {
        let connection = Connection::open(path)
            .with_context(|| format!("Failed to open SQLite database at {}", path.display()))?;
        let database = Self { connection };
        database.migrate()?;
        Ok(database)
    }

    fn migrate(&self) -> Result<()> {
        self.connection
            .execute_batch(
                "
                CREATE TABLE IF NOT EXISTS secrets (
                    id TEXT PRIMARY KEY,
                    title TEXT NOT NULL,
                    username TEXT,
                    secret TEXT NOT NULL,
                    created_at TEXT NOT NULL DEFAULT (datetime('now'))
                );

                CREATE TABLE IF NOT EXISTS meta (
                    key TEXT PRIMARY KEY,
                    value TEXT NOT NULL
                );
                ",
            )
            .context("Failed to run SQLite migrations")?;
        Ok(())
    }

    pub fn list_items(&self) -> Result<Vec<SecretItem>> {
        let mut statement = self
            .connection
            .prepare("SELECT id, title FROM secrets ORDER BY title")
            .context("Failed to prepare secret list query")?;

        let rows = statement
            .query_map([], |row| {
                Ok(SecretItem {
                    id: row.get(0)?,
                    title: row.get(1)?,
                })
            })
            .context("Failed to query secrets")?;

        let mut items = Vec::new();
        for row in rows {
            items.push(row.context("Failed to read a secret row")?);
        }
        Ok(items)
    }

    pub fn create_item(&self, title: &str, username: Option<&str>, secret: &str) -> Result<String> {
        if title.trim().is_empty() {
            return Err(anyhow!("An item title is required."));
        }

        if secret.is_empty() {
            return Err(anyhow!("The API secret is required."));
        }

        let id = uuid::Uuid::new_v4().to_string();

        self.connection
            .execute(
                "INSERT INTO secrets (id, title, username, secret) VALUES (?, ?, ?, ?)",
                params![id, title.trim(), username.unwrap_or(""), secret],
            )
            .context("Failed to insert secret into database")?;

        Ok(id)
    }

    pub fn get_secret(&self, id: &str) -> Result<Option<String>> {
        let mut statement = self
            .connection
            .prepare("SELECT secret FROM secrets WHERE id = ?")
            .context("Failed to prepare secret lookup query")?;

        let mut rows = statement.query_map(params![id], |row| row.get::<_, String>(0))?;

        if let Some(row) = rows.next() {
            return Ok(Some(row.context("Failed to read secret value")?));
        }

        Ok(None)
    }

    pub fn copy_secret_to_clipboard(&self, reference: &str) -> Result<()> {
        // Strip the sqlite:// prefix if present, since the database id column
        // stores only the raw UUID without the scheme prefix.
        let db_id = reference.strip_prefix("sqlite://").unwrap_or(reference);

        let secret = self
            .get_secret(db_id)?
            .ok_or_else(|| anyhow!("No secret found for reference '{reference}' in SQLite database."))?;

        copy_to_clipboard(&secret)
    }

    pub fn set_notion_api_key(&self, key: &str) -> Result<()> {
        if key.trim().is_empty() {
            return Err(anyhow!("The Notion API key must not be empty."));
        }

        self.connection
            .execute(
                "INSERT INTO meta (key, value) VALUES (?, ?)
                 ON CONFLICT(key) DO UPDATE SET value = excluded.value",
                params![NOTION_API_KEY_ID, key.trim()],
            )
            .context("Failed to store the Notion API key")?;

        Ok(())
    }

    pub fn get_notion_api_key(&self) -> Result<Option<String>> {
        let mut statement = self
            .connection
            .prepare("SELECT value FROM meta WHERE key = ?")
            .context("Failed to prepare meta lookup query")?;

        let mut rows = statement.query_map(params![NOTION_API_KEY_ID], |row| {
            row.get::<_, String>(0)
        })?;

        if let Some(row) = rows.next() {
            return Ok(Some(row.context("Failed to read meta value")?));
        }

        Ok(None)
    }
}

fn copy_to_clipboard(text: &str) -> Result<()> {
    use std::io::Write;
    use std::process::{Command, Stdio};

    let mut clipboard = Command::new("pbcopy")
        .stdin(Stdio::piped())
        .spawn()
        .context("Failed to launch macOS clipboard utility")?;

    clipboard
        .stdin
        .take()
        .context("Could not access clipboard input")?
        .write_all(text.as_bytes())
        .context("Failed to copy credential to clipboard")?;

    let status = clipboard
        .wait()
        .context("Failed to complete clipboard operation")?;

    if !status.success() {
        return Err(anyhow!("Failed to copy credential to clipboard."));
    }

    Ok(())
}