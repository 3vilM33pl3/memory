use std::{
    env, fs, io,
    path::{Path, PathBuf},
    time::Duration,
};

use anyhow::{Context, Result};
use crossterm::{
    event::{self, Event, KeyCode, KeyEvent, KeyModifiers},
    execute,
    terminal::{EnterAlternateScreen, LeaveAlternateScreen, disable_raw_mode, enable_raw_mode},
};
use mem_api::{AppConfig, discover_global_config_path};
use ratatui::{
    Terminal,
    backend::CrosstermBackend,
    layout::{Constraint, Direction, Layout, Rect},
    style::{Color, Modifier, Style},
    text::{Line, Span},
    widgets::{Block, Borders, Paragraph, Wrap},
};
use reqwest::Client;

use super::{
    ApiClient, default_global_config_path, enable_watch_service, mask_database_url,
    packaged_service_available, print_doctor_report, print_scan_report, repair_repo_bootstrap,
    run_doctor, run_systemctl_system, shared_env_lookup, shared_env_path_for_config,
    write_shared_env_file,
};
use crate::scan;

pub(crate) async fn run(
    cwd: &Path,
    repo_root: &Path,
    project: Option<String>,
    prefer_global: bool,
) -> Result<()> {
    let mut terminal = setup_terminal()?;
    let mut app = WizardApp::new(cwd, repo_root, project, prefer_global);
    let outcome = loop {
        terminal.draw(|frame| draw(frame, &app))?;
        if event::poll(Duration::from_millis(200))? {
            match event::read()? {
                Event::Key(key) => {
                    if let Some(outcome) = app.handle_key(key)? {
                        break outcome;
                    }
                }
                _ => {}
            }
        }
    };
    restore_terminal(terminal)?;

    match outcome {
        WizardOutcome::Cancel => {
            println!("Wizard cancelled.");
            Ok(())
        }
        WizardOutcome::Apply(state) => apply(state).await,
    }
}

#[derive(Debug, Clone)]
struct WizardState {
    global_config_path: PathBuf,
    shared_env_path: PathBuf,
    repo_root: Option<PathBuf>,
    project: String,
    configure_global: bool,
    database_url: String,
    api_token: String,
    llm_provider: String,
    llm_base_url: String,
    llm_api_key_env: String,
    llm_model: String,
    llm_api_key_value: String,
    initialize_repo: bool,
    enable_backend_service: bool,
    enable_watcher_service: bool,
    run_scan: bool,
    scan_dry_run: bool,
}

