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

    pub fn copy_secret_to_clipboard(&self, reference: &str) -> Result<bool> {
        // Strip the sqlite:// prefix if present, since the database id column
        // stores only the raw UUID without the scheme prefix.
        let db_id = reference.strip_prefix("sqlite://").unwrap_or(reference);

        let secret = self.get_secret(db_id)?.ok_or_else(|| {
            anyhow!("No secret found for reference '{reference}' in SQLite database.")
        })?;

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

        let mut rows =
            statement.query_map(params![NOTION_API_KEY_ID], |row| row.get::<_, String>(0))?;

        if let Some(row) = rows.next() {
            return Ok(Some(row.context("Failed to read meta value")?));
        }

        Ok(None)
    }

    pub fn delete_notion_api_key(&self) -> Result<()> {
        let rows_affected = self
            .connection
            .execute("DELETE FROM meta WHERE key = ?", params![NOTION_API_KEY_ID])
            .context("Failed to delete Notion API key from database")?;

        if rows_affected == 0 {
            return Err(anyhow!("No Notion API key found in database."));
        }

        Ok(())
    }

    pub fn delete_credential(&self, reference: Option<&str>, title: &str) -> Result<bool> {
        match self.resolve_id(reference, title)? {
            Some(id) => self.delete_by_id(&id),
            None => Ok(false),
        }
    }

    pub fn update_credential(
        &self,
        reference: Option<&str>,
        title: &str,
        new_title: &str,
        new_secret: Option<&str>,
    ) -> Result<bool> {
        let new_title = new_title.trim();

        if new_title.is_empty() {
            return Err(anyhow!("A credential name is required."));
        }

        let Some(id) = self.resolve_id(reference, title)? else {
            return Ok(false);
        };

        match new_secret.filter(|secret| !secret.is_empty()) {
            Some(secret) => self.connection.execute(
                "UPDATE secrets SET title = ?, secret = ? WHERE id = ?",
                params![new_title, secret, id],
            ),
            None => self.connection.execute(
                "UPDATE secrets SET title = ? WHERE id = ?",
                params![new_title, id],
            ),
        }
        .context("Failed to update credential in database")?;

        Ok(true)
    }

    fn resolve_id(&self, reference: Option<&str>, title: &str) -> Result<Option<String>> {
        let reference = reference.map(str::trim).filter(|value| !value.is_empty());

        if let Some(reference) = reference {
            let id = reference.strip_prefix("sqlite://").unwrap_or(reference);
            if self.contains_id(id)? {
                return Ok(Some(id.to_string()));
            }
        }

        // The Notion reference can drift from the local database (for example
        // after editing the page or migrating stores), so fall back to the
        // displayed title when it identifies exactly one stored secret.
        let ids = self.item_ids_by_title(title)?;

        match ids.as_slice() {
            [id] => Ok(Some(id.clone())),
            [] => Ok(None),
            _ => Err(anyhow!(
                "Multiple local secrets are named '{}'; make the credential reference unambiguous.",
                title.trim()
            )),
        }
    }

    fn contains_id(&self, id: &str) -> Result<bool> {
        let count: i64 = self
            .connection
            .query_row(
                "SELECT COUNT(*) FROM secrets WHERE id = ?",
                params![id],
                |row| row.get(0),
            )
            .context("Failed to check whether the credential exists")?;

        Ok(count > 0)
    }

    fn delete_by_id(&self, id: &str) -> Result<bool> {
        let rows_affected = self
            .connection
            .execute("DELETE FROM secrets WHERE id = ?", params![id])
            .context("Failed to delete credential from database")?;

        Ok(rows_affected > 0)
    }

    fn item_ids_by_title(&self, title: &str) -> Result<Vec<String>> {
        let mut statement = self
            .connection
            .prepare("SELECT id FROM secrets WHERE title = ? COLLATE NOCASE")
            .context("Failed to prepare credential title lookup")?;

        let rows = statement
            .query_map(params![title.trim()], |row| row.get(0))
            .context("Failed to query credentials by title")?;

        let mut ids = Vec::new();
        for row in rows {
            ids.push(row.context("Failed to read a credential row")?);
        }

        Ok(ids)
    }
}

fn copy_to_clipboard(text: &str) -> Result<bool> {
    use std::io::Write;
    use std::process::{Command, Stdio};

    match Command::new("pbcopy").stdin(Stdio::piped()).spawn() {
        Ok(mut clipboard) => {
            clipboard
                .stdin
                .take()
                .context("Could not access clipboard input")?
                .write_all(text.as_bytes())
                .context("Failed to copy credential to clipboard")?;

            let status = clipboard
                .wait()
                .context("Failed to complete clipboard operation")?;

            if status.success() {
                return Ok(true);
            }
        }
        Err(_) => {
            // pbcopy not available (e.g., inside Docker/Linux container)
        }
    }

    // Fallback: print to stdout so user can copy from terminal output
    println!("{}", text);
    Ok(false)
}

#[cfg(test)]
mod tests {
    use super::*;
    fn test_database() -> (Database, tempfile::TempDir) {
        let directory = tempfile::tempdir().expect("failed to create temp directory");
        let path = directory.path().join("credentials.db");
        let database = Database::open(&path).expect("failed to open test database");
        (database, directory)
    }

