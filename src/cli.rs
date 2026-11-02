use std::io;

use crossterm::{
    event::{self, Event, KeyCode, KeyEventKind},
    execute,
    terminal::{
        disable_raw_mode, enable_raw_mode, EnterAlternateScreen, LeaveAlternateScreen,
    },
};

use ratatui::{
    backend::CrosstermBackend,
    layout::{Constraint, Direction, Layout, Rect},
    style::{Color, Modifier, Style},
    text::{Line, Span},
    widgets::{Block, Borders, List, ListItem, ListState, Paragraph},
    Frame, Terminal,
};

use crate::models::Credential;

const MENU_ITEMS: [&str; 7] = [
    "Add Credential",
    "Search / Retrieve Credential",
    "List Credentials",
    "Update Credential Reference",
    "Register Existing 1Password Item",
    "Rotate Credential",
    "Exit",
];

const BG: Color = Color::Rgb(11, 18, 32);
const PANEL_BG: Color = Color::Rgb(5, 8, 15);
const BORDER: Color = Color::Rgb(34, 48, 74);
const TEXT: Color = Color::Rgb(230, 237, 247);
const MUTED: Color = Color::Rgb(147, 164, 189);
const ACCENT: Color = Color::Rgb(126, 226, 168);
const AMBER: Color = Color::Rgb(240, 198, 116);
const SELECT_BG: Color = Color::Rgb(27, 42, 68);

#[derive(Clone, Copy, PartialEq, Eq)]
enum Screen {
    Menu,
    Add,
    Search,
    List,
    Update,
    Register,
    Rotate,
}

impl Screen {
    fn title(self) -> &'static str {
        match self {
            Screen::Menu => "CREDENTIAL OPERATIONS",
            Screen::Add => "ADD CREDENTIAL",
            Screen::Search => "SEARCH / RETRIEVE",
            Screen::List => "CREDENTIALS",
            Screen::Update => "UPDATE CREDENTIAL REFERENCE",
            Screen::Register => "REGISTER 1PASSWORD ITEM",
            Screen::Rotate => "ROTATE CREDENTIAL",
        }
    }

    fn hint(self) -> &'static str {
        match self {
            Screen::Menu => "Use ↑ / ↓ to navigate · Enter to open · q to quit",
            Screen::Add => {
                "Type to edit · Tab / ↑ / ↓ switches fields · Enter to save · Esc to go back"
            }
            Screen::Search => "Type to filter · Enter to search · Esc to go back",
            Screen::List => "Enter to refresh · Esc to go back",
            Screen::Update => {
                "Type to edit · Tab / ↑ / ↓ switches fields · Enter to save · Esc to go back"
            }
            Screen::Register => {
                "Type to edit · Tab / ↑ / ↓ switches fields · Enter to save · Esc to go back"
            }
            Screen::Rotate => "↑ / ↓ to choose · Enter to rotate · Esc to go back",
        }
    }

    fn is_form(self) -> bool {
        matches!(self, Screen::Add | Screen::Search | Screen::Update | Screen::Register)
    }
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
}

impl Dashboard {
    fn new() -> Self {
        Self {
            screen: Screen::Menu,
            selected: 0,
            status: Screen::Menu.hint().to_string(),
            form: Form::new(&[]),
            credentials: Vec::new(),
            selected_credential: 0,
        }
    }

    fn load_credentials(&mut self) {
        self.credentials = crate::notion::list_credentials();
        self.selected_credential = 0;
    }

    fn open(&mut self, screen: Screen) {
        self.screen = screen;
        self.status = screen.hint().to_string();
        self.form = match screen {
            Screen::Add => Form::new(&["Name", "Service", "Username", "Description"]),
            Screen::Search => Form::new(&["Query"]),
            Screen::Update => Form::new(&["Credential name", "New 1Password reference"]),
            Screen::Register => Form::new(&["1Password item", "Notion name"]),
            _ => Form::new(&[]),
        };

        if matches!(screen, Screen::List | Screen::Search | Screen::Rotate) {
            self.load_credentials();
        }

        if screen == Screen::List {
            self.status = Self::credential_status(self.credentials.len());
        } else if screen == Screen::Rotate && self.credentials.is_empty() {
            self.status = "No credentials available to rotate.".to_string();
        }
    }

    fn back_to_menu(&mut self) {
        self.screen = Screen::Menu;
        self.form = Form::new(&[]);
    }

    fn credential_status(count: usize) -> String {
        if count == 0 {
            "No credentials found.".to_string()
        } else {
            format!("Found {count} credential(s).")
        }
    }