impl WizardState {
    fn new(cwd: &Path, repo_root: &Path, project: Option<String>, prefer_global: bool) -> Self {
        let global_config_path =
            discover_global_config_path().unwrap_or_else(default_global_config_path);
        let shared_env_path = shared_env_path_for_config(&global_config_path);
        let repo_available = repo_root != cwd || repo_root.join(".git").exists();
        let repo_root = repo_available.then(|| repo_root.to_path_buf());
        let existing_config = if repo_available {
            AppConfig::load_from_path(None).ok()
        } else {
            AppConfig::load_from_path(Some(global_config_path.clone())).ok()
        };
        let project = project
            .or_else(|| repo_root.as_deref().and_then(read_project_slug))
            .or_else(|| {
                repo_root
                    .as_ref()
                    .and_then(|root| root.file_name())
                    .and_then(|value| value.to_str())
                    .map(ToOwned::to_owned)
            })
            .or_else(|| {
                cwd.file_name()
                    .and_then(|value| value.to_str())
                    .map(ToOwned::to_owned)
            })
            .unwrap_or_else(|| "memory".to_string());

        let llm_api_key_env = existing_config
            .as_ref()
            .map(|config| config.llm.api_key_env.clone())
            .unwrap_or_else(|| "OPENAI_API_KEY".to_string());

        let llm_api_key_value = shared_env_lookup(&shared_env_path, &llm_api_key_env)
            .or_else(|| env::var(&llm_api_key_env).ok())
            .unwrap_or_default();

        Self {
            global_config_path,
            shared_env_path,
            repo_root: repo_root.clone(),
            project,
            configure_global: default_configure_global(repo_root.is_some(), prefer_global),
            database_url: existing_config
                .as_ref()
                .map(|config| config.database.url.clone())
                .unwrap_or_else(|| {
                    "postgresql://memory:<password>@localhost:5432/memory".to_string()
                }),
            api_token: existing_config
                .as_ref()
                .map(|config| config.service.api_token.clone())
                .unwrap_or_else(|| "dev-memory-token".to_string()),
            llm_provider: existing_config
                .as_ref()
                .map(|config| config.llm.provider.clone())
                .unwrap_or_else(|| "openai_compatible".to_string()),
            llm_base_url: existing_config
                .as_ref()
                .map(|config| config.llm.base_url.clone())
                .unwrap_or_else(|| "https://api.openai.com/v1".to_string()),
            llm_api_key_env,
            llm_model: existing_config
                .as_ref()
                .map(|config| config.llm.model.clone())
                .unwrap_or_default(),
            llm_api_key_value,
            initialize_repo: repo_root
                .as_ref()
                .is_some_and(|root| !root.join(".mem").exists()),
            enable_backend_service: false,
            enable_watcher_service: false,
            run_scan: false,
            scan_dry_run: true,
        }
    }

    fn repo_available(&self) -> bool {
        self.repo_root.is_some()
    }
}

fn read_project_slug(repo_root: &Path) -> Option<String> {
    let project_path = repo_root.join(".mem").join("project.toml");
    let content = fs::read_to_string(project_path).ok()?;
    for line in content.lines() {
        let trimmed = line.trim();
        if let Some(value) = trimmed.strip_prefix("slug = ") {
            return Some(value.trim_matches('"').to_string());
        }
    }
    None
}