    #[test]
    fn deletes_by_sqlite_reference() {
        let (database, _directory) = test_database();
        let id = database
            .create_item("OpenAI", None, "secret")
            .expect("failed to create item");

        database
            .delete_credential(Some(&format!("sqlite://{id}")), "OpenAI")
            .expect("failed to delete credential");

        assert!(
            database
                .get_secret(&id)
                .expect("failed to read secret")
                .is_none()
        );
    }

    #[test]
    fn falls_back_to_title_when_reference_is_stale() {
        let (database, _directory) = test_database();
        let id = database
            .create_item("OpenAI", None, "secret")
            .expect("failed to create item");

        database
            .delete_credential(Some("sqlite://missing-id"), "OpenAi")
            .expect("failed to delete credential");

        assert!(
            database
                .get_secret(&id)
                .expect("failed to read secret")
                .is_none()
        );
    }

    #[test]
    fn falls_back_to_title_when_reference_is_absent() {
        let (database, _directory) = test_database();
        let id = database
            .create_item("OpenAI", None, "secret")
            .expect("failed to create item");

        database
            .delete_credential(None, "  openai ")
            .expect("failed to delete credential");

        assert!(
            database
                .get_secret(&id)
                .expect("failed to read secret")
                .is_none()
        );
    }

    #[test]
    fn reports_when_nothing_matches() {
        let (database, _directory) = test_database();

        let deleted = database
            .delete_credential(Some("sqlite://missing-id"), "OpenAi")
            .expect("lookup should not fail");

        assert!(!deleted);
    }

    #[test]
    fn refuses_to_guess_when_titles_are_ambiguous() {
        let (database, _directory) = test_database();
        database
            .create_item("OpenAI", None, "first")
            .expect("failed to create item");
        database
            .create_item("OpenAi", None, "second")
            .expect("failed to create item");

        let error = database
            .delete_credential(Some("sqlite://missing-id"), "OpenAi")
            .expect_err("deletion should fail");

        assert!(error.to_string().contains("Multiple local secrets"));
        assert_eq!(
            database.list_items().expect("failed to list items").len(),
            2
        );
    }

    #[test]
    fn updates_title_without_touching_the_secret() {
        let (database, _directory) = test_database();
        let id = database
            .create_item("OpenAI", None, "secret")
            .expect("failed to create item");

        let updated = database
            .update_credential(
                Some(&format!("sqlite://{id}")),
                "OpenAI",
                "OpenAI Prod",
                None,
            )
            .expect("failed to update credential");

        assert!(updated);
        let items = database.list_items().expect("failed to list items");
        assert_eq!(items.len(), 1);
        assert_eq!(items[0].title, "OpenAI Prod");
        assert_eq!(
            database.get_secret(&id).expect("failed to read secret"),
            Some("secret".to_string())
        );
    }

    #[test]
    fn replaces_the_secret_when_provided() {
        let (database, _directory) = test_database();
        let id = database
            .create_item("OpenAI", None, "old-secret")
            .expect("failed to create item");

        database
            .update_credential(
                Some(&format!("sqlite://{id}")),
                "OpenAI",
                "OpenAI",
                Some("new-secret"),
            )
            .expect("failed to update credential");

        assert_eq!(
            database.get_secret(&id).expect("failed to read secret"),
            Some("new-secret".to_string())
        );
    }

    #[test]
    fn update_falls_back_to_title_when_reference_is_stale() {
        let (database, _directory) = test_database();
        let id = database
            .create_item("OpenAI", None, "secret")
            .expect("failed to create item");

        let updated = database
            .update_credential(Some("sqlite://missing-id"), "openai", "Renamed", None)
            .expect("failed to update credential");

        assert!(updated);
        assert_eq!(
            database.get_secret(&id).expect("failed to read secret"),
            Some("secret".to_string())
        );
        assert_eq!(
            database.list_items().expect("failed to list items")[0].title,
            "Renamed"
        );
    }

    #[test]
    fn update_reports_when_nothing_matches() {
        let (database, _directory) = test_database();

        let updated = database
            .update_credential(Some("sqlite://missing-id"), "OpenAI", "Renamed", None)
            .expect("lookup should not fail");

        assert!(!updated);
    }

    #[test]
    fn deletes_notion_api_key() {
        let (database, _directory) = test_database();
        database
            .set_notion_api_key("test-key-123")
            .expect("failed to set key");

        assert_eq!(
            database.get_notion_api_key().expect("failed to get key"),
            Some("test-key-123".to_string())
        );

        database
            .delete_notion_api_key()
            .expect("failed to delete key");

        assert_eq!(
            database
                .get_notion_api_key()
                .expect("failed to get key after delete"),
            None
        );
    }

    #[test]
    fn reports_missing_notion_api_key_on_delete() {
        let (database, _directory) = test_database();

        let error = database
            .delete_notion_api_key()
            .expect_err("deletion should fail");

        assert!(error.to_string().contains("No Notion API key found"));
    }
}
