use anyhow::{Context, Result, anyhow};
use rusqlite::{Connection, OptionalExtension, params};
use std::path::{Path, PathBuf};
use std::time::Duration;

use crate::crypto::{Cipher, Key, is_encrypted};

const NOTION_API_KEY_ID: &str = "notion-api-key";
const ENCRYPTION_MARKER: &str = "encryption";
const ENCRYPTION_VERSION: &str = "v1";
const STDOUT_FALLBACK_ENV: &str = "QUANTUMDATA_ALLOW_STDOUT_SECRET";

pub const CLIPBOARD_CLEAR_SECONDS: u64 = 45;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CopyOutcome {
    Clipboard,
    Stdout,
}

#[derive(Debug, Clone)]
pub struct SecretItem {
    pub id: String,
    pub title: String,
}

pub struct Database {
    connection: Connection,
    cipher: Option<Cipher>,
    key: Option<Key>,
    path: PathBuf,
}

impl Database {
    #[cfg(test)]
    pub fn open(path: &Path) -> Result<Self> {
        Self::open_inner(path, None)
    }

    pub fn open_with_key(path: &Path, key: Key) -> Result<Self> {
        Self::open_inner(path, Some(key))
    }

    pub fn open_with_optional_key(path: &Path, key: Option<Key>) -> Result<Self> {
        Self::open_inner(path, key)
    }

    fn open_inner(path: &Path, key: Option<Key>) -> Result<Self> {
        let connection = Connection::open(path)
            .with_context(|| format!("Failed to open SQLite database at {}", path.display()))?;
        connection
            .busy_timeout(Duration::from_secs(5))
            .context("Failed to configure the SQLite busy timeout")?;

        let database = Self {
            connection,
            cipher: key.as_ref().map(Cipher::new),
            key,
            path: path.to_path_buf(),
        };
        database.migrate()?;

        if database.cipher.is_some() {
            database.encrypt_plaintext_rows()?;
        } else if database.get_meta(ENCRYPTION_MARKER)?.is_some() {
            return Err(anyhow!(
                "The SQLite database at {} is encrypted. Start the app normally so it can load the master key.",
                path.display()
            ));
        }

        Ok(database)
    }

    pub fn path(&self) -> &Path {
        &self.path
    }

    pub fn key(&self) -> Option<Key> {
        self.key
    }

    fn encrypt(&self, value: &str) -> Result<String> {
        match &self.cipher {
            Some(cipher) => cipher.encrypt(value),
            None => Ok(value.to_string()),
        }
    }

    fn decrypt(&self, stored: &str) -> Result<String> {
        match &self.cipher {
            Some(cipher) if is_encrypted(stored) => cipher.decrypt(stored),
            Some(_) => Ok(stored.to_string()),
            None if is_encrypted(stored) => Err(anyhow!(
                "The stored value is encrypted but no master key is loaded."
            )),
            None => Ok(stored.to_string()),
        }
    }

    fn encrypt_plaintext_rows(&self) -> Result<()> {
        let Some(cipher) = self.cipher.as_ref() else {
            return Ok(());
        };

        let transaction = self
            .connection
            .unchecked_transaction()
            .context("Failed to start the encryption migration")?;

        let secrets: Vec<(String, String)> = {
            let mut statement = transaction
                .prepare("SELECT id, secret FROM secrets")
                .context("Failed to prepare the encryption migration")?;
            let rows = statement
                .query_map([], |row| Ok((row.get(0)?, row.get(1)?)))
                .context("Failed to read secrets for encryption")?;
            rows.collect::<rusqlite::Result<Vec<_>>>()
                .context("Failed to read secrets for encryption")?
        };

        for (id, secret) in secrets {
            if is_encrypted(&secret) {
                continue;
            }
            transaction
                .execute(
                    "UPDATE secrets SET secret = ? WHERE id = ?",
                    params![cipher.encrypt(&secret)?, id],
                )
                .context("Failed to encrypt a stored secret")?;
        }

        if let Some(stored) = transaction
            .query_row(
                "SELECT value FROM meta WHERE key = ?",
                params![NOTION_API_KEY_ID],
                |row| row.get::<_, String>(0),
            )
            .optional()
            .context("Failed to read the Notion API key for encryption")?
        {
            if !is_encrypted(&stored) {
                transaction
                    .execute(
                        "UPDATE meta SET value = ? WHERE key = ?",
                        params![cipher.encrypt(&stored)?, NOTION_API_KEY_ID],
                    )
                    .context("Failed to encrypt the Notion API key")?;
            }
        }

        transaction
            .execute(
                "INSERT INTO meta (key, value) VALUES (?, ?)
                 ON CONFLICT(key) DO UPDATE SET value = excluded.value",
                params![ENCRYPTION_MARKER, ENCRYPTION_VERSION],
            )
            .context("Failed to record the encryption version")?;

        transaction
            .commit()
            .context("Failed to commit the encryption migration")?;

        Ok(())
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
        let stored = self.encrypt(secret)?;

        self.connection
            .execute(
                "INSERT INTO secrets (id, title, username, secret) VALUES (?, ?, ?, ?)",
                params![id, title.trim(), username.unwrap_or(""), stored],
            )
            .context("Failed to insert secret into database")?;

        Ok(id)
    }