fn default_configure_global(repo_available: bool, prefer_global: bool) -> bool {
    if repo_available { prefer_global } else { true }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum WizardField {
    ConfigureGlobal,
    Project,
    InitializeRepo,
    EnableWatcher,
    RunScan,
    ScanDryRun,
    DatabaseUrl,
    ApiToken,
    LlmProvider,
    LlmBaseUrl,
    LlmApiKeyEnv,
    LlmModel,
    LlmApiKeyValue,
    EnableBackendService,
    Apply,
    Cancel,
}

#[derive(Clone, Debug)]
struct VisibleField {
    key: WizardField,
    label: &'static str,
    value: String,
    kind: FieldKind,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum FieldKind {
    Toggle,
    Text,
    Action,
}

#[derive(Clone, Debug)]
enum InputMode {
    Normal,
    Editing {
        field: WizardField,
        original: String,
        buffer: String,
    },
}

enum WizardOutcome {
    Cancel,
    Apply(WizardState),
}

struct WizardApp {
    state: WizardState,
    selected: usize,
    input_mode: InputMode,
    status: String,
}

impl WizardApp {
    fn new(cwd: &Path, repo_root: &Path, project: Option<String>, prefer_global: bool) -> Self {
        let state = WizardState::new(cwd, repo_root, project, prefer_global);
        let status = if state.repo_available() {
            "Repo-local setup is the default. Toggle shared/global config on if you need it."
                .to_string()
        } else {
            "No repository detected. The wizard will only configure shared/global files."
                .to_string()
        };
        Self {
            state,
            selected: 0,
            input_mode: InputMode::Normal,
            status,
        }
    }

    fn fields(&self) -> Vec<VisibleField> {
        let mut fields = Vec::new();
        if self.state.repo_available() {
            fields.push(VisibleField {
                key: WizardField::ConfigureGlobal,
                label: "Configure shared/global files",
                value: bool_label(self.state.configure_global),
                kind: FieldKind::Toggle,
            });
            fields.push(VisibleField {
                key: WizardField::Project,
                label: "Project slug",
                value: self.state.project.clone(),
                kind: FieldKind::Text,
            });
            fields.push(VisibleField {
                key: WizardField::InitializeRepo,
                label: "Initialize repo-local files",
                value: bool_label(self.state.initialize_repo),
                kind: FieldKind::Toggle,
            });
            fields.push(VisibleField {
                key: WizardField::EnableWatcher,
                label: "Enable watcher user service",
                value: bool_label(self.state.enable_watcher_service),
                kind: FieldKind::Toggle,
            });
            fields.push(VisibleField {
                key: WizardField::RunScan,
                label: "Run initial project scan",
                value: bool_label(self.state.run_scan),
                kind: FieldKind::Toggle,
            });
            if self.state.run_scan {
                fields.push(VisibleField {
                    key: WizardField::ScanDryRun,
                    label: "Scan dry-run only",
                    value: bool_label(self.state.scan_dry_run),
                    kind: FieldKind::Toggle,
                });
            }
        }

        if self.state.configure_global {
            fields.push(VisibleField {
                key: WizardField::DatabaseUrl,
                label: "Database URL",
                value: mask_database_url(&self.state.database_url),
                kind: FieldKind::Text,
            });
            fields.push(VisibleField {
                key: WizardField::ApiToken,
                label: "Write API token",
                value: secret_label(&self.state.api_token),
                kind: FieldKind::Text,
            });
            fields.push(VisibleField {
                key: WizardField::LlmProvider,
                label: "LLM provider",
                value: self.state.llm_provider.clone(),
                kind: FieldKind::Text,
            });
            fields.push(VisibleField {
                key: WizardField::LlmBaseUrl,
                label: "LLM base URL",
                value: self.state.llm_base_url.clone(),
                kind: FieldKind::Text,
            });
            fields.push(VisibleField {
                key: WizardField::LlmApiKeyEnv,
                label: "LLM API key env var",
                value: self.state.llm_api_key_env.clone(),
                kind: FieldKind::Text,
            });
            fields.push(VisibleField {
                key: WizardField::LlmModel,
                label: "LLM model",
                value: display_empty(&self.state.llm_model),
                kind: FieldKind::Text,
            });
            fields.push(VisibleField {
                key: WizardField::LlmApiKeyValue,
                label: "LLM API key value",
                value: secret_label(&self.state.llm_api_key_value),
                kind: FieldKind::Text,
            });
            if packaged_service_available() {
                fields.push(VisibleField {
                    key: WizardField::EnableBackendService,
                    label: "Enable backend system service",
                    value: bool_label(self.state.enable_backend_service),
                    kind: FieldKind::Toggle,
                });
            }
        }

        fields.push(VisibleField {
            key: WizardField::Apply,
            label: "Apply changes",
            value: "run setup".to_string(),
            kind: FieldKind::Action,
        });
        fields.push(VisibleField {
            key: WizardField::Cancel,
            label: "Cancel",
            value: "exit without writing".to_string(),
            kind: FieldKind::Action,
        });
        fields
    }

    fn handle_key(&mut self, key: KeyEvent) -> Result<Option<WizardOutcome>> {
        let input_mode = std::mem::replace(&mut self.input_mode, InputMode::Normal);
        match input_mode {
            InputMode::Normal => self.handle_normal_key(key),
            InputMode::Editing {
                field,
                original,
                mut buffer,
            } => self.handle_edit_key(key, field, &original, &mut buffer),
        }
    }

    fn handle_normal_key(&mut self, key: KeyEvent) -> Result<Option<WizardOutcome>> {
        let field_count = self.fields().len();
        match key.code {
            KeyCode::Char('q') | KeyCode::Esc => return Ok(Some(WizardOutcome::Cancel)),
            KeyCode::Up | KeyCode::Char('k') => {
                self.selected = self.selected.saturating_sub(1);
            }
            KeyCode::Down | KeyCode::Char('j') | KeyCode::Tab => {
                if self.selected + 1 < field_count {
                    self.selected += 1;
                }
            }
            KeyCode::BackTab => {
                self.selected = self.selected.saturating_sub(1);
            }
            KeyCode::Enter | KeyCode::Char(' ') => {
                return self.activate_selected();
            }
            _ => {}
        }
        self.clamp_selection();
        Ok(None)
    }

    fn handle_edit_key(
        &mut self,
        key: KeyEvent,
        field: WizardField,
        original: &str,
        buffer: &mut String,
    ) -> Result<Option<WizardOutcome>> {
        match key.code {
            KeyCode::Esc => {
                let original = original.to_string();
                self.input_mode = InputMode::Normal;
                self.set_field_value(field, original);
            }
            KeyCode::Enter => {
                let value = buffer.clone();
                self.set_field_value(field, value);
                self.input_mode = InputMode::Normal;
                self.status = "Updated wizard field.".to_string();
            }
            KeyCode::Backspace => {
                buffer.pop();
                self.set_field_value(field, buffer.clone());
                self.input_mode = InputMode::Editing {
                    field,
                    original: original.to_string(),
                    buffer: buffer.clone(),
                };
            }
            KeyCode::Char(ch) => {
                if key.modifiers.contains(KeyModifiers::CONTROL) {
                    self.input_mode = InputMode::Editing {
                        field,
                        original: original.to_string(),
                        buffer: buffer.clone(),
                    };
                    return Ok(None);
                }
                buffer.push(ch);
                self.set_field_value(field, buffer.clone());
                self.input_mode = InputMode::Editing {
                    field,
                    original: original.to_string(),
                    buffer: buffer.clone(),
                };
            }
            _ => {
                self.input_mode = InputMode::Editing {
                    field,
                    original: original.to_string(),
                    buffer: buffer.clone(),
                };
            }
        }
        Ok(None)
    }

    fn activate_selected(&mut self) -> Result<Option<WizardOutcome>> {
        let fields = self.fields();
        let Some(selected) = fields.get(self.selected) else {
            return Ok(None);
        };

        match selected.kind {
            FieldKind::Toggle => {
                self.toggle(selected.key);
                self.status = format!("Updated {}.", selected.label.to_lowercase());
            }
            FieldKind::Text => {
                let current = self.raw_value(selected.key);
                self.input_mode = InputMode::Editing {
                    field: selected.key,
                    original: current.clone(),
                    buffer: current,
                };
                self.status = format!("Editing {}. Enter saves, Esc cancels.", selected.label);
            }
            FieldKind::Action => match selected.key {
                WizardField::Apply => return Ok(Some(WizardOutcome::Apply(self.state.clone()))),
                WizardField::Cancel => return Ok(Some(WizardOutcome::Cancel)),
                _ => {}
            },
        }

        self.clamp_selection();
        Ok(None)
    }

    fn toggle(&mut self, field: WizardField) {
        match field {
            WizardField::ConfigureGlobal => {
                self.state.configure_global = !self.state.configure_global;
                if !self.state.configure_global {
                    self.state.enable_backend_service = false;
                }
            }
            WizardField::InitializeRepo => {
                self.state.initialize_repo = !self.state.initialize_repo;
            }
            WizardField::EnableWatcher => {
                self.state.enable_watcher_service = !self.state.enable_watcher_service;
            }
            WizardField::RunScan => {
                self.state.run_scan = !self.state.run_scan;
                if !self.state.run_scan {
                    self.state.scan_dry_run = true;
                }
            }
            WizardField::ScanDryRun => {
                self.state.scan_dry_run = !self.state.scan_dry_run;
            }
            WizardField::EnableBackendService => {
                self.state.enable_backend_service = !self.state.enable_backend_service;
            }
            _ => {}
        }
    }

    fn raw_value(&self, field: WizardField) -> String {
        match field {
            WizardField::Project => self.state.project.clone(),
            WizardField::DatabaseUrl => self.state.database_url.clone(),
            WizardField::ApiToken => self.state.api_token.clone(),
            WizardField::LlmProvider => self.state.llm_provider.clone(),
            WizardField::LlmBaseUrl => self.state.llm_base_url.clone(),
            WizardField::LlmApiKeyEnv => self.state.llm_api_key_env.clone(),
            WizardField::LlmModel => self.state.llm_model.clone(),
            WizardField::LlmApiKeyValue => self.state.llm_api_key_value.clone(),
            _ => String::new(),
        }
    }

    fn set_field_value(&mut self, field: WizardField, value: String) {
        match field {
            WizardField::Project => self.state.project = value,
            WizardField::DatabaseUrl => self.state.database_url = value,
            WizardField::ApiToken => self.state.api_token = value,
            WizardField::LlmProvider => self.state.llm_provider = value,
            WizardField::LlmBaseUrl => self.state.llm_base_url = value,
            WizardField::LlmApiKeyEnv => self.state.llm_api_key_env = value,
            WizardField::LlmModel => self.state.llm_model = value,
            WizardField::LlmApiKeyValue => self.state.llm_api_key_value = value,
            _ => {}
        }
    }

    fn clamp_selection(&mut self) {
        let field_count = self.fields().len();
        if field_count == 0 {
            self.selected = 0;
        } else if self.selected >= field_count {
            self.selected = field_count - 1;
        }
    }
}

fn draw(frame: &mut ratatui::Frame<'_>, app: &WizardApp) {
    let area = frame.area();
    let chunks = Layout::default()
        .direction(Direction::Vertical)
        .constraints([
            Constraint::Length(3),
            Constraint::Min(12),
            Constraint::Length(4),
        ])
        .split(area);

    let title = Paragraph::new(vec![
        Line::from(Span::styled(
            "Memory Layer Wizard",
            Style::default()
                .fg(Color::Cyan)
                .add_modifier(Modifier::BOLD),
        )),
        Line::from(Span::styled(
            "Repo-local setup is the default. Use the toggle below only if you also want to edit shared/global config.",
            Style::default().fg(Color::Gray),
        )),
    ])
    .block(Block::default().borders(Borders::ALL).title("Setup"));
    frame.render_widget(title, chunks[0]);

    let body = Layout::default()
        .direction(Direction::Horizontal)
        .constraints([Constraint::Percentage(48), Constraint::Percentage(52)])
        .split(chunks[1]);

    draw_form(frame, body[0], app);
    draw_summary(frame, body[1], app);

    let footer = footer_text(app);
    let footer = Paragraph::new(footer)
        .wrap(Wrap { trim: false })
        .block(Block::default().borders(Borders::ALL).title("Help"));
    frame.render_widget(footer, chunks[2]);
}

fn draw_form(frame: &mut ratatui::Frame<'_>, area: Rect, app: &WizardApp) {
    let fields = app.fields();
    let inner_height = area.height.saturating_sub(2) as usize;
    let scroll = if app.selected >= inner_height {
        app.selected + 1 - inner_height
    } else {
        0
    };

    let lines = fields
        .iter()
        .enumerate()
        .skip(scroll)
        .take(inner_height)
        .map(|(index, field)| {
            let selected = index == app.selected;
            let marker = if selected { ">" } else { " " };
            let base_style = match field.kind {
                FieldKind::Action => Style::default().fg(Color::Yellow),
                FieldKind::Toggle => Style::default().fg(Color::Green),
                FieldKind::Text => Style::default().fg(Color::White),
            };
            let style = if selected {
                base_style
                    .bg(Color::Blue)
                    .fg(Color::Black)
                    .add_modifier(Modifier::BOLD)
            } else {
                base_style
            };
            let content = format!("{marker} {:<28} {}", field.label, field.value);
            Line::from(Span::styled(content, style))
        })
        .collect::<Vec<_>>();

    let form = Paragraph::new(lines)
        .wrap(Wrap { trim: false })
        .block(Block::default().borders(Borders::ALL).title("Fields"));
    frame.render_widget(form, area);
}

fn draw_summary(frame: &mut ratatui::Frame<'_>, area: Rect, app: &WizardApp) {
    let mut lines = Vec::new();
    lines.push(Line::from(Span::styled(
        "Planned changes",
        Style::default()
            .fg(Color::LightCyan)
            .add_modifier(Modifier::BOLD),
    )));
    lines.push(Line::from(""));

    if let Some(repo_root) = &app.state.repo_root {
        lines.push(Line::from(format!("Repo root: {}", repo_root.display())));
        lines.push(Line::from(format!("Project slug: {}", app.state.project)));
        lines.push(Line::from(format!(
            "Repo-local files: {}",
            if app.state.initialize_repo {
                "create/update .mem and .agents/skills"
            } else {
                "leave current repo files unchanged"
            }
        )));
        lines.push(Line::from(format!(
            "Watcher service: {}",
            if app.state.enable_watcher_service {
                "enable user service"
            } else {
                "skip"
            }
        )));
        lines.push(Line::from(format!(
            "Initial scan: {}",
            if app.state.run_scan {
                if app.state.scan_dry_run {
                    "run dry-run after setup"
                } else {
                    "run write mode after setup"
                }
            } else {
                "skip"
            }
        )));
    } else {
        lines.push(Line::from("No repository detected."));
    }

    lines.push(Line::from(""));
    lines.push(Line::from(Span::styled(
        "Shared/global config",
        Style::default()
            .fg(Color::LightCyan)
            .add_modifier(Modifier::BOLD),
    )));
    lines.push(Line::from(format!(
        "Mode: {}",
        if app.state.configure_global {
            "configure shared files"
        } else {
            "leave shared files unchanged"
        }
    )));
    lines.push(Line::from(format!(
        "Global config: {}",
        app.state.global_config_path.display()
    )));
    lines.push(Line::from(format!(
        "Shared env file: {}",
        app.state.shared_env_path.display()
    )));

    if app.state.configure_global {
        lines.push(Line::from(format!(
            "Database URL: {}",
            mask_database_url(&app.state.database_url)
        )));
        lines.push(Line::from(format!(
            "API token: {}",
            secret_label(&app.state.api_token)
        )));
        lines.push(Line::from(format!(
            "LLM model: {}",
            display_empty(&app.state.llm_model)
        )));
        lines.push(Line::from(format!(
            "LLM API key: {} via {}",
            secret_label(&app.state.llm_api_key_value),
            app.state.llm_api_key_env
        )));
        lines.push(Line::from(format!(
            "Backend service: {}",
            if app.state.enable_backend_service {
                "enable/start memory-layer.service"
            } else {
                "skip"
            }
        )));
    }

    lines.push(Line::from(""));
    lines.push(Line::from(Span::styled(
        "Notes",
        Style::default()
            .fg(Color::LightCyan)
            .add_modifier(Modifier::BOLD),
    )));
    lines.push(Line::from(app.status.clone()));
    if app.state.run_scan && !app.state.configure_global {
        lines.push(Line::from(
            "Scan will rely on any existing shared config and running backend.",
        ));
    }

    let summary = Paragraph::new(lines)
        .wrap(Wrap { trim: false })
        .block(Block::default().borders(Borders::ALL).title("Review"));
    frame.render_widget(summary, area);
}

fn footer_text(app: &WizardApp) -> Vec<Line<'static>> {
    let mut lines = vec![Line::from(
        "Up/Down or j/k move. Enter edits or activates. Space toggles. q cancels.",
    )];
    match &app.input_mode {
        InputMode::Normal => lines.push(Line::from(
            "Selected text fields open in-place editing. Apply runs the selected actions after terminal restore.",
        )),
        InputMode::Editing { field, .. } => lines.push(Line::from(format!(
            "Editing {}. Type to change it, Enter saves, Esc cancels.",
            field_label(*field)
        ))),
    }
    lines
}

fn bool_label(value: bool) -> String {
    if value {
        "[x]".to_string()
    } else {
        "[ ]".to_string()
    }
}

fn secret_label(value: &str) -> String {
    if value.trim().is_empty() {
        "<unset>".to_string()
    } else {
        "<configured>".to_string()
    }
}

fn display_empty(value: &str) -> String {
    if value.trim().is_empty() {
        "<empty>".to_string()
    } else {
        value.to_string()
    }
}

fn field_label(field: WizardField) -> &'static str {
    match field {
        WizardField::ConfigureGlobal => "Configure shared/global files",
        WizardField::Project => "Project slug",
        WizardField::InitializeRepo => "Initialize repo-local files",
        WizardField::EnableWatcher => "Enable watcher user service",
        WizardField::RunScan => "Run initial project scan",
        WizardField::ScanDryRun => "Scan dry-run only",
        WizardField::DatabaseUrl => "Database URL",
        WizardField::ApiToken => "Write API token",
        WizardField::LlmProvider => "LLM provider",
        WizardField::LlmBaseUrl => "LLM base URL",
        WizardField::LlmApiKeyEnv => "LLM API key env var",
        WizardField::LlmModel => "LLM model",
        WizardField::LlmApiKeyValue => "LLM API key value",
        WizardField::EnableBackendService => "Enable backend system service",
        WizardField::Apply => "Apply changes",
        WizardField::Cancel => "Cancel",
    }
}

