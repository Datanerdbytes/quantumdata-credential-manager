use std::io;

use crossterm::{
    event::{self, Event, KeyCode, KeyEventKind},
    execute,
    terminal::{EnterAlternateScreen, LeaveAlternateScreen, disable_raw_mode, enable_raw_mode},
};

use ratatui::{
    Frame, Terminal,
    backend::CrosstermBackend,
    layout::{Constraint, Direction, Layout, Rect},
    style::{Color, Modifier, Style},
    text::{Line, Span},
    widgets::{Block, Borders, List, ListItem, ListState, Paragraph, Wrap},
};

use crate::models::Credential;
use crate::notion::{NewCredential, create_credential};
use crate::secrets::{Database, SecretItem};

const MENU_ITEMS: [&str; 10] = [
    "Add Credential",
    "Search / Retrieve Credential",
    "List Credentials",
    "Update Credential Reference",
    "Register Existing Credential",
    "Rotate Credential",
    "Store Notion API Key",
    "Delete Notion API Key",
    "Delete Credential",
    "Exit",
];

const ADD_CHOICES: [&str; 2] = [
    "Create a new credential",
    "Use an existing credential",
];

const ADD_FIELDS: [&str; 9] = [
    "Name",
    "Purpose",
    "Environment Variable",
    "Environment",
    "Repository",
    "Owner",
    "Rotation Frequency",
    "Last Rotated",
    "Next Rotation Due",
];

const REQUIRED_ADD_FIELDS: usize = 1;

const BG: Color = Color::Rgb(11, 18, 32);
const PANEL_BG: Color = Color::Rgb(5, 8, 15);
const BORDER: Color = Color::Rgb(34, 48, 74);
const TEXT: Color = Color::Rgb(230, 237, 247);
const MUTED: Color = Color::Rgb(147, 164, 189);
const ACCENT: Color = Color::Rgb(126, 226, 168);
const AMBER: Color = Color::Rgb(240, 198, 116);
const DANGER: Color = Color::Rgb(255, 105, 105);
const BADGE_ACTIVE_BG: Color = Color::Rgb(18, 40, 28);
const BADGE_INACTIVE_BG: Color = Color::Rgb(46, 36, 18);
const BADGE_REVOKED_BG: Color = Color::Rgb(48, 20, 20);
const SELECT_BG: Color = Color::Rgb(27, 42, 68);

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Screen {
    Menu,
    Add,
    AddDetails,
    AddSecret,
    AddExisting,
    AddResult,
    Search,
    List,
    Details,
    Update,
    Register,
    Rotate,
    SetNotionApiKey,
    DeleteNotionApiKey,
    DeleteCredential,
}