    pub fn get_secret(&self, id: &str) -> Result<Option<String>> {
        let mut statement = self
            .connection
            .prepare("SELECT secret FROM secrets WHERE id = ?")
            .context("Failed to prepare secret lookup query")?;

        let mut rows = statement
            .query(params![id])
            .context("Failed to query the secret")?;

        if let Some(row) = rows.next().context("Failed to read secret value")? {
            let stored: String = row.get(0).context("Failed to read secret value")?;
            return self.decrypt(&stored).map(Some);
        }

        Ok(None)
    }

    pub fn copy_secret_to_clipboard(&self, reference: &str) -> Result<CopyOutcome> {
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

        let stored = self.encrypt(key.trim())?;
        self.set_meta(NOTION_API_KEY_ID, &stored)
            .context("Failed to store the Notion API key")?;

        Ok(())
    }

    pub fn get_notion_api_key(&self) -> Result<Option<String>> {
        self.get_meta(NOTION_API_KEY_ID)?
            .map(|value| self.decrypt(&value))
            .transpose()
    }

    pub fn get_meta(&self, key: &str) -> Result<Option<String>> {
        let mut statement = self
            .connection
            .prepare("SELECT value FROM meta WHERE key = ?")
            .context("Failed to prepare meta lookup query")?;

        statement
            .query_row(params![key], |row| row.get::<_, String>(0))
            .optional()
            .context("Failed to read meta value")
    }

    pub fn set_meta(&self, key: &str, value: &str) -> Result<()> {
        self.connection
            .execute(
                "INSERT INTO meta (key, value) VALUES (?, ?)
                 ON CONFLICT(key) DO UPDATE SET value = excluded.value",
                params![key, value],
            )
            .context("Failed to store meta value")?;

        Ok(())
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
            Some(secret) => {
                let stored = self.encrypt(secret)?;
                self.connection.execute(
                    "UPDATE secrets SET title = ?, secret = ? WHERE id = ?",
                    params![new_title, stored, id],
                )
            }
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

fn copy_to_clipboard(text: &str) -> Result<CopyOutcome> {
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
                schedule_clipboard_clear(text.to_string());
                return Ok(CopyOutcome::Clipboard);
            }
        }
        Err(_) => {
            // pbcopy not available (e.g., inside Docker/Linux container)
        }
    }

    if stdout_fallback_enabled(std::env::var(STDOUT_FALLBACK_ENV).ok().as_deref()) {
        println!("{}", text);
        return Ok(CopyOutcome::Stdout);
    }

    Err(anyhow!(
        "Clipboard unavailable (pbcopy not found). Set {STDOUT_FALLBACK_ENV}=1 to print the secret to stdout instead."
    ))
}

fn schedule_clipboard_clear(secret: String) {
    std::thread::spawn(move || {
        std::thread::sleep(Duration::from_secs(CLIPBOARD_CLEAR_SECONDS));
        clear_clipboard_if_unchanged(secret.as_bytes());
    });
}

fn clear_clipboard_if_unchanged(secret: &[u8]) {
    use std::process::{Command, Stdio};

    let Ok(current) = Command::new("pbpaste").output() else {
        return;
    };

    if !current.status.success() || current.stdout != secret {
        return;
    }

    // Dropping the stdin handle closes it immediately, so pbcopy receives EOF
    // and replaces the clipboard with an empty string.
    if let Ok(mut clipboard) = Command::new("pbcopy").stdin(Stdio::piped()).spawn() {
        drop(clipboard.stdin.take());
        let _ = clipboard.wait();
    }
}