    fn search_matches(&self) -> Vec<&Credential> {
        let query = self.form.value(0).trim().to_lowercase();
        self.credentials
            .iter()
            .filter(|credential| {
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
            .collect()
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

    fn submit(&mut self) {
        match self.screen {
            Screen::Menu => {}
            Screen::Search => {
                let count = self.search_matches().len();
                self.status = if count == 0 {
                    "No credentials match that query.".to_string()
                } else {
                    format!("Found {count} matching credential(s).")
                };
            }
            Screen::List => {
                self.load_credentials();
                self.status = Self::credential_status(self.credentials.len());
            }
            Screen::Add => {
                let name = self.form.value(0).trim().to_string();
                if name.is_empty() {
                    self.status = "Name is required.".to_string();
                    return;
                }
                self.status = format!(
                    "Add Credential: '{name}' is ready, but Notion/1Password are not connected yet."
                );
                self.back_to_menu();
            }
            Screen::Update => {
                let name = self.form.value(0).trim().to_string();
                let reference = self.form.value(1).trim().to_string();
                if name.is_empty() || reference.is_empty() {
                    self.status =
                        "Credential name and 1Password reference are required.".to_string();
                    return;
                }
                self.status = format!(
                    "Update '{name}': Notion/1Password are not connected yet."
                );
                self.back_to_menu();
            }
            Screen::Register => {
                let item = self.form.value(0).trim().to_string();
                let name = self.form.value(1).trim().to_string();
                if item.is_empty() || name.is_empty() {
                    self.status = "1Password item and Notion name are required.".to_string();
                    return;
                }
                self.status =
                    format!("Register '{name}': Notion/1Password are not connected yet.");
                self.back_to_menu();
            }
            Screen::Rotate => {
                if self.credentials.is_empty() {
                    self.status = "No credentials available to rotate.".to_string();
                    return;
                }
                let index = self.selected_credential.min(self.credentials.len() - 1);
                let name = self.credentials[index].name.clone();
                self.status = format!("Rotate '{name}': Notion/1Password are not connected yet.");
            }
        }
    }

    fn handle_key(&mut self, code: KeyCode) -> bool {
        if self.screen == Screen::Menu {
            return self.handle_menu_key(code);
        }

        match code {
            KeyCode::Esc => {
                self.back_to_menu();
                self.status = Screen::Menu.hint().to_string();
            }
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
            KeyCode::Up => {
                if self.screen.is_form() {
                    self.form.previous();
                } else {
                    self.select_credential(-1);
                }
            }
            KeyCode::Down => {
                if self.screen.is_form() {
                    self.form.next();
                } else {
                    self.select_credential(1);
                }
            }
            KeyCode::Backspace => {
                if self.screen.is_form() {
                    self.form.backspace();
                }
            }
            KeyCode::Char(character) => {
                if self.screen.is_form() {
                    self.form.push(character);
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
                    _ => return true,
                };
                self.open(screen);
            }
            KeyCode::Char('q') | KeyCode::Char('Q') => return true,
            _ => {}
        }
        false
    }

    fn render(&self, frame: &mut Frame) {
        let area = frame.area();
        frame.render_widget(
            Block::default().style(Style::default().bg(BG)),
            area,
        );

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
                render_selectable_list(frame, layout[1], self.screen.title(), items, self.selected, None);
            }
            Screen::Add => self.render_form(
                frame,
                layout[1],
                "Secrets stay in 1Password; Notion stores metadata.",
            ),
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
            Screen::Update => self.render_form(
                frame,
                layout[1],
                "Enter the credential name and the new 1Password reference.",
            ),
            Screen::Register => self.render_form(
                frame,
                layout[1],
                "Link an existing 1Password item without re-entering secrets.",
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

    fn render_search(&self, frame: &mut Frame, area: Rect) {
        let mut lines = form_lines(&self.form, usize::MAX);
        lines.push(Line::from(""));
        lines.push(Line::from(Span::styled(
            "  RESULTS",
            Style::default().fg(ACCENT).add_modifier(Modifier::BOLD),
        )));

        let matches = self.search_matches();
        if matches.is_empty() {
            lines.push(Line::from(Span::styled(
                "  No credentials found.",
                Style::default().fg(MUTED),
            )));
        } else {
            for credential in matches {
                let service = credential.service.as_deref().unwrap_or("—");
                lines.push(Line::from(Span::styled(
                    format!("  {}  ({service})", credential.name),
                    Style::default().fg(TEXT),
                )));
            }
        }

        frame.render_widget(
            Paragraph::new(lines)
                .style(Style::default().bg(PANEL_BG))
                .block(panel(self.screen.title())),
            area,
        );
    }
}

pub fn run() -> Result<(), Box<dyn std::error::Error>> {
    enable_raw_mode()?;

    let mut stdout = io::stdout();
    execute!(stdout, EnterAlternateScreen)?;

    let backend = CrosstermBackend::new(stdout);
    let mut terminal = Terminal::new(backend)?;

    let result = show_dashboard(&mut terminal);

    disable_raw_mode()?;
    execute!(terminal.backend_mut(), LeaveAlternateScreen)?;
    terminal.show_cursor()?;

    result
}

fn show_dashboard(
    terminal: &mut Terminal<CrosstermBackend<io::Stdout>>,
) -> Result<(), Box<dyn std::error::Error>> {
    let mut dashboard = Dashboard::new();

    loop {
        terminal.draw(|frame| dashboard.render(frame))?;

        if let Event::Key(key) = event::read()? {
            if key.kind == KeyEventKind::Release {
                continue;
            }

            if dashboard.handle_key(key.code) {
                break;
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
            "Secrets stay in 1Password; Notion stores metadata.",
            Style::default().fg(MUTED),
        )),
    ];

    let header = Paragraph::new(lines)
        .style(Style::default().bg(PANEL_BG))
        .block(
            Block::default()
                .title(Line::from(Span::styled(
                    " NOTION + 1PASSWORD ",
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

fn credential_items(credentials: &[Credential]) -> Vec<ListItem<'static>> {
    credentials
        .iter()
        .map(|credential| {
            let mut detail = credential
                .service
                .clone()
                .unwrap_or_else(|| "—".to_string());
            if let Some(username) = &credential.username {
                if !username.is_empty() {
                    detail = format!("{detail} · {username}");
                }
            }
            ListItem::new(format!("  {}  ({detail})", credential.name))
        })
        .collect()
}

fn form_lines(form: &Form, visible_rows: usize) -> Vec<Line<'static>> {
    let offset = form
        .active
        .saturating_sub(visible_rows.saturating_sub(1));

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