async fn apply(state: WizardState) -> Result<()> {
    let mut outputs = Vec::new();

    if state.configure_global {
        write_global_config(&state)?;
        outputs.push(format!(
            "Updated shared config at {}",
            state.global_config_path.display()
        ));
        if !state.llm_api_key_value.trim().is_empty() {
            write_shared_env_file(
                &state.shared_env_path,
                &state.llm_api_key_env,
                &state.llm_api_key_value,
            )?;
            outputs.push(format!(
                "Updated shared env file at {}",
                state.shared_env_path.display()
            ));
        }
    } else {
        outputs.push("Left shared/global files unchanged.".to_string());
    }

    if let Some(repo_root) = &state.repo_root {
        if state.initialize_repo {
            repair_repo_bootstrap(repo_root, &state.project)?;
            outputs.push(format!(
                "Ensured repo-local Memory Layer files exist for project `{}` at {}.",
                state.project,
                repo_root.display()
            ));
        }
        if state.enable_watcher_service {
            outputs.push(enable_watch_service(repo_root, &state.project)?);
        }
    }

    if state.configure_global && state.enable_backend_service {
        run_systemctl_system(["daemon-reload"])?;
        run_systemctl_system(["enable", "--now", "memory-layer.service"])?;
        outputs.push("Enabled memory-layer.service".to_string());
    }

    if state.run_scan {
        let config = AppConfig::load_from_path(None).context("reload config after wizard")?;
        let client = Client::builder()
            .timeout(config.service.request_timeout)
            .build()
            .context("build http client")?;
        let api = ApiClient::new(client, config);
        let repo_root = state
            .repo_root
            .as_deref()
            .ok_or_else(|| anyhow::anyhow!("scan requested without a repository"))?;
        let report =
            scan::run_scan(&api, repo_root, &state.project, None, state.scan_dry_run).await?;
        print_scan_report(&report);
    }

    println!("Wizard applied.\n");
    for output in outputs {
        println!("{output}\n");
    }

    if let Some(repo_root) = &state.repo_root {
        let report = run_doctor(None, repo_root, &state.project, false).await?;
        print_doctor_report(&report);
    }

    Ok(())
}