impl Screen {
    fn title(self) -> &'static str {
        match self {
            Screen::Menu => "CREDENTIAL OPERATIONS",
            Screen::Add => "ADD CREDENTIAL",
            Screen::AddDetails => "ADD CREDENTIAL · METADATA",
            Screen::AddSecret => "ADD CREDENTIAL · SECRET",
            Screen::AddExisting => "SELECT CREDENTIAL",
            Screen::AddResult => "ADD CREDENTIAL · RESULT",
            Screen::Search => "SEARCH / RETRIEVE",
            Screen::List => "CREDENTIALS",
            Screen::Details => "CREDENTIAL DETAILS",
            Screen::Update => "UPDATE CREDENTIAL REFERENCE",
            Screen::Register => "REGISTER CREDENTIAL",
            Screen::Rotate => "ROTATE CREDENTIAL",
            Screen::SetNotionApiKey => "STORE NOTION API KEY",
            Screen::DeleteNotionApiKey => "DELETE NOTION API KEY",
            Screen::DeleteCredential => "DELETE CREDENTIAL",
        }
    }

    fn hint(self) -> &'static str {
        match self {
            Screen::Menu => "Use ↑ / ↓ to navigate · Enter to open · q to quit",
            Screen::Add => "↑ / ↓ to choose · Enter to continue · Esc to go back",
            Screen::AddDetails => {
                "Type to edit · Tab / ↑ / ↓ switches fields · Enter to continue · Esc to go back"
            }
            Screen::AddSecret => {
                "Type the API secret (hidden) · Enter creates the item · Esc discards and goes back"
            }
            Screen::AddExisting => "↑ / ↓ Navigate · Enter select · Esc Back",
            Screen::AddResult => "Esc Back to menu",
            Screen::Search => "Type to filter · ↑ / ↓ select · Enter view details · Esc to go back",
            Screen::List => "↑ / ↓ Navigate · Enter View details · r Refresh · Esc Back",
            Screen::Details => "↑ / ↓ Scroll · c Copy secret · Esc Back · q Menu",
            Screen::Update => {
                "Type to edit · Tab / ↑ / ↓ switches fields · Enter to save · Esc to go back"
            }
            Screen::Register => {
                "Type to edit · Tab / ↑ / ↓ switches fields · Enter to save · Esc to go back"
            }
            Screen::Rotate => "↑ / ↓ to choose · Enter to rotate · Esc to go back",
            Screen::SetNotionApiKey => {
                "Type to edit · Enter to save · Esc to go back"
            }
            Screen::DeleteNotionApiKey => {
                "Type DELETE to confirm · Enter to delete · Esc to go back"
            }
            Screen::DeleteCredential => {
                "↑ / ↓ Navigate · Enter delete · Esc to go back"
            }
        }
    }

    fn is_form(self) -> bool {
        matches!(
            self,
            Screen::AddDetails | Screen::Search | Screen::Update | Screen::Register | Screen::SetNotionApiKey | Screen::DeleteNotionApiKey
        )
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
enum PendingAction {
    CopySecret,
    LoadItems,
    CreateItem,
    RegisterExisting { item_id: String, item_title: String },
}

struct Form {
    fields: Vec<(&'static str, String)>,
    active: usize,
}

impl Form {
    fn new(labels: &[&'static str]) -> Self {
        Self {
            fields: labels.iter().map(|label| (*label, String::new())).collect(),
            active: 0,
        }
    }

    fn value(&self, index: usize) -> &str {
        self.fields
            .get(index)
            .map(|(_, value)| value.as_str())
            .unwrap_or("")
    }

    fn next(&mut self) {
        if !self.fields.is_empty() {
            self.active = (self.active + 1) % self.fields.len();
        }
    }

    fn previous(&mut self) {
        if !self.fields.is_empty() {
            self.active = (self.active + self.fields.len() - 1) % self.fields.len();
        }
    }

    fn push(&mut self, character: char) {
        if let Some((_, value)) = self.fields.get_mut(self.active) {
            value.push(character);
        }
    }

    fn backspace(&mut self) {
        if let Some((_, value)) = self.fields.get_mut(self.active) {
            value.pop();
        }
    }
}

struct Dashboard {
    screen: Screen,
    selected: usize,
    status: String,
    form: Form,
    credentials: Vec<Credential>,
    selected_credential: usize,
    search_selected: usize,
    details_offset: usize,
    details_origin: Screen,
    pending_action: Option<PendingAction>,
    add_choice: usize,
    secret_input: String,
    items: Vec<SecretItem>,
    item_selected: usize,
    result_lines: Vec<String>,
    database: Database,
}

impl Dashboard {
    fn new(database: Database) -> Self {
        Self {
            screen: Screen::Menu,
            selected: 0,
            status: Screen::Menu.hint().to_string(),
            form: Form::new(&[]),
            credentials: Vec::new(),
            selected_credential: 0,
            search_selected: 0,
            details_offset: 0,
            details_origin: Screen::List,
            pending_action: None,
            add_choice: 0,
            secret_input: String::new(),
            items: Vec::new(),
            item_selected: 0,
            result_lines: Vec::new(),
            database,
        }
    }

    fn load_credentials(&mut self) -> bool {
        match crate::notion::list_credentials(&self.database) {
            Ok(credentials) => {
                self.credentials = credentials;
                self.selected_credential = 0;
                self.search_selected = 0;
                true
            }
            Err(error) => {
                self.status = format!("Failed to load credentials: {error:#}");
                false
            }
        }
    }

    fn open(&mut self, screen: Screen) {
        self.screen = screen;
        self.status = screen.hint().to_string();
        self.form = match screen {
            Screen::Add => {
                self.add_choice = 0;
                self.secret_input.clear();
                self.items.clear();
                self.item_selected = 0;
                Form::new(&[])
            }
            Screen::AddDetails => Form::new(&ADD_FIELDS),
            Screen::Search => Form::new(&["Query"]),
            Screen::Update => Form::new(&["Credential name", "New credential reference"]),
            Screen::Register => Form::new(&["Existing credential", "Notion name"]),
            Screen::SetNotionApiKey => Form::new(&["Notion API Key"]),
            Screen::DeleteNotionApiKey => Form::new(&["Type DELETE to confirm"]),
            _ => Form::new(&[]),
        };

        let loaded = matches!(
            screen,
            Screen::List | Screen::Search | Screen::Rotate | Screen::DeleteCredential
        )
        && self.load_credentials();

        if screen == Screen::Search {
            self.search_selected = 0;
        }

        if screen == Screen::List && loaded {
            self.status = format!(
                "Successfully loaded {} credentials from Notion.",
                self.credentials.len()
            );
        } else if screen == Screen::Rotate && loaded && self.credentials.is_empty() {
            self.status = "No credentials available to rotate.".to_string();
        } else if screen == Screen::DeleteCredential && loaded && self.credentials.is_empty() {
            self.status = "No credentials available to delete.".to_string();
        }
    }

    fn back_to_menu(&mut self) {
        self.screen = Screen::Menu;
        self.form = Form::new(&[]);
        self.secret_input.clear();
        self.items.clear();
        self.result_lines.clear();
    }

    fn search_indices(&self) -> Vec<usize> {
        let query = self.form.value(0).trim().to_lowercase();
        self.credentials
            .iter()
            .enumerate()
            .filter(|(_, credential)| {
                query.is_empty()
                    || credential.name.to_lowercase().contains(&query)
                    || credential
                        .service
                        .as_deref()
                        .is_some_and(|service| service.to_lowercase().contains(&query))
                    || credential
                        .username
                        .as_deref()
                        .is_some_and(|username| username.to_lowercase().contains(&query))
            })
            .map(|(index, _)| index)
            .collect()
    }

    fn search_matches(&self) -> Vec<&Credential> {
        self.search_indices()
            .into_iter()
            .filter_map(|index| self.credentials.get(index))
            .collect()
    }

    fn select_search_result(&mut self, delta: isize) {
        let indices = self.search_indices();
        if indices.is_empty() {
            return;
        }
        let last = indices.len() - 1;
        let next = (self.search_selected as isize + delta).clamp(0, last as isize) as usize;
        self.search_selected = next;
        let index = indices[next];
        self.status = format!("Selected: {}", self.credentials[index].name);
    }

    fn select_credential(&mut self, delta: isize) {
        if self.credentials.is_empty() {
            return;
        }
        let last = self.credentials.len() - 1;
        let next = (self.selected_credential as isize + delta).clamp(0, last as isize) as usize;
        self.selected_credential = next;
        self.status = format!("Selected: {}", self.credentials[next].name);
    }

    fn take_pending_action(&mut self) -> Option<PendingAction> {
        self.pending_action.take()
    }

    fn perform_action(&mut self, action: PendingAction) {
        match action {
            PendingAction::CopySecret => self.copy_selected_secret(),
            PendingAction::LoadItems => self.load_items(),
            PendingAction::CreateItem => self.create_new_credential(),
            PendingAction::RegisterExisting {
                item_id,
                item_title,
            } => self.register_existing(&item_id, &item_title),
        }
    }

    fn select_add_choice(&mut self, delta: isize) {
        let last = ADD_CHOICES.len() - 1;
        self.add_choice = (self.add_choice as isize + delta).clamp(0, last as isize) as usize;
        self.status = format!("Selected: {}", ADD_CHOICES[self.add_choice]);
    }

    fn select_item(&mut self, delta: isize) {
        if self.items.is_empty() {
            return;
        }
        let last = self.items.len() - 1;
        self.item_selected = (self.item_selected as isize + delta).clamp(0, last as isize) as usize;
        self.status = format!("Selected: {}", self.items[self.item_selected].title);
    }

    fn load_items(&mut self) {
        match self.database.list_items() {
            Ok(items) => {
                self.items = items;
                self.item_selected = 0;
                self.screen = Screen::AddExisting;
                self.status = if self.items.is_empty() {
                    "No credentials found in database.".to_string()
                } else {
                    format!(
                        "Found {} credentials. Select one with ↑ / ↓ and press Enter.",
                        self.items.len()
                    )
                };
            }
            Err(error) => {
                self.status = format!("Could not list credentials: {error:#}");
            }
        }
    }

    fn create_new_credential(&mut self) {
        let name = self.form.value(0).trim().to_string();

        let mut secret = std::mem::take(&mut self.secret_input);
        let item_id = match self.database.create_item(&name, None, &secret) {
            Ok(id) => id,
            Err(error) => {
                self.screen = Screen::AddSecret;
                secret.clear();
                drop(secret);
                self.status = format!(
                    "Credential creation failed. No Notion entry was created. ({error:#})"
                );
                return;
            }
        };
        secret.clear();
        drop(secret);

        // Use sqlite://<id> as the reference format for SQLite-stored credentials
        let reference = format!("sqlite://{}", item_id);

        let credential = self.credential_from_form(reference.clone());

        match create_credential(&credential, &self.database) {
            Ok(()) => {
                self.finish_add();
                self.status = format!("Created '{name}' in database and registered it in Notion.");
            }
            Err(error) => {
                self.show_add_result(vec![
                    "The credential was created, but Notion registration failed.".to_string(),
                    String::new(),
                    format!("Credential     {name}"),
                    format!("Database ID    {item_id}"),
                    format!("Reference      {reference}"),
                    format!("Error          {error:#}"),
                    String::new(),
                    "Retry Notion registration using the reference above.".to_string(),
                ]);
            }
        }
    }

    fn register_existing(&mut self, item_id: &str, item_title: &str) {
        // Use sqlite://<id> as the reference format for SQLite-stored credentials
        let reference = format!("sqlite://{}", item_id);

        let credential = self.credential_from_form(reference.clone());
        let name = credential.name.clone();

        match create_credential(&credential, &self.database) {
            Ok(()) => {
                self.finish_add();
                self.status =
                    format!("Registered '{name}' with existing credential '{item_title}'.");
            }
            Err(error) => {
                self.show_add_result(vec![
                    "The existing credential was left unchanged, but Notion registration failed."
                        .to_string(),
                    String::new(),
                    format!("Credential     {name}"),
                    format!("Existing item  {item_title}"),
                    format!("Reference      {reference}"),
                    format!("Error          {error:#}"),
                    String::new(),
                    "Retry Notion registration using the reference above.".to_string(),
                ]);
            }
        }
    }

    fn show_add_result(&mut self, lines: Vec<String>) {
        self.finish_add();
        self.result_lines = lines;
        self.screen = Screen::AddResult;
        self.status = Screen::AddResult.hint().to_string();
    }

    fn finish_add(&mut self) {
        self.back_to_menu();
        self.status = Screen::Menu.hint().to_string();
        self.add_choice = 0;
    }

    fn credential_from_form(&self, reference: String) -> NewCredential {
        NewCredential {
            name: self.form.value(0).trim().to_string(),
            purpose: self.form.value(1).trim().to_string(),
            credential_reference: reference,
            environment_variable: self.form.value(2).trim().to_string(),
            environment: self.form.value(3).trim().to_string(),
            repository: self.form.value(4).trim().to_string(),
            owner: self.form.value(5).trim().to_string(),
            rotation_frequency: self.form.value(6).trim().to_string(),
            last_rotated: optional_value(self.form.value(7)),
            next_rotation_due: optional_value(self.form.value(8)),
        }
    }

    fn copy_selected_secret(&mut self) {
        let Some(credential) = self.credentials.get(self.selected_credential) else {
            self.status = "No credential selected.".to_string();
            return;
        };

        let reference = credential
            .credential_reference
            .as_deref()
            .map(str::trim)
            .filter(|reference| !reference.is_empty());

        let Some(reference) = reference else {
            self.status = "No credential reference configured for this credential.".to_string();
            return;
        };

        match self.database.copy_secret_to_clipboard(reference) {
            Ok(()) => self.status = "Secret output to terminal.".to_string(),
            Err(error) => {
                self.status = format!("Could not copy credential: {error:#}");
            }
        }
    }

    fn submit(&mut self) {
        match self.screen {
            Screen::Menu => {}
            Screen::Search => {
                let indices = self.search_indices();
                if indices.is_empty() {
                    self.status = "No credentials match that query.".to_string();
                    return;
                }
                let position = self.search_selected.min(indices.len() - 1);
                let index = indices[position];
                self.search_selected = position;
                self.selected_credential = index;
                self.details_offset = 0;
                self.details_origin = Screen::Search;
                self.screen = Screen::Details;
                self.status = Screen::Details.hint().to_string();
            }
            Screen::List => {
                if self.credentials.is_empty() {
                    self.status = "No credentials available.".to_string();
                } else {
                    let index = self.selected_credential.min(self.credentials.len() - 1);
                    self.selected_credential = index;
                    self.details_offset = 0;
                    self.details_origin = Screen::List;
                    self.screen = Screen::Details;
                    self.status = Screen::Details.hint().to_string();
                }
            }
            Screen::Add => {
                if self.form.fields.len() != ADD_FIELDS.len() {
                    self.form = Form::new(&ADD_FIELDS);
                }
                self.screen = Screen::AddDetails;
                self.status = Screen::AddDetails.hint().to_string();
            }
            Screen::AddDetails => {
                for (index, label) in ADD_FIELDS.iter().take(REQUIRED_ADD_FIELDS).enumerate() {
                    if self.form.value(index).trim().is_empty() {
                        self.status = format!("{label} is required.");
                        return;
                    }
                }

                for index in [7, 8] {
                    let value = self.form.value(index).trim();
                    if !value.is_empty() && !is_date_like(value) {
                        self.status = format!("{} must use YYYY-MM-DD format.", ADD_FIELDS[index]);
                        return;
                    }
                }

                if self.add_choice == 0 {
                    self.screen = Screen::AddSecret;
                    self.status = Screen::AddSecret.hint().to_string();
                } else {
                    self.pending_action = Some(PendingAction::LoadItems);
                    self.status = "Loading credentials from database…".to_string();
                }
            }
            Screen::AddSecret => {
                if self.secret_input.is_empty() {
                    self.status = "The API secret is required.".to_string();
                    return;
                }
                self.pending_action = Some(PendingAction::CreateItem);
                self.status = "Creating the credential and the Notion entry…".to_string();
            }
            Screen::AddExisting => {
                if self.items.is_empty() {
                    self.status = "No credentials available to select.".to_string();
                    return;
                }
                let index = self.item_selected.min(self.items.len() - 1);
                let item = self.items[index].clone();
                self.pending_action = Some(PendingAction::RegisterExisting {
                    item_id: item.id,
                    item_title: item.title,
                });
                self.status = "Registering the selected credential in Notion…".to_string();
            }
            Screen::Update => {
                let name = self.form.value(0).trim().to_string();
                let reference = self.form.value(1).trim().to_string();
                if name.is_empty() || reference.is_empty() {
                    self.status = "Credential name and reference are required.".to_string();
                    return;
                }
                self.status = format!("Update '{name}': Notion/database are not connected yet.");
                self.back_to_menu();
            }
            Screen::Register => {
                let item = self.form.value(0).trim().to_string();
                let name = self.form.value(1).trim().to_string();
                if item.is_empty() || name.is_empty() {
                    self.status = "Credential and Notion name are required.".to_string();
                    return;
                }
                self.status = format!("Register '{name}': Notion/database are not connected yet.");
                self.back_to_menu();
            }
            Screen::Rotate => {
                if self.credentials.is_empty() {
                    self.status = "No credentials available to rotate.".to_string();
                    return;
                }
                let index = self.selected_credential.min(self.credentials.len() - 1);
                let name = self.credentials[index].name.clone();
                self.status = format!("Rotate '{name}': Notion/database are not connected yet.");
            }
            Screen::SetNotionApiKey => {
                let key = self.form.value(0).trim();
                if key.is_empty() {
                    self.status = "Notion API Key is required.".to_string();
                    return;
                }
                match self.database.set_notion_api_key(key) {
                    Ok(()) => {
                        self.back_to_menu();
                        self.status = "Notion API Key stored successfully.".to_string();
                    }
                    Err(error) => {
                        self.status = format!("Failed to store Notion API Key: {error:#}");
                    }
                }
            }
            Screen::DeleteNotionApiKey => {
                let confirmation = self.form.value(0).trim();
                if confirmation != "DELETE" {
                    self.status = "Type DELETE to confirm removal.".to_string();
                    return;
                }
                match self.database.delete_notion_api_key() {
                    Ok(()) => {
                        self.back_to_menu();
                        self.status = "Notion API Key deleted from database.".to_string();
                    }
                    Err(error) => {
                        self.status = format!("Failed to delete Notion API Key: {error:#}");
                    }
                }
            }
    Screen::DeleteCredential => {
        if self.credentials.is_empty() {
            self.status = "No credentials available to delete.".to_string();
            return;
        }
        let index = self.selected_credential.min(self.credentials.len() - 1);
        let reference = match &self.credentials[index].credential_reference {
            Some(ref_) => ref_.clone(),
            None => {
                self.status = "Credential has no reference to delete.".to_string();
                return;
            }
        };
        let name = self.credentials[index].name.clone();
        match self.database.delete_item_by_reference(&reference) {
            Ok(()) => {
                self.status = format!("Deleted '{name}' from database.");
                self.load_credentials();
                if self.credentials.is_empty() {
                    self.selected_credential = 0;
                } else if index >= self.credentials.len() {
                    self.selected_credential = self.credentials.len() - 1;
                }
            }
            Err(error) => {
                self.status = format!("Failed to delete '{name}': {error:#}");
            }
        }
    }
    Screen::Details => {}
            Screen::AddResult => {}
        }
    }

    fn handle_key(&mut self, code: KeyCode) -> bool {
        if self.screen == Screen::Menu {
            return self.handle_menu_key(code);
        }

        match code {
            KeyCode::Esc => match self.screen {
                Screen::Details => {
                    self.screen = match self.details_origin {
                        Screen::Search => Screen::Search,
                        _ => Screen::List,
                    };
                    self.status = self.screen.hint().to_string();
                }
                Screen::AddSecret => {
                    self.secret_input.clear();
                    self.screen = Screen::AddDetails;
                    self.status = "Secret entry cancelled. The secret was discarded.".to_string();
                }
                Screen::AddExisting => {
                    self.items.clear();
                    self.screen = Screen::AddDetails;
                    self.status = Screen::AddDetails.hint().to_string();
                }
                Screen::AddDetails => {
                    self.secret_input.clear();
                    self.screen = Screen::Add;
                    self.status = Screen::Add.hint().to_string();
                }
                _ => {
                    self.back_to_menu();
                    self.status = Screen::Menu.hint().to_string();
                }
            },
            KeyCode::Tab => {
                if self.screen.is_form() {
                    self.form.next();
                }
            }
            KeyCode::BackTab => {
                if self.screen.is_form() {
                    self.form.previous();
                }
            }
            KeyCode::Up => match self.screen {
                Screen::Search => self.select_search_result(-1),
                Screen::Add => self.select_add_choice(-1),
                Screen::AddExisting => self.select_item(-1),
                Screen::Details => self.details_offset = self.details_offset.saturating_sub(1),
                Screen::AddSecret => {}
                Screen::AddResult => {}
                _ if self.screen.is_form() => self.form.previous(),
                _ => self.select_credential(-1),
            },
            KeyCode::Down => match self.screen {
                Screen::Search => self.select_search_result(1),
                Screen::Add => self.select_add_choice(1),
                Screen::AddExisting => self.select_item(1),
                Screen::Details => self.details_offset += 1,
                Screen::AddSecret => {}
                Screen::AddResult => {}
                _ if self.screen.is_form() => self.form.next(),
                _ => self.select_credential(1),
            },
            KeyCode::Backspace => {
                if self.screen == Screen::AddSecret {
                    self.secret_input.pop();
                } else if self.screen.is_form() {
                    self.form.backspace();
                    if self.screen == Screen::Search {
                        self.search_selected = 0;
                    }
                }
            }
            KeyCode::Char(character) => {
                if self.screen == Screen::AddSecret {
                    self.secret_input.push(character);
                } else if self.screen.is_form() {
                    self.form.push(character);
                    if self.screen == Screen::Search {
                        self.search_selected = 0;
                    }
                } else if self.screen == Screen::List && (character == 'r' || character == 'R') {
                    if self.load_credentials() {
                        self.status = format!(
                            "Successfully loaded {} credentials from Notion.",
                            self.credentials.len()
                        );
                    }
                } else if self.screen == Screen::Details && (character == 'c' || character == 'C') {
                    self.pending_action = Some(PendingAction::CopySecret);
                } else if character == 'q' || character == 'Q' {
                    self.back_to_menu();
                    self.status = Screen::Menu.hint().to_string();
                }
            }
            KeyCode::Enter => self.submit(),
            _ => {}
        }
        false
    }

    fn handle_menu_key(&mut self, code: KeyCode) -> bool {
        match code {
            KeyCode::Up => {
                self.selected = self.selected.saturating_sub(1);
                self.status = format!("Highlighted: {}", MENU_ITEMS[self.selected]);
            }
            KeyCode::Down => {
                self.selected = (self.selected + 1).min(MENU_ITEMS.len() - 1);
                self.status = format!("Highlighted: {}", MENU_ITEMS[self.selected]);
            }
            KeyCode::Enter | KeyCode::Char(' ') => {
                let screen = match self.selected {
                    0 => Screen::Add,
                    1 => Screen::Search,
                    2 => Screen::List,
                    3 => Screen::Update,
                    4 => Screen::Register,
                    5 => Screen::Rotate,
                    6 => Screen::SetNotionApiKey,
                    7 => Screen::DeleteNotionApiKey,
                    8 => Screen::DeleteCredential,
                    _ => return true,
                };
                self.open(screen);
            }
            KeyCode::Char('q') | KeyCode::Char('Q') => return true,
            _ => {}
        }
        false
    }

    fn render(&mut self, frame: &mut Frame) {
        let area = frame.area();
        frame.render_widget(Block::default().style(Style::default().bg(BG)), area);

        let inner = Rect {
            x: area.x + 1,
            y: area.y + 1,
            width: area.width.saturating_sub(2),
            height: area.height.saturating_sub(2),
        };

        let footer_height = 3.min(inner.height);
        let body_floor = 5.min(inner.height.saturating_sub(footer_height));
        let header_height = 6.min(inner.height.saturating_sub(footer_height + body_floor));
        let body_height = inner.height.saturating_sub(header_height + footer_height);

        let layout = Layout::default()
            .direction(Direction::Vertical)
            .constraints([
                Constraint::Length(header_height),
                Constraint::Length(body_height),
                Constraint::Length(footer_height),
            ])
            .split(inner);

        render_header(frame, layout[0]);

        match self.screen {
            Screen::Menu => {
                let items = MENU_ITEMS
                    .iter()
                    .map(|item| ListItem::new(format!("  {item}")))
                    .collect();
                render_selectable_list(
                    frame,
                    layout[1],
                    self.screen.title(),
                    items,
                    self.selected,
                    None,
                );
            }
            Screen::Add => self.render_add_choice(frame, layout[1]),
            Screen::AddDetails => self.render_form(
                frame,
                layout[1],
                "Secrets stay in local SQLite database; Notion stores metadata.",
            ),
            Screen::AddSecret => self.render_secret(frame, layout[1]),
            Screen::AddExisting => self.render_add_existing(frame, layout[1]),
            Screen::AddResult => self.render_add_result(frame, layout[1]),
            Screen::Search => self.render_search(frame, layout[1]),
            Screen::List => {
                let items = credential_items(&self.credentials);
                render_selectable_list(
                    frame,
                    layout[1],
                    self.screen.title(),
                    items,
                    self.selected_credential,
                    Some("No credentials found. Add one to get started."),
                );
            }
            Screen::Details => {
                self.render_details(frame, layout[1]);
            }
            Screen::Update => self.render_form(
                frame,
                layout[1],
                "Enter the credential name and the new credential reference.",
            ),
            Screen::Register => self.render_form(
                frame,
                layout[1],
                "Link an existing credential without re-entering secrets.",
            ),
            Screen::Rotate => {
                let items = credential_items(&self.credentials);
                render_selectable_list(
                    frame,
                    layout[1],
                    self.screen.title(),
                    items,
                    self.selected_credential,
                    Some("No credentials available to rotate."),
                );
            }
            Screen::DeleteCredential => {
                let items = credential_items(&self.credentials);
                render_selectable_list(
                    frame,
                    layout[1],
                    self.screen.title(),
                    items,
                    self.selected_credential,
                    Some("No credentials available to delete."),
                );
            }
            Screen::SetNotionApiKey => self.render_form(
                frame,
                layout[1],
                "Enter your Notion API key. It will be stored in the local SQLite database.",
            ),
            Screen::DeleteNotionApiKey => self.render_form(
                frame,
                layout[1],
                "Type DELETE to confirm removal of the Notion API Key from the local database.",
            ),
        }

        render_status(frame, layout[2], &self.status);
    }

    fn render_form(&self, frame: &mut Frame, area: Rect, note: &str) {
        let visible_rows = area.height.saturating_sub(2) as usize;
        let mut lines = form_lines(&self.form, visible_rows.saturating_sub(2));
        lines.push(Line::from(""));
        lines.push(Line::from(Span::styled(
            format!("  {note}"),
            Style::default().fg(MUTED),
        )));
        frame.render_widget(
            Paragraph::new(lines)
                .style(Style::default().bg(PANEL_BG))
                .block(panel(self.screen.title())),
            area,
        );
    }

    fn render_add_choice(&self, frame: &mut Frame, area: Rect) {
        let sections = Layout::default()
            .direction(Direction::Vertical)
            .constraints([Constraint::Length(3), Constraint::Min(3)])
            .split(area);

        let prompt = Paragraph::new(Line::from(Span::styled(
            "  How would you like to store this credential?",
            Style::default().fg(TEXT).add_modifier(Modifier::BOLD),
        )))
        .style(Style::default().bg(PANEL_BG))
        .block(panel(self.screen.title()));

        frame.render_widget(prompt, sections[0]);

        let items = ADD_CHOICES
            .iter()
            .map(|choice| ListItem::new(format!("  {choice}")))
            .collect();

        render_selectable_list(
            frame,
            sections[1],
            "STORAGE OPTION",
            items,
            self.add_choice,
            None,
        );
    }

    fn render_secret(&self, frame: &mut Frame, area: Rect) {
        let name = self.form.value(0).trim();

        let lines = vec![
            Line::from(vec![
                Span::styled("  Credential  ", Style::default().fg(MUTED)),
                Span::styled(
                    name.to_string(),
                    Style::default().fg(TEXT).add_modifier(Modifier::BOLD),
                ),
            ]),
            Line::from(""),
            Line::from(vec![
                Span::styled("  Secret      ", Style::default().fg(MUTED)),
                Span::styled(
                    "••••••••",
                    Style::default().fg(ACCENT).add_modifier(Modifier::BOLD),
                ),
                Span::styled("▌", Style::default().fg(ACCENT)),
                Span::styled("  (hidden)", Style::default().fg(MUTED)),
            ]),
            Line::from(""),
            Line::from(Span::styled(
                "  The secret is never rendered, logged, or sent to Notion.",
                Style::default().fg(MUTED),
            )),
            Line::from(Span::styled(
                "  It is discarded immediately after the create attempt.",
                Style::default().fg(MUTED),
            )),
            Line::from(""),
            Line::from(Span::styled(
                "  Enter to create · Esc to cancel and discard",
                Style::default().fg(ACCENT),
            )),
        ];

        frame.render_widget(
            Paragraph::new(lines)
                .style(Style::default().bg(PANEL_BG))
                .block(accent_panel(self.screen.title())),
            area,
        );
    }

    fn render_add_existing(&self, frame: &mut Frame, area: Rect) {
        let items = self
            .items
            .iter()
            .map(|item| {
                ListItem::new(Line::from(vec![
                    Span::styled(
                        format!("  {:<36}", truncate(&item.title, 36)),
                        Style::default().fg(TEXT),
                    ),
                    Span::styled(format!("  {}", item.id), Style::default().fg(MUTED)),
                ]))
            })
            .collect();

        render_selectable_list(
            frame,
            area,
            self.screen.title(),
            items,
            self.item_selected,
            Some("No credentials found in database."),
        );
    }

    fn render_add_result(&self, frame: &mut Frame, area: Rect) {
        let lines: Vec<Line<'static>> = self
            .result_lines
            .iter()
            .map(|line| Line::from(Span::styled(format!("  {line}"), Style::default().fg(TEXT))))
            .collect();

        frame.render_widget(
            Paragraph::new(lines)
                .style(Style::default().bg(PANEL_BG).fg(TEXT))
                .block(accent_panel(self.screen.title()))
                .wrap(Wrap { trim: false }),
            area,
        );
    }

    fn render_search(&self, frame: &mut Frame, area: Rect) {
        let sections = Layout::default()
            .direction(Direction::Vertical)
            .constraints([Constraint::Length(4), Constraint::Min(3)])
            .split(area);

        let query_line = Line::from(vec![
            Span::styled(
                "  Query  ",
                Style::default().fg(ACCENT).add_modifier(Modifier::BOLD),
            ),
            Span::styled(
                self.form.value(0).to_string(),
                Style::default().fg(TEXT).add_modifier(Modifier::BOLD),
            ),
            Span::styled("▌", Style::default().fg(ACCENT)),
        ]);
        let note = Line::from(Span::styled(
            "  Filters as you type · ↑ / ↓ select · Enter view details",
            Style::default().fg(MUTED),
        ));

        frame.render_widget(
            Paragraph::new(vec![query_line, note])
                .style(Style::default().bg(PANEL_BG))
                .block(panel(self.screen.title())),
            sections[0],
        );

        let matches = self.search_matches();
        let selected = if matches.is_empty() {
            0
        } else {
            self.search_selected.min(matches.len() - 1)
        };
        let items = matches
            .iter()
            .map(|credential| credential_item(credential))
            .collect();

        render_selectable_list(
            frame,
            sections[1],
            "RESULTS",
            items,
            selected,
            Some("No credentials match that query."),
        );
    }

    fn render_details(&mut self, frame: &mut Frame, area: Rect) {
        let Some(credential) = self.credentials.get(self.selected_credential) else {
            frame.render_widget(
                Paragraph::new("No credential selected.")
                    .style(Style::default().fg(MUTED))
                    .block(accent_panel(self.screen.title())),
                area,
            );
            return;
        };

        let inner_width = area.width.saturating_sub(2) as usize;
        let value_width = inner_width.saturating_sub(28);

        let status = credential.status.as_deref().unwrap_or("Unknown");
        let badge = format!(" {} ", status.to_uppercase());
        let badge_width = badge.chars().count();
        let name_width = inner_width.saturating_sub(badge_width + 5).max(1);
        let name = truncate(&credential.name, name_width);
        let gap = inner_width.saturating_sub(4 + name.chars().count() + badge_width);

        let header = vec![
            Line::from(vec![
                Span::styled(
                    format!("  {name}"),
                    Style::default().fg(TEXT).add_modifier(Modifier::BOLD),
                ),
                Span::raw(" ".repeat(gap)),
                Span::styled(badge, status_badge_style(status)),
                Span::raw("  "),
            ]),
            Line::from(Span::styled(
                format!(" {} ", "─".repeat(inner_width.saturating_sub(2))),
                Style::default().fg(BORDER),
            )),
        ];

        let mut lines = vec![
            detail_line(
                "Environment",
                credential.environment.as_deref(),
                value_width,
            ),
            detail_line("Purpose", credential.purpose.as_deref(), value_width),
            detail_line("Owner", credential.owner.as_deref(), value_width),
            detail_line("Repository", credential.repository.as_deref(), value_width),
            detail_line(
                "Environment Variable",
                credential.environment_variable.as_deref(),
                value_width,
            ),
            detail_line(
                "Credential Reference",
                credential.credential_reference.as_deref(),
                value_width,
            ),
            detail_line(
                "Rotation Frequency",
                credential.rotation_frequency.as_deref(),
                value_width,
            ),
            detail_line(
                "Last Rotated",
                credential.last_rotated.as_deref(),
                value_width,
            ),
            detail_line(
                "Next Rotation Due",
                credential.next_rotation_due.as_deref(),
                value_width,
            ),
            detail_line("Created At", credential.created_at.as_deref(), value_width),
            detail_line(
                "Last Updated",
                credential.updated_at.as_deref(),
                value_width,
            ),
        ];

        let inner = Rect {
            x: area.x + 1,
            y: area.y + 1,
            width: area.width.saturating_sub(2),
            height: area.height.saturating_sub(2),
        };
        let header_height = 2.min(inner.height);
        let body_height = inner.height.saturating_sub(header_height);

        let max_offset = lines.len().saturating_sub(body_height as usize);
        self.details_offset = self.details_offset.min(max_offset);
        lines.drain(..self.details_offset);

        let header_area = Rect {
            height: header_height,
            ..inner
        };
        let body_area = Rect {
            y: inner.y + header_height,
            height: body_height,
            ..inner
        };

        frame.render_widget(accent_panel(self.screen.title()), area);
        frame.render_widget(
            Paragraph::new(header).style(Style::default().bg(PANEL_BG)),
            header_area,
        );
        frame.render_widget(
            Paragraph::new(lines).style(Style::default().bg(PANEL_BG).fg(TEXT)),
            body_area,
        );
    }
}

pub fn run() -> Result<(), Box<dyn std::error::Error>> {
    let settings = crate::config::Settings::load()?;
    let database = Database::open(&settings.db_path)?;

    enable_raw_mode()?;

    let mut stdout = io::stdout();
    execute!(stdout, EnterAlternateScreen)?;

    let backend = CrosstermBackend::new(stdout);
    let mut terminal = Terminal::new(backend)?;

    let result = show_dashboard(&mut terminal, database);

    disable_raw_mode()?;
    execute!(terminal.backend_mut(), LeaveAlternateScreen)?;
    terminal.show_cursor()?;

    result
}

fn suspend_tui(terminal: &mut Terminal<CrosstermBackend<io::Stdout>>) -> io::Result<()> {
    disable_raw_mode()?;
    execute!(terminal.backend_mut(), LeaveAlternateScreen)?;
    terminal.show_cursor()?;
    Ok(())
}

fn resume_tui(terminal: &mut Terminal<CrosstermBackend<io::Stdout>>) -> io::Result<()> {
    enable_raw_mode()?;
    execute!(terminal.backend_mut(), EnterAlternateScreen)?;
    terminal.hide_cursor()?;
    terminal.clear()?;
    Ok(())
}

fn show_dashboard(
    terminal: &mut Terminal<CrosstermBackend<io::Stdout>>,
    database: Database,
) -> Result<(), Box<dyn std::error::Error>> {
    let mut dashboard = Dashboard::new(database);

    loop {
        terminal.draw(|frame| dashboard.render(frame))?;

        if let Event::Key(key) = event::read()? {
            if key.kind == KeyEventKind::Release {
                continue;
            }

            if dashboard.handle_key(key.code) {
                break;
            }

            if let Some(action) = dashboard.take_pending_action() {
                suspend_tui(terminal)?;
                dashboard.perform_action(action);
                resume_tui(terminal)?;
                terminal.draw(|frame| dashboard.render(frame))?;
            }
        }
    }

    Ok(())
}

fn panel(title: &str) -> Block<'static> {
    Block::default()
        .title(Line::from(Span::styled(
            format!(" {title} "),
            Style::default().fg(MUTED).add_modifier(Modifier::BOLD),
        )))
        .borders(Borders::ALL)
        .border_style(Style::default().fg(BORDER))
        .style(Style::default().bg(PANEL_BG))
}

fn accent_panel(title: &str) -> Block<'static> {
    Block::default()
        .title(Line::from(Span::styled(
            format!(" {title} "),
            Style::default().fg(ACCENT).add_modifier(Modifier::BOLD),
        )))
        .borders(Borders::ALL)
        .border_style(Style::default().fg(BORDER))
        .style(Style::default().bg(PANEL_BG))
}

fn status_badge_style(status: &str) -> Style {
    match status.to_lowercase().as_str() {
        "active" => Style::default().fg(ACCENT).bg(BADGE_ACTIVE_BG),
        "inactive" => Style::default().fg(AMBER).bg(BADGE_INACTIVE_BG),
        "revoked" => Style::default().fg(DANGER).bg(BADGE_REVOKED_BG),
        _ => Style::default().fg(MUTED).bg(SELECT_BG),
    }
    .add_modifier(Modifier::BOLD)
}

fn render_header(frame: &mut Frame, area: Rect) {
    let lines = vec![
        Line::from(Span::styled(
            "API CREDENTIAL MANAGER",
            Style::default().fg(ACCENT).add_modifier(Modifier::BOLD),
        )),
        Line::from(""),
        Line::from(Span::styled(
            "Securely manage your developer credentials.",
            Style::default().fg(TEXT),
        )),
        Line::from(Span::styled(
            "Secrets stay in local SQLite database; Notion stores metadata.",
            Style::default().fg(MUTED),
        )),
    ];

    let header = Paragraph::new(lines)
        .style(Style::default().bg(PANEL_BG))
        .block(
            Block::default()
                .title(Line::from(Span::styled(
                    " NOTION + SQLITE ",
                    Style::default().fg(ACCENT).add_modifier(Modifier::BOLD),
                )))
                .borders(Borders::ALL)
                .border_style(Style::default().fg(BORDER))
                .style(Style::default().bg(PANEL_BG)),
        );

    frame.render_widget(header, area);
}

fn render_status(frame: &mut Frame, area: Rect, status: &str) {
    let footer = Paragraph::new(Line::from(Span::styled(
        status.to_string(),
        Style::default().fg(AMBER).add_modifier(Modifier::BOLD),
    )))
    .style(Style::default().bg(PANEL_BG))
    .block(
        Block::default()
            .title(Line::from(Span::styled(
                " STATUS ",
                Style::default().fg(MUTED).add_modifier(Modifier::BOLD),
            )))
            .borders(Borders::ALL)
            .border_style(Style::default().fg(BORDER))
            .style(Style::default().bg(PANEL_BG)),
    );

    frame.render_widget(footer, area);
}

fn render_selectable_list(
    frame: &mut Frame,
    area: Rect,
    title: &str,
    items: Vec<ListItem<'static>>,
    selected: usize,
    empty_message: Option<&str>,
) {
    if items.is_empty() {
        if let Some(message) = empty_message {
            let paragraph = Paragraph::new(Line::from(Span::styled(
                format!("  {message}"),
                Style::default().fg(MUTED),
            )))
            .style(Style::default().bg(PANEL_BG))
            .block(panel(title));
            frame.render_widget(paragraph, area);
        }
        return;
    }

    let visible_rows = area.height.saturating_sub(2) as usize;
    let offset = selected.saturating_sub(visible_rows.saturating_sub(1));
    let mut state = ListState::default()
        .with_offset(offset)
        .with_selected(Some(selected.min(items.len() - 1)));

    let list = List::new(items)
        .style(Style::default().bg(PANEL_BG).fg(TEXT))
        .block(panel(title))
        .highlight_style(
            Style::default()
                .bg(SELECT_BG)
                .fg(ACCENT)
                .add_modifier(Modifier::BOLD),
        )
        .highlight_symbol("➜ ");

    frame.render_stateful_widget(list, area, &mut state);
}

fn detail_line(label: &str, value: Option<&str>, width: usize) -> Line<'static> {
    let value = value
        .filter(|value| !value.trim().is_empty())
        .unwrap_or("—");

    Line::from(vec![
        Span::styled(format!("  {label:<24}"), Style::default().fg(MUTED)),
        Span::styled(truncate(value, width), Style::default().fg(TEXT)),
    ])
}