fn stdout_fallback_enabled(value: Option<&str>) -> bool {
    let Some(value) = value else {
        return false;
    };

    matches!(
        value.trim().to_ascii_lowercase().as_str(),
        "1" | "true" | "yes"
    )
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
    fn stdout_fallback_is_opt_in() {
        assert!(!stdout_fallback_enabled(None));
        assert!(!stdout_fallback_enabled(Some("")));
        assert!(!stdout_fallback_enabled(Some("0")));
        assert!(!stdout_fallback_enabled(Some("false")));
        assert!(stdout_fallback_enabled(Some("1")));
        assert!(stdout_fallback_enabled(Some(" TRUE ")));
        assert!(stdout_fallback_enabled(Some("yes")));
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

    fn encrypted_test_database() -> (Database, tempfile::TempDir) {
        let directory = tempfile::tempdir().expect("failed to create temp directory");
        let path = directory.path().join("credentials.db");
        let database =
            Database::open_with_key(&path, [7u8; 32]).expect("failed to open encrypted database");
        (database, directory)
    }

    fn raw_secret(database: &Database, id: &str) -> String {
        database
            .connection
            .query_row(
                "SELECT secret FROM secrets WHERE id = ?",
                params![id],
                |row| row.get(0),
            )
            .expect("failed to read raw secret")
    }

    #[test]
    fn encrypts_secrets_at_rest() {
        let (database, _directory) = encrypted_test_database();
        let id = database
            .create_item("OpenAI", None, "sk-plaintext")
            .expect("failed to create item");

        assert_eq!(
            database.get_secret(&id).expect("failed to read secret"),
            Some("sk-plaintext".to_string())
        );
        let raw = raw_secret(&database, &id);
        assert!(raw.starts_with("enc:v1:"));
        assert_ne!(raw, "sk-plaintext");
    }

    #[test]
    fn encrypts_notion_api_key_at_rest() {
        let (database, _directory) = encrypted_test_database();
        database
            .set_notion_api_key("secret_notion_token")
            .expect("failed to set key");

        let raw = database
            .get_meta("notion-api-key")
            .expect("failed to read meta")
            .expect("missing meta value");
        assert!(raw.starts_with("enc:v1:"));
        assert_eq!(
            database.get_notion_api_key().expect("failed to get key"),
            Some("secret_notion_token".to_string())
        );
    }

    #[test]
    fn migrates_plaintext_rows_when_opened_with_a_key() {
        let directory = tempfile::tempdir().expect("failed to create temp directory");
        let path = directory.path().join("credentials.db");

        let id = {
            let database = Database::open(&path).expect("failed to open plaintext database");
            let id = database
                .create_item("OpenAI", None, "secret")
                .expect("failed to create item");
            database
                .set_notion_api_key("notion-token")
                .expect("failed to set key");
            id
        };

        let database = Database::open_with_key(&path, [3u8; 32]).expect("failed to reopen");

        assert_eq!(
            database.get_secret(&id).expect("failed to read secret"),
            Some("secret".to_string())
        );
        assert_eq!(
            database.get_notion_api_key().expect("failed to get key"),
            Some("notion-token".to_string())
        );
        assert!(raw_secret(&database, &id).starts_with("enc:v1:"));
        assert_eq!(
            database
                .get_meta("encryption")
                .expect("failed to read marker")
                .as_deref(),
            Some("v1")
        );
    }

    #[test]
    fn reencrypts_plaintext_rows_written_after_migration() {
        let (database, _directory) = encrypted_test_database();
        let path = database.path().to_path_buf();
        let id = database
            .create_item("OpenAI", None, "secret")
            .expect("failed to create item");
        database
            .connection
            .execute(
                "UPDATE secrets SET secret = 'legacy-plain' WHERE id = ?",
                params![id],
            )
            .expect("failed to downgrade a row");
        drop(database);

        let database = Database::open_with_key(&path, [7u8; 32]).expect("failed to reopen");

        assert!(raw_secret(&database, &id).starts_with("enc:v1:"));
        assert_eq!(
            database.get_secret(&id).expect("failed to read secret"),
            Some("legacy-plain".to_string())
        );
    }

    #[test]
    fn plaintext_open_refuses_an_encrypted_database() {
        let directory = tempfile::tempdir().expect("failed to create temp directory");
        let path = directory.path().join("credentials.db");

        {
            let database =
                Database::open_with_key(&path, [7u8; 32]).expect("failed to open encrypted db");
            database
                .create_item("OpenAI", None, "secret")
                .expect("failed to create item");
        }

        let error = match Database::open(&path) {
            Ok(_) => panic!("plaintext open should fail"),
            Err(error) => error,
        };
        assert!(error.to_string().contains("encrypted"));
    }

    #[test]
    fn a_different_key_cannot_decrypt() {
        let directory = tempfile::tempdir().expect("failed to create temp directory");
        let path = directory.path().join("credentials.db");

        let id = {
            let database =
                Database::open_with_key(&path, [7u8; 32]).expect("failed to open encrypted db");
            database
                .create_item("OpenAI", None, "secret")
                .expect("failed to create item")
        };

        let database =
            Database::open_with_key(&path, [8u8; 32]).expect("failed to reopen with other key");
        assert!(database.get_secret(&id).is_err());
    }

    #[test]
    fn replacement_secrets_are_encrypted() {
        let (database, _directory) = encrypted_test_database();
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
        assert!(raw_secret(&database, &id).starts_with("enc:v1:"));
    }
}