fn setup_terminal() -> Result<Terminal<CrosstermBackend<io::Stdout>>> {
    enable_raw_mode().context("enable raw mode")?;
    let mut stdout = io::stdout();
    execute!(stdout, EnterAlternateScreen).context("enter alternate screen")?;
    Terminal::new(CrosstermBackend::new(stdout)).context("create terminal")
}

fn restore_terminal(mut terminal: Terminal<CrosstermBackend<io::Stdout>>) -> Result<()> {
    disable_raw_mode().context("disable raw mode")?;
    execute!(terminal.backend_mut(), LeaveAlternateScreen).context("leave alternate screen")?;
    terminal.show_cursor().context("show cursor")
}

fn write_global_config(state: &WizardState) -> Result<()> {
    let parent = state
        .global_config_path
        .parent()
        .ok_or_else(|| anyhow::anyhow!("global config path has no parent"))?;
    fs::create_dir_all(parent).with_context(|| format!("create {}", parent.display()))?;
    fs::write(&state.global_config_path, render_global_config(state))
        .with_context(|| format!("write {}", state.global_config_path.display()))
}

fn render_global_config(state: &WizardState) -> String {
    format!(
        "# Shared Memory Layer defaults and secrets.\n# Repo-local overrides should live in .mem/config.toml inside each project.\n\n[service]\nbind_addr = \"127.0.0.1:4040\"\ncapnp_unix_socket = \"/tmp/memory-layer.capnp.sock\"\ncapnp_tcp_addr = \"127.0.0.1:4041\"\napi_token = \"{}\"\nrequest_timeout = \"30s\"\n\n[database]\nurl = \"{}\"\n\n[features]\nllm_curation = false\n\n[llm]\nprovider = \"{}\"\nbase_url = \"{}\"\napi_key_env = \"{}\"\nmodel = \"{}\"\ntemperature = 0.0\nmax_input_bytes = 120000\nmax_output_tokens = 3000\n\n[automation]\nenabled = false\nmode = \"suggest\"\npoll_interval = \"10s\"\nidle_threshold = \"5m\"\nmin_changed_files = 2\nrequire_passing_test = false\nignored_paths = [\".git/\", \"target/\", \".memory-layer/\"]\n# repo_root = \"/path/to/repo\"\n# audit_log_path = \"/path/to/repo/.memory-layer/automation.log\"\n# state_file_path = \"/path/to/repo/.memory-layer/automation-state.json\"\n",
        state.api_token,
        state.database_url,
        state.llm_provider,
        state.llm_base_url,
        state.llm_api_key_env,
        state.llm_model,
    )
}

#[cfg(test)]
mod tests {
    use std::fs;

    use super::{default_configure_global, read_project_slug};

    #[test]
    fn wizard_defaults_to_local_scope_inside_repo() {
        assert!(!default_configure_global(true, false));
        assert!(default_configure_global(true, true));
    }

    #[test]
    fn wizard_defaults_to_global_outside_repo() {
        assert!(default_configure_global(false, false));
        assert!(default_configure_global(false, true));
    }

    #[test]
    fn wizard_reads_existing_project_slug() {
        let repo_root = std::env::temp_dir().join(format!(
            "mem-wizard-{}-{}",
            std::process::id(),
            chrono::Utc::now().timestamp_nanos_opt().unwrap_or_default()
        ));
        fs::create_dir_all(repo_root.join(".mem")).unwrap();
        fs::write(
            repo_root.join(".mem/project.toml"),
            "slug = \"homelab\"\nrepo_root = \"/tmp/homelab\"\n",
        )
        .unwrap();

        assert_eq!(read_project_slug(&repo_root).as_deref(), Some("homelab"));

        let _ = fs::remove_dir_all(repo_root);
    }
}