fn optional_value(value: &str) -> Option<String> {
    let value = value.trim();
    if value.is_empty() {
        None
    } else {
        Some(value.to_string())
    }
}

fn is_date_like(value: &str) -> bool {
    let bytes = value.as_bytes();

    if bytes.len() < 10 {
        return false;
    }

    let date_is_shaped = bytes[4] == b'-'
        && bytes[7] == b'-'
        && bytes[..4].iter().all(u8::is_ascii_digit)
        && bytes[5..7].iter().all(u8::is_ascii_digit)
        && bytes[8..10].iter().all(u8::is_ascii_digit);

    date_is_shaped && (bytes.len() == 10 || bytes[10] == b'T')
}

fn truncate(value: &str, max: usize) -> String {
    if value.chars().count() <= max {
        value.to_string()
    } else {
        value
            .chars()
            .take(max.saturating_sub(1))
            .chain(['…'])
            .collect()
    }
}

fn credential_items(credentials: &[Credential]) -> Vec<ListItem<'static>> {
    credentials.iter().map(credential_item).collect()
}

fn credential_item(credential: &Credential) -> ListItem<'static> {
    let status = credential.status.as_deref().unwrap_or("Unknown");
    let environment = credential.environment.as_deref().unwrap_or("—");

    let status_style = match status.to_lowercase().as_str() {
        "active" => Style::default().fg(ACCENT).add_modifier(Modifier::BOLD),
        "inactive" => Style::default().fg(AMBER).add_modifier(Modifier::BOLD),
        "revoked" => Style::default().fg(DANGER).add_modifier(Modifier::BOLD),
        _ => Style::default().fg(MUTED),
    };

    ListItem::new(Line::from(vec![
        Span::styled(
            format!("  {:<32}", truncate(&credential.name, 32)),
            Style::default().fg(TEXT),
        ),
        Span::styled(format!("[{:<10}]", status.to_uppercase()), status_style),
        Span::styled(
            format!("  {:<14}", truncate(environment, 14)),
            Style::default().fg(MUTED),
        ),
    ]))
}

