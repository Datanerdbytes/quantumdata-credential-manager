# QuantumData

A terminal UI for managing API credentials as Notion metadata while keeping the actual secrets in a local SQLite database.

Notion stores the inventory — name, purpose, owner, environment, rotation dates, and a local database reference. The secret value never leaves your machine. When you need a key, open it in the app and press `c`: the value goes straight to your clipboard, ready to paste into `.env`, a terminal, or a CI secret field. No vault, no Notion page, no scrolling through old notes.

> **Security note:** secrets and the Notion API key are encrypted at rest with AES-256-GCM. The master key lives in the macOS Keychain. See [Data and security](#data-and-security).

## Screenshot

![QuantumData terminal UI showing the credential operations menu](docs/screenshot.png)

*The main menu. Secrets stay in SQLite; Notion stores only metadata.*

## Features

- **One-key copy** — open any credential and press `c`. The secret is read from SQLite and piped to the clipboard, ready to paste into `.env`, a terminal, or an AI provider's API key field. The clipboard is cleared after 45 seconds if the value is unchanged.
- **Import a `.env` file** — bulk-create local secrets from `KEY=VALUE` lines and register each one in Notion, with a summary of imported, skipped, and failed entries.
- **Encrypted at rest** — secret values and the Notion token are sealed with AES-256-GCM. The master key is generated once and stored in the macOS Keychain; plaintext databases are migrated automatically at startup, and any plaintext row written afterwards is re-encrypted on the next start.
- **Rotation reminders** — credentials whose `Next Rotation Due` date falls within 7 days appear in a banner on the menu and trigger one aggregated macOS notification per day.
- **Rotate credentials** — replace a secret, advance `Last Rotated` to today, and derive the next due date from `Rotation Frequency` in one flow.
- **Add credentials** — create a secret in SQLite and register the metadata in Notion, or link an existing local secret to a new Notion entry.
- **Search and inspect** — filter by name, service, username, owner, environment, repository, or environment variable, then open a detail view that shows metadata only, never the secret.
- **Update credentials** — edit every Notion metadata field from a prefilled form and optionally replace the secret stored in SQLite.
- **Delete credentials** — archives the Notion page and removes the matching local secret. Falls back to a unique title match when the stored reference is stale.
- **Notion API key management** — store or delete the Notion integration token from inside the app.
- **Responsive UI** — Notion list loads run in the background, so the interface stays interactive while a request is in flight.
- **Startup splash** — an animated QuantumData splash; press any key to skip it.
- **Safe tests** — the test suite uses temporary databases and never calls Notion.

## Copy an API key in seconds

This is the highlight of the app. Whether you are wiring an AI provider into a side project, filling a CI secret, or refreshing a `.env` before a demo, you usually do not need to edit the credential — you just need the value on your clipboard, right now.

1. Launch `api-manager` and choose **Search / Retrieve Credential** (or **List Credentials** to browse).
2. Type part of the name. Results filter as you type.
3. Press `Enter` to open the credential.
4. Press `c`.

The status line confirms the copy:

```text
Secret copied to clipboard. It will be cleared in 45 seconds.
```

Paste it wherever it is needed:

```bash
# load it into the current shell
export OPENAI_API_KEY="$(pbpaste)"

# or append it to a project's env file
echo "OPENAI_API_KEY=$(pbpaste)" >> .env
```

What makes the flow fast and safe:

- **Local lookup** — Notion is the index; the value comes from the local SQLite row, so the copy itself happens instantly on your machine.
- **Never rendered** — the details screen shows metadata only. The secret is fetched when you press `c` and written straight to the clipboard, so it does not appear in the terminal or its scrollback.
- **Confirmed on screen** — the status line tells you whether the copy succeeded and when it will clear.
- **Auto-clears** — 45 seconds after the copy, the app empties the clipboard, but only if it still contains that same secret. Copying something else cancels the clear.
- **No silent stdout fallback** — if `pbcopy` is unavailable, the copy fails. Set `QUANTUMDATA_ALLOW_STDOUT_SECRET=1` to opt into printing the secret to the terminal instead. See [Data and security](#data-and-security).

## Bulk import a `.env` file

If a project already has a `.env`, import it instead of retyping every key.

1. Launch `api-manager` and choose **Import .env File**.
2. Enter the path — absolute, relative, or `~/project/.env`.
3. Optionally fill in **Environment**, **Owner**, **Repository**, and **Rotation Frequency**; those values are applied to every imported key, and `Last Rotated` defaults to today with the next due date derived from the frequency.
4. Press `Enter`. The import runs in the background, then a summary shows each key as imported, skipped, or failed.

```dotenv
# ~/project/.env
OPENAI_API_KEY=sk-...
ANTHROPIC_API_KEY=sk-ant-...
export DATABASE_URL="postgres://localhost/app"
```

What the importer does:

- Each valid `KEY=VALUE` line becomes a local secret titled with the variable name, and a Notion entry with that name, `Environment Variable`, and a `sqlite://<uuid>` reference.
- Values are written only to SQLite. Notion receives names and metadata, never the values, and the summary never echoes a secret.
- Supported syntax is simple `KEY=VALUE`, an optional leading `export `, and single- or double-quoted values. Blank values, invalid variable names, unterminated quotes, and duplicate keys in the same file are skipped without revealing the value.
- Re-running is safe: a local secret with the same title is linked instead of duplicated, and only entries already registered in Notion are skipped. A Notion failure is reported for that key while the local secret is kept, so fixing the error and re-running resumes where it stopped.

## How it works

```text
┌──────────────────────┐        ┌──────────────────────┐
│  Notion data source  │        │  Local SQLite        │
│                      │        │                      │
│  Name, Purpose,      │        │  secrets             │
│  Owner, Environment, │◀──────▶│  ├─ id               │
│  Rotation, Reference │        │  ├─ title            │
│  (metadata only)     │        │  ├─ secret (enc:v1:) │
└──────────────────────┘        │  └─ created_at       │
                                │  meta                │
                                │  ├─ notion-api-key   │
                                │  └─ encryption       │
                                └──────────────────────┘
```

Each Notion entry stores a `Credential Reference` such as `sqlite://<uuid>`. The app resolves that reference to a row in the `secrets` table. Secrets are never written to Notion.

Sensitive columns are stored as `enc:v1:<base64(nonce||ciphertext)>`. The app derives an AES-256-GCM cipher from the 32-byte master key in the Keychain, decrypts on read, and encrypts on write. On startup it re-encrypts any plaintext secret or Notion token inside a transaction and writes the `encryption=v1` marker to `meta`. The check runs on every open rather than once, so a plaintext row written by an older build is healed the next time the app starts.

## Requirements

- **Rust 1.85 or newer** (the crate uses the 2024 edition). Install with [rustup](https://rustup.rs/).
- A C compiler for the bundled SQLite build:
  - macOS: Xcode Command Line Tools (`xcode-select --install`)
  - Debian/Ubuntu: `sudo apt install build-essential`
- **Notion workspace and integration** with access to a database that has the properties listed below.
- **macOS Keychain** (default on macOS) or a `QUANTUMDATA_MASTER_KEY` environment variable for headless/CI use. See [Encryption at rest](#encryption-at-rest).
- macOS is the primary target. Linux builds, but clipboard support depends on `pbcopy`; without it copying fails unless you opt into the stdout fallback with `QUANTUMDATA_ALLOW_STDOUT_SECRET=1` (see [Data and security](#data-and-security)).

## 1. Create the Notion integration

1. Open <https://www.notion.so/my-integrations> and create a new internal integration.
2. Under **Capabilities**, enable:
   - Read content
   - Update content
   - Insert content
3. Copy the integration token. You will paste it into the app on first run; it is stored in SQLite, not in `.env`.

## 2. Prepare the Notion database

The app does not create the database for you. Build it once in Notion, then share it with the integration. Property names are matched exactly; extra properties are ignored.

### Create the database

1. In Notion, create a new page and choose **Database → Full page** (an inline database also works). Name it something like `API Credentials`.
2. Rename the default title property to `Name`.
3. Use the `+` button in the table header to add each property from the schema below, choosing the matching Notion type.
4. For the two `Date` properties, turn **Include time** off.

### Property schema

| Property | Notion type | Notes |
| --- | --- | --- |
| `Name` | Title | Credential display name |
| `Status` | Select | The app writes `Active` when creating a credential. Add `Inactive` and `Revoked` if you want to set them manually in Notion. |
| `Purpose` | Text | What the credential is for. Notion's UI calls it *Text*; the API type is `rich_text`. |
| `Credential Reference` | Text | Leave empty. The app writes `sqlite://<uuid>` here. |
| `Environment Variable` | Text | e.g. `OPENAI_API_KEY` |
| `Environment` | Select | Suggested options: `Local`, `Development`, `Staging`, `Production` |
| `Repository` | Text | e.g. `owner/repo` |
| `Owner` | Text | Person or team responsible |
| `Rotation Frequency` | Select | Suggested options: `Every 30 days`, `Every 90 days`, `Every 180 days`, `Yearly` |
| `Last Rotated` | Date | `YYYY-MM-DD`, time off |
| `Next Rotation Due` | Date | `YYYY-MM-DD`, time off |

Notes:

- If a select value you type is not already an option, add that option in Notion first. The app sends the exact text from the form.
- `Status` is only written on create; use Notion directly to mark a credential `Inactive` or `Revoked`.
- A wrong property type or a misspelled property name causes a Notion `validation_error`.
- The app ignores any extra properties you add.

### Share the database with the integration

1. Open the database as a full page in Notion.
2. Open the `•••` menu → **Connections** → **Connect to** → select your integration.
3. Confirm the integration can access the database.

## 3. Configure the application to use the new database

The app reads from two separate stores:

| Store | What it holds | Configured by |
| --- | --- | --- |
| Notion | Credential metadata | `NOTION_DATA_SOURCE_ID` |
| Local SQLite | Secret values and the Notion API key | `DB_PATH` |

Nothing in Notion points at the database you just built until you complete this section.

### Clone the repository

```bash
git clone https://github.com/Datanerdbytes/quantumdata-credential-manager.git
cd quantumdata-credential-manager
```

### Get the Notion data source ID

This project uses Notion API version `2025-09-03`, where pages live under a **data source**, not directly under a database. The app expects the data source ID, not the database page ID.

With the database shared to your integration, fetch its data sources:

```bash
export NOTION_TOKEN="secret_your_integration_token"
export DATABASE_ID="the_32_character_database_id_from_the_notion_url"

curl -s "https://api.notion.com/v1/databases/${DATABASE_ID}" \
  -H "Authorization: Bearer ${NOTION_TOKEN}" \
  -H "Notion-Version: 2025-09-03" \
  | jq '.data_sources[] | {id, name}'
```

Copy the `id` that belongs to the database you created in step 2. If you only have one, use the first entry. If `data_sources` is empty or the request returns `object_not_found`, the database has not been shared with the integration yet.

> Do not put the database page ID from the Notion URL into `NOTION_DATA_SOURCE_ID`. That ID is only used in the `curl` command above; the app needs the `data_sources[].id`.

### Create the `.env` file

```bash
cp .env.example .env
mkdir -p data
```

Edit `.env` and point the app at the new data source and local database:

```dotenv
# Required: data source ID from the previous step
NOTION_DATA_SOURCE_ID=xxxxxxxx-xxxx-xxxx-xxxx-xxxxxxxxxxxx

# Optional but recommended: absolute path so every launch uses the same SQLite file
DB_PATH=/absolute/path/to/quantumdata-credential-manager/data/credentials.db
```

Notes:

- The app loads `.env` from the current directory or one of its parents via `dotenvy`. Run it from the project directory, or export the variables in your shell.
- `DB_PATH` is the **local SQLite file**, not a Notion database. It defaults to `credentials.db` in the current working directory; use an absolute path to avoid accidentally creating a second store when launching from another folder.
- `NOTION_DATABASE_ID`, `NOTION_API_KEY_OP_REF`, and `ONEPASSWORD_VAULT` are legacy 1Password-era variables and are not used.
- `.env`, `data/`, and `credentials.db` are gitignored. Never commit them.

### Encryption at rest

On first launch the app creates a random 32-byte master key and stores it in the macOS Keychain:

```text
service: com.quantumdata.credentials
account: master-key
```

No configuration is required on macOS. If the database is already plaintext, it is migrated automatically — every row in `secrets` and the stored Notion token are encrypted in one transaction.

For Linux, containers, or scripted runs where the Keychain is unavailable, export a base64-encoded 32-byte key before launching:

```bash
export QUANTUMDATA_MASTER_KEY="$(openssl rand -base64 32)"
```

Store that value securely (for example in a secret manager) — without it the encrypted database cannot be opened. Do not put it in a committed `.env` file.

### Store the token and verify the connection

The integration token is not read from `.env`. It is stored in the SQLite `meta` table through the app:

1. Start the app (see [Build and run](#4-build-and-run)).
2. Choose **Store Notion API Key** and paste the integration token.
3. Choose **List Credentials**.

The status line should read `Successfully loaded N credentials from Notion.` — `N` is `0` for a brand-new database. If you see an authentication or `object_not_found` error, revisit the integration permissions and the data source ID.

### Switch to a different Notion database later

1. Get the new data source ID with the same `curl` command.
2. Update `NOTION_DATA_SOURCE_ID` in `.env`.
3. Restart the app.

If the new Notion database should use its own set of local secrets, also point `DB_PATH` at a different SQLite file. Otherwise the existing local secrets remain available through their `sqlite://<uuid>` references.

## 4. Build and run

Run directly from the source tree:

```bash
cargo run --release
```

Or install the binary once and launch it by name:

```bash
cargo install --path . --force
api-manager
```

`cargo install` places `api-manager` in `~/.cargo/bin`. Make sure that directory is on your `PATH`.

On first launch:

1. Wait for the QuantumData splash, or press any key to skip it.
2. Choose **Store Notion API Key** if you have not stored the token yet.
3. Choose **List Credentials** to confirm the data source is reachable.
4. Choose **Import .env File** to onboard an existing project, or **Add Credential** to create your first entry.

## Usage

### Menu

| Key | Action |
| --- | --- |
| `↑` / `↓` | Move the highlight |
| `Enter` / `Space` | Open the highlighted action |
| `q` | Quit from the main menu |

### Forms

| Key | Action |
| --- | --- |
| Any character | Edit the active field |
| `Tab` / `Shift+Tab` | Next / previous field |
| `↑` / `↓` | Previous / next field |
| `Enter` | Continue or save |
| `Esc` | Go back without saving |

Dates must use `YYYY-MM-DD` (an ISO timestamp such as `2026-01-01T10:00:00Z` is also accepted).

`Last Rotated` defaults to today when you add a credential, and `Next Rotation Due` is derived from `Last Rotated + Rotation Frequency` when left empty. Supported frequencies include `Every N days`, `Every N weeks`, `Every N months`, and `Yearly`. An explicit `Next Rotation Due` always wins.

### Lists and details

| Key | Action |
| --- | --- |
| `↑` / `↓` | Navigate or scroll |
| `Enter` | Open the selected item |
| `Esc` | Go back |
| `r` | Refresh the Notion list (List screen only) |
| `c` | Copy the secret to the clipboard (Details screen only) |
| `q` | Return to the main menu |

### Credential flows

- **Add Credential**
  - *Create a new credential* — enter metadata, then the secret. The app creates the SQLite row first, then the Notion page.
  - *Use an existing credential* — pick a secret already in SQLite and register new metadata in Notion. This is the working path; the standalone **Register Existing Credential** menu entry is still a placeholder.
- **Import .env File** — enter a path (for example `~/project/.env`) plus optional environment, owner, repository, and rotation frequency. The app imports every valid `KEY=VALUE` line: the secret goes to SQLite titled with the variable name, and the metadata is registered in Notion. Blank values, invalid variable names, file duplicates, and entries already registered in Notion are skipped. If a local secret with the same title exists but has no Notion entry, the importer links it instead of creating a duplicate, so re-running after fixing an error resumes where it stopped. Notion failures are reported per key without stopping the rest.
- **Search / Retrieve Credential** — filter as you type across name, service, username, owner, environment, repository, and environment variable. Open details, press `c` to copy.
- **List Credentials** — browse everything in Notion; press `r` to refresh.
- **Update Credential** — choose an entry, edit the prefilled metadata, then optionally type a replacement secret. Leave the secret field empty and press `Enter` to keep the current secret.
- **Rotate Credential** — choose an entry, type the new secret (hidden), and press `Enter`. The app updates SQLite and Notion, sets `Last Rotated` to today, and derives the next `Next Rotation Due` from the rotation frequency. The list refreshes in the background afterward.
- **Rotation reminders** — when a credential is due within 7 days (or overdue), the menu shows a banner listing the most urgent entries. A single aggregated macOS notification is delivered at most once per day; enable notifications for Script Editor or Terminal if macOS suppresses them.
- **Store / Delete Notion API Key** — manage the integration token in SQLite.
- **Delete Credential** — archives the Notion page and deletes the local secret. If the stored reference is stale, the app falls back to a unique case-insensitive title match; if several secrets share that title, it refuses to guess.

## Data and security

- **Secrets are encrypted at rest.** `secrets.secret` and the Notion API key in `meta.value` are sealed with AES-256-GCM under a random 32-byte master key. The key is stored in the macOS Keychain (`com.quantumdata.credentials` / `master-key`), or supplied via `QUANTUMDATA_MASTER_KEY` (base64) on systems without a Keychain. Losing the key makes the database unrecoverable.
- **Encryption is per-value and authenticated.** Each value gets a fresh 96-bit nonce and uses `quantumdata:sqlite:v1` as associated data, so tampering with the ciphertext fails the read instead of returning corrupted data.
- **Plaintext backups are not created automatically.** Before the first encrypted startup, copy the database yourself if you want a rollback point; any such copy stays plaintext until you delete it. Set permissions on it:
  ```bash
  chmod 600 data/credentials.db
  ```
  Prefer full-disk encryption (FileVault on macOS) and restrict access to your user account.
- **Notion never receives secret values.** Only metadata and the `sqlite://<uuid>` reference are sent.
- **Rotation alerts run locally.** The app computes due dates in-process and sends a notification through `osascript`; no credential data leaves the machine. The date of the last notification is stored in `meta` so the alert is not repeated the same day.
- **Clipboard hygiene.** A successful copy schedules a clear 45 seconds later. The clear only runs if the clipboard still holds the same secret, and it only runs while the app is open. Clipboard managers may keep their own copy of the value, so disable history for secret copies if that matters to you.
- **Stdout fallback is opt-in.** If `pbcopy` is unavailable (for example inside a container or on Linux), copying fails by default. Set `QUANTUMDATA_ALLOW_STDOUT_SECRET=1` to print the secret to the terminal instead; avoid doing so while screen sharing or recording.
- **`.env` import stays local until registration.** Imported values are written to SQLite only. Notion receives variable names and metadata, never the values. If a Notion registration fails, the local secret remains and the summary lists the key.
- **Back up** `data/credentials.db` (and the master key) if you care about the stored secrets. A database copy without its key cannot be restored. Notion holds only the metadata.
- **Do not commit** `.env`, `data/`, or `credentials.db`; they are already listed in `.gitignore`.

## Troubleshooting

| Symptom | Fix |
| --- | --- |
| `Missing NOTION_DATA_SOURCE_ID environment variable` | Create `.env` from `.env.example`, set the data source ID, and launch from the project directory (or export the variable). |
| `Notion API key not found in database. Run the app to store it first.` | Use **Store Notion API Key** in the menu. |
| `The SQLite database ... is encrypted. Start the app normally so it can load the master key.` | The database was opened without a key (for example by a script using the plaintext path). Launch `api-manager` normally, or export `QUANTUMDATA_MASTER_KEY`. |
| `Failed to load the master key from the macOS Keychain` | Unlock the login keychain and approve access, or export `QUANTUMDATA_MASTER_KEY` for headless runs. |
| `The stored value is encrypted but no master key is loaded.` | The database is encrypted but the key is missing or different. Use the original key; a different key cannot decrypt it. |
| Rotation notification never appears | Allow notifications for Script Editor/Terminal in **System Settings → Notifications**, and check the menu banner — it is the authoritative in-app alert. |
| `Notion API error (object_not_found)` | The integration lacks access, or the ID is wrong. Share the database with the integration and use the data source ID, not the database page ID. |
| `Notion API error (validation_error)` | A property name or type does not match the table in [Prepare the Notion database](#2-prepare-the-notion-database), or a select/date value is invalid. |
| `No secret found for reference ...` | The Notion reference points to a missing row. Correct the `Credential Reference`, or rely on the title fallback when the title is unique. |
| Credential disappears between launches | Two databases are in use. Set an absolute `DB_PATH` and remove stray `credentials.db` files. |
| Delete says the title is ambiguous | Multiple local secrets share that name. Rename one or fix the `Credential Reference` in Notion. |
| `Clipboard unavailable (pbcopy not found)` | Run the app directly on macOS, or set `QUANTUMDATA_ALLOW_STDOUT_SECRET=1` to print the secret to the terminal instead. Clear the terminal afterwards. |
| Clipboard still contains the secret after a minute | The auto-clear only runs while the app is open, and only if the value is unchanged. Copy something else or clear it manually if the app was closed. |
| Import reports `Could not read ...` | Use an absolute path or `~/...`; the path is resolved from the app's working directory. |
| Import reports `already registered in Notion` for some keys | A credential with that name or reference is already in Notion. Rename it or remove the line from the file. |
| Import reports `Notion API key not found` | Store the Notion token first, then re-run the import. Local secrets created by the failed run are linked to new Notion entries instead of being duplicated. |
| Splash slows down scripts | It lasts 1.5 seconds and exits on any key. |

## Development

```bash
cargo fmt
cargo test
```

The suite currently contains 80 tests covering metadata mapping, update property clearing, SQLite updates and deletes, encryption and migration, rotation parsing and alerts, and TUI state transitions. Tests use temporary databases and do not touch your real database or the Notion API.

Project layout:

```text
src/
├── main.rs              # Entry point
├── cli.rs               # Terminal UI, screens, splash, background loads
├── config.rs            # .env loading and database path
├── crypto.rs            # AES-256-GCM cipher and Keychain master key
├── env_import.rs        # .env parsing and validation
├── rotation.rs          # Due dates, frequencies, and alert formatting
├── models/              # Credential model
├── notion/
│   ├── client.rs        # Notion HTTP client (API 2025-09-03)
│   └── credentials.rs   # Notion property mapping and CRUD helpers
└── secrets.rs           # Encrypted SQLite storage and clipboard integration
```

## Limitations

- The macOS notification is best-effort; the in-app banner is the reliable rotation alert.
- Clipboard clearing only happens while the app is open. If `pbcopy` is missing, you must opt in to the stdout fallback with `QUANTUMDATA_ALLOW_STDOUT_SECRET=1`; there is no automatic fallback.
- `.env` import supports simple `KEY=VALUE` files only. It does not evaluate `$(...)`, expand `${VAR}` references, join multi-line values, or read shell profiles. There is no export command.
- The standalone **Register Existing Credential** menu entry is a placeholder; register a local secret through **Add Credential → Use an existing credential**.

## License

Released under the [MIT License](LICENSE).

Copyright (c) 2026 QuantumEcho