fn form_lines(form: &Form, visible_rows: usize) -> Vec<Line<'static>> {
    let offset = form.active.saturating_sub(visible_rows.saturating_sub(1));

    form.fields
        .iter()
        .enumerate()
        .skip(offset)
        .map(|(index, (label, value))| {
            let active = index == form.active;
            let label_style = Style::default().fg(if active { ACCENT } else { MUTED });
            let mut value_style = Style::default().fg(TEXT);
            if active {
                value_style = value_style.add_modifier(Modifier::BOLD);
            }

            let mut spans = vec![
                Span::styled(format!("  {label:<24}"), label_style),
                Span::styled(value.clone(), value_style),
            ];
            if active {
                spans.push(Span::styled("▌", Style::default().fg(ACCENT)));
            }

            Line::from(spans)
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::NamedTempFile;

    fn test_database() -> Database {
        let file = NamedTempFile::new().expect("failed to create temp file");
        Database::open(file.path()).expect("failed to open test database")
    }

    fn filled_dashboard() -> Dashboard {
        let database = test_database();
        let mut dashboard = Dashboard::new(database);
        dashboard.open(Screen::Add);
        dashboard.submit();

        let values = [
            "Test Credential",
            "Testing",
            "TEST_API_KEY",
            "Local",
            "example/repo",
            "Owner",
            "Every 90 days",
        ];

        for (index, value) in values.iter().enumerate() {
            dashboard.form.fields[index].1 = (*value).to_string();
        }

        dashboard
    }

    #[test]
    fn add_defaults_to_create_new() {
        let mut dashboard = Dashboard::new(test_database());
        dashboard.open(Screen::Add);

        assert_eq!(dashboard.add_choice, 0);
        assert_eq!(ADD_CHOICES[0], "Create a new credential");

        dashboard.submit();
        assert_eq!(dashboard.screen, Screen::AddDetails);
    }

    #[test]
    fn create_path_advances_to_secret_entry() {
        let mut dashboard = filled_dashboard();

        dashboard.submit();

        assert_eq!(dashboard.screen, Screen::AddSecret);
        assert!(dashboard.pending_action.is_none());
    }

    #[test]
    fn create_path_requires_each_required_field() {
        let mut dashboard = filled_dashboard();
        dashboard.form.fields[0].1.clear();

        dashboard.submit();

        assert_eq!(dashboard.screen, Screen::AddDetails);
        assert_eq!(dashboard.status, "Name is required.");
    }

    #[test]
    fn secret_input_is_not_echoed_or_stored_in_the_form() {
        let mut dashboard = filled_dashboard();
        dashboard.submit();

        for character in "super-secret".chars() {
            dashboard.handle_key(KeyCode::Char(character));
        }

        assert_eq!(dashboard.secret_input, "super-secret");
        assert!(!dashboard.status.contains("super-secret"));
        assert_eq!(dashboard.form.value(0), "Test Credential");
    }

    #[test]
    fn cancelling_secret_entry_discards_it() {
        let mut dashboard = filled_dashboard();
        dashboard.submit();
        dashboard.handle_key(KeyCode::Char('x'));

        dashboard.handle_key(KeyCode::Esc);

        assert_eq!(dashboard.screen, Screen::AddDetails);
        assert!(dashboard.secret_input.is_empty());
        assert_eq!(dashboard.form.value(0), "Test Credential");
    }

    #[test]
    fn confirming_secret_requests_creation_without_running_it() {
        let mut dashboard = filled_dashboard();
        dashboard.submit();
        dashboard.handle_key(KeyCode::Char('x'));

        dashboard.handle_key(KeyCode::Enter);

        assert_eq!(dashboard.pending_action, Some(PendingAction::CreateItem));
        assert_eq!(dashboard.secret_input, "x");
    }

    #[test]
    fn existing_path_requests_item_listing() {
        let mut dashboard = filled_dashboard();
        dashboard.add_choice = 1;

        dashboard.submit();

        assert_eq!(dashboard.pending_action, Some(PendingAction::LoadItems));
    }

    #[test]
    fn existing_path_registers_the_highlighted_item() {
        let mut dashboard = filled_dashboard();
        dashboard.add_choice = 1;
        dashboard.screen = Screen::AddExisting;
        dashboard.items = vec![
            SecretItem {
                id: "one".into(),
                title: "First".into(),
            },
            SecretItem {
                id: "two".into(),
                title: "Second".into(),
            },
        ];
        dashboard.select_item(1);

        dashboard.submit();

        assert_eq!(
            dashboard.pending_action,
            Some(PendingAction::RegisterExisting {
                item_id: "two".into(),
                item_title: "Second".into(),
            })
        );
    }

    #[test]
    fn escaping_the_choice_screen_returns_to_the_menu() {
        let mut dashboard = Dashboard::new(test_database());
        dashboard.open(Screen::Add);

        dashboard.handle_key(KeyCode::Esc);

        assert_eq!(dashboard.screen, Screen::Menu);
    }

    #[test]
    fn result_screen_clears_the_collected_secret() {
        let mut dashboard = filled_dashboard();
        dashboard.submit();
        dashboard.handle_key(KeyCode::Char('s'));

        dashboard.show_add_result(vec!["Partial failure details".into()]);

        assert_eq!(dashboard.screen, Screen::AddResult);
        assert!(dashboard.secret_input.is_empty());
        assert_eq!(dashboard.result_lines, ["Partial failure details"]);

        dashboard.handle_key(KeyCode::Esc);

        assert_eq!(dashboard.screen, Screen::Menu);
        assert!(dashboard.result_lines.is_empty());
    }

    #[test]
    fn date_validation_accepts_iso_shapes_only() {
        assert!(is_date_like("2026-01-01"));
        assert!(is_date_like("2026-01-01T10:00:00Z"));
        assert!(!is_date_like("01/02/2026"));
        assert!(!is_date_like("2026-1-1"));
        assert!(!is_date_like("2026-01-01 10:00"));
    }

    #[test]
    fn invalid_optional_date_blocks_the_flow() {
        let mut dashboard = filled_dashboard();
        dashboard.form.fields[7].1 = "next tuesday".into();

        dashboard.submit();

        assert_eq!(dashboard.screen, Screen::AddDetails);
        assert_eq!(dashboard.status, "Last Rotated must use YYYY-MM-DD format.");
    }

    #[test]
    fn metadata_maps_optional_dates_only_when_present() {
        let dashboard = filled_dashboard();

        let credential = dashboard.credential_from_form("sqlite://vault/item/credential".into());

        assert_eq!(credential.name, "Test Credential");
        assert_eq!(credential.environment_variable, "TEST_API_KEY");
        assert_eq!(credential.environment, "Local");
        assert!(credential.last_rotated.is_none());
        assert!(credential.next_rotation_due.is_none());

        let mut dashboard = filled_dashboard();
        dashboard.form.fields[7].1 = "2026-01-01".into();

        let credential = dashboard.credential_from_form("sqlite://vault/item/credential".into());

        assert_eq!(credential.last_rotated.as_deref(), Some("2026-01-01"));
    }
}
