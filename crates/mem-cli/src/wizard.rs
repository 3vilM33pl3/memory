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
use mem_api::{AppConfig, AutomationMode, discover_global_config_path};
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
    ApiClient, DoctorReport, DoctorStatus, default_global_config_path, enable_watch_service,
    mask_database_url, packaged_service_available, render_project_metadata,
    repair_repo_bootstrap, run_doctor, run_systemctl_system, shared_env_lookup,
    shared_env_path_for_config, write_shared_env_file,
};
use crate::scan::{self, ScanReport};

pub(crate) async fn run(
    cwd: &Path,
    repo_root: &Path,
    project: Option<String>,
    prefer_global: bool,
) -> Result<()> {
    let mut terminal = setup_terminal()?;
    let mut app = WizardApp::new(cwd, repo_root, project, prefer_global);

    loop {
        terminal.draw(|frame| draw(frame, &app))?;
        if event::poll(Duration::from_millis(200))? {
            match event::read()? {
                Event::Key(key) => {
                    if app.handle_key(key).await? {
                        break;
                    }
                }
                _ => {}
            }
        }
    }

    restore_terminal(terminal)?;
    if let Some(message) = &app.exit_message {
        println!("{message}");
    }
    Ok(())
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum Step {
    Welcome,
    Shared,
    Repo,
    Services,
    Review,
    Result,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum ToggleChoice {
    Yes,
    No,
}

impl ToggleChoice {
    fn toggle(&mut self) {
        *self = match self {
            Self::Yes => Self::No,
            Self::No => Self::Yes,
        };
    }

    fn is_yes(self) -> bool {
        matches!(self, Self::Yes)
    }

    fn label(self) -> &'static str {
        match self {
            Self::Yes => "Yes",
            Self::No => "No",
        }
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum ScanChoice {
    Skip,
    DryRun,
    Write,
}

impl ScanChoice {
    fn cycle(&mut self) {
        *self = match self {
            Self::Skip => Self::DryRun,
            Self::DryRun => Self::Write,
            Self::Write => Self::Skip,
        };
    }

    fn label(self) -> &'static str {
        match self {
            Self::Skip => "Skip",
            Self::DryRun => "Dry-run",
            Self::Write => "Write",
        }
    }
}

#[derive(Clone, Debug)]
struct WizardDraft {
    global_config_path: PathBuf,
    shared_env_path: PathBuf,
    repo_root: Option<PathBuf>,
    project: String,
    include_global: ToggleChoice,
    database_url: String,
    api_token: String,
    llm_provider: String,
    llm_base_url: String,
    llm_api_key_env: String,
    llm_model: String,
    llm_api_key_value: String,
    local_database_url: String,
    local_llm_api_key_value: String,
    apply_repo_setup: ToggleChoice,
    automation_enabled: ToggleChoice,
    automation_mode: AutomationMode,
    automation_poll_interval: String,
    automation_idle_threshold: String,
    automation_min_changed_files: String,
    automation_require_passing_test: ToggleChoice,
    automation_ignored_paths: String,
    enable_backend_service: ToggleChoice,
    enable_watcher_service: ToggleChoice,
    scan_choice: ScanChoice,
    run_doctor: ToggleChoice,
}

impl WizardDraft {
    fn new(cwd: &Path, repo_root: &Path, project: Option<String>, prefer_global: bool) -> Self {
        let global_config_path =
            discover_global_config_path().unwrap_or_else(default_global_config_path);
        let shared_env_path = shared_env_path_for_config(&global_config_path);
        let repo_available = repo_root != cwd || repo_root.join(".git").exists();
        let repo_root = repo_available.then(|| repo_root.to_path_buf());
        let global_config = AppConfig::load_from_path(Some(global_config_path.clone())).ok();
        let existing_config = if repo_available {
            AppConfig::load_from_path(None).ok()
        } else {
            global_config.clone()
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

        let llm_api_key_env = global_config
            .as_ref()
            .map(|config| config.llm.api_key_env.clone())
            .or_else(|| {
                existing_config
                    .as_ref()
                    .map(|config| config.llm.api_key_env.clone())
            })
            .unwrap_or_else(|| "OPENAI_API_KEY".to_string());

        let llm_api_key_value = shared_env_lookup(&shared_env_path, &llm_api_key_env)
            .or_else(|| env::var(&llm_api_key_env).ok())
            .unwrap_or_default();
        let local_database_url = repo_root
            .as_deref()
            .and_then(read_local_database_override)
            .unwrap_or_default();
        let local_llm_api_key_value = repo_root
            .as_deref()
            .and_then(|root| read_local_env_override(root, &llm_api_key_env))
            .unwrap_or_default();

        Self {
            global_config_path,
            shared_env_path,
            repo_root: repo_root.clone(),
            project,
            include_global: default_include_global(repo_root.is_some(), prefer_global),
            database_url: global_config
                .as_ref()
                .map(|config| config.database.url.clone())
                .unwrap_or_else(|| {
                    "postgresql://memory:<password>@localhost:5432/memory".to_string()
                }),
            api_token: global_config
                .as_ref()
                .map(|config| config.service.api_token.clone())
                .unwrap_or_else(|| "dev-memory-token".to_string()),
            llm_provider: global_config
                .as_ref()
                .map(|config| config.llm.provider.clone())
                .unwrap_or_else(|| "openai_compatible".to_string()),
            llm_base_url: global_config
                .as_ref()
                .map(|config| config.llm.base_url.clone())
                .unwrap_or_else(|| "https://api.openai.com/v1".to_string()),
            llm_api_key_env,
            llm_model: global_config
                .as_ref()
                .map(|config| config.llm.model.clone())
                .unwrap_or_default(),
            llm_api_key_value,
            local_database_url,
            local_llm_api_key_value,
            apply_repo_setup: if repo_root.is_some() {
                ToggleChoice::Yes
            } else {
                ToggleChoice::No
            },
            automation_enabled: existing_config
                .as_ref()
                .map(|config| toggle_from_bool(config.automation.enabled))
                .unwrap_or(ToggleChoice::No),
            automation_mode: existing_config
                .as_ref()
                .map(|config| config.automation.mode.clone())
                .unwrap_or(AutomationMode::Suggest),
            automation_poll_interval: existing_config
                .as_ref()
                .map(|config| duration_to_string(config.automation.poll_interval))
                .unwrap_or_else(|| "10s".to_string()),
            automation_idle_threshold: existing_config
                .as_ref()
                .map(|config| duration_to_string(config.automation.idle_threshold))
                .unwrap_or_else(|| "5m".to_string()),
            automation_min_changed_files: existing_config
                .as_ref()
                .map(|config| config.automation.min_changed_files.to_string())
                .unwrap_or_else(|| "2".to_string()),
            automation_require_passing_test: existing_config
                .as_ref()
                .map(|config| toggle_from_bool(config.automation.require_passing_test))
                .unwrap_or(ToggleChoice::No),
            automation_ignored_paths: existing_config
                .as_ref()
                .map(|config| config.automation.ignored_paths.join(", "))
                .filter(|value| !value.trim().is_empty())
                .unwrap_or_else(|| ".git/, target/, .mem/".to_string()),
            enable_backend_service: ToggleChoice::No,
            enable_watcher_service: ToggleChoice::No,
            scan_choice: ScanChoice::Skip,
            run_doctor: ToggleChoice::No,
        }
    }

    fn repo_available(&self) -> bool {
        self.repo_root.is_some()
    }

    fn includes_global(&self) -> bool {
        self.include_global.is_yes()
    }

    fn applies_repo_setup(&self) -> bool {
        self.apply_repo_setup.is_yes() && self.repo_available()
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum FieldKey {
    IncludeGlobal,
    DatabaseUrl,
    ApiToken,
    LlmModel,
    LlmApiKeyEnv,
    LlmApiKeyValue,
    Project,
    LocalDatabaseUrl,
    LocalLlmApiKeyValue,
    ApplyRepoSetup,
    AutomationEnabled,
    AutomationMode,
    AutomationPollInterval,
    AutomationIdleThreshold,
    AutomationMinChangedFiles,
    AutomationRequirePassingTest,
    AutomationIgnoredPaths,
    EnableBackendService,
    EnableWatcher,
    ScanChoice,
    RunDoctor,
    Next,
    Back,
    Apply,
    Finish,
    Cancel,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum ItemKind {
    Text,
    Choice,
    Action,
    Static,
}

struct StepItem {
    key: FieldKey,
    label: String,
    value: String,
    kind: ItemKind,
}

#[derive(Clone, Debug)]
enum InputMode {
    Normal,
    Editing {
        field: FieldKey,
        original: String,
        buffer: String,
    },
}

struct WizardResult {
    title: String,
    lines: Vec<String>,
    success: bool,
}

struct WizardApp {
    draft: WizardDraft,
    step: Step,
    selected: usize,
    input_mode: InputMode,
    status: String,
    result: Option<WizardResult>,
    exit_message: Option<String>,
}

impl WizardApp {
    fn new(cwd: &Path, repo_root: &Path, project: Option<String>, prefer_global: bool) -> Self {
        let draft = WizardDraft::new(cwd, repo_root, project, prefer_global);
        let status = if draft.repo_available() {
            "Step 1 of 5. Choose whether this run should also edit shared/global settings."
                .to_string()
        } else {
            "Step 1 of 4. No repository detected, so this run is shared/global setup only."
                .to_string()
        };
        Self {
            draft,
            step: Step::Welcome,
            selected: 0,
            input_mode: InputMode::Normal,
            status,
            result: None,
            exit_message: None,
        }
    }

    async fn handle_key(&mut self, key: KeyEvent) -> Result<bool> {
        if self.step == Step::Result {
            return Ok(matches!(key.code, KeyCode::Enter | KeyCode::Char('q') | KeyCode::Esc));
        }

        let input_mode = std::mem::replace(&mut self.input_mode, InputMode::Normal);
        match input_mode {
            InputMode::Normal => self.handle_normal_key(key).await,
            InputMode::Editing {
                field,
                original,
                mut buffer,
            } => self.handle_edit_key(key, field, &original, &mut buffer),
        }
    }

    async fn handle_normal_key(&mut self, key: KeyEvent) -> Result<bool> {
        let item_count = self.current_items().len();
        match key.code {
            KeyCode::Char('q') | KeyCode::Esc => {
                self.exit_message = Some("Wizard cancelled.".to_string());
                return Ok(true);
            }
            KeyCode::Up | KeyCode::Char('k') => {
                self.selected = self.selected.saturating_sub(1);
            }
            KeyCode::Down | KeyCode::Char('j') | KeyCode::Tab => {
                if self.selected + 1 < item_count {
                    self.selected += 1;
                }
            }
            KeyCode::BackTab => {
                self.selected = self.selected.saturating_sub(1);
            }
            KeyCode::Enter | KeyCode::Char(' ') => return self.activate_selected().await,
            _ => {}
        }
        self.clamp_selection();
        Ok(false)
    }

    fn handle_edit_key(
        &mut self,
        key: KeyEvent,
        field: FieldKey,
        original: &str,
        buffer: &mut String,
    ) -> Result<bool> {
        match key.code {
            KeyCode::Esc => {
                self.set_field_value(field, original.to_string());
                self.input_mode = InputMode::Normal;
            }
            KeyCode::Enter => {
                self.set_field_value(field, buffer.clone());
                self.input_mode = InputMode::Normal;
                self.status = format!("Updated {}.", field_label(field));
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
                    return Ok(false);
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
        Ok(false)
    }

    async fn activate_selected(&mut self) -> Result<bool> {
        let items = self.current_items();
        let Some(item) = items.get(self.selected) else {
            return Ok(false);
        };

        match item.kind {
            ItemKind::Static => {}
            ItemKind::Choice => {
                self.cycle_choice(item.key);
                self.status = format!("Updated {}.", field_label(item.key));
            }
            ItemKind::Text => {
                let current = self.current_value(item.key);
                self.input_mode = InputMode::Editing {
                    field: item.key,
                    original: current.clone(),
                    buffer: current,
                };
                self.status = format!("Editing {}. Enter saves, Esc cancels.", field_label(item.key));
            }
            ItemKind::Action => match item.key {
                FieldKey::Next => self.go_next(),
                FieldKey::Back => self.go_back(),
                FieldKey::Cancel => {
                    self.exit_message = Some("Wizard cancelled.".to_string());
                    return Ok(true);
                }
                FieldKey::Apply => {
                    self.apply().await;
                }
                FieldKey::Finish => return Ok(true),
                _ => {}
            },
        }

        self.clamp_selection();
        Ok(false)
    }

    fn go_next(&mut self) {
        self.step = next_step(self.step, &self.draft);
        self.selected = 0;
        self.status = step_status(self.step, &self.draft).to_string();
    }

    fn go_back(&mut self) {
        self.step = previous_step(self.step, &self.draft);
        self.selected = 0;
        self.status = step_status(self.step, &self.draft).to_string();
    }

    async fn apply(&mut self) {
        match apply_draft(&self.draft).await {
            Ok(result) => {
                self.result = Some(result);
                self.step = Step::Result;
                self.selected = 0;
            }
            Err(error) => {
                self.result = Some(WizardResult {
                    title: "Apply failed".to_string(),
                    lines: vec![error.to_string()],
                    success: false,
                });
                self.step = Step::Result;
                self.selected = 0;
            }
        }
    }

    fn current_items(&self) -> Vec<StepItem> {
        match self.step {
            Step::Welcome => welcome_items(&self.draft),
            Step::Shared => shared_items(&self.draft),
            Step::Repo => repo_items(&self.draft),
            Step::Services => service_items(&self.draft),
            Step::Review => review_items(),
            Step::Result => result_items(),
        }
    }

    fn cycle_choice(&mut self, field: FieldKey) {
        match field {
            FieldKey::IncludeGlobal => self.draft.include_global.toggle(),
            FieldKey::ApplyRepoSetup => self.draft.apply_repo_setup.toggle(),
            FieldKey::AutomationEnabled => self.draft.automation_enabled.toggle(),
            FieldKey::AutomationRequirePassingTest => {
                self.draft.automation_require_passing_test.toggle()
            }
            FieldKey::EnableBackendService => self.draft.enable_backend_service.toggle(),
            FieldKey::EnableWatcher => self.draft.enable_watcher_service.toggle(),
            FieldKey::RunDoctor => self.draft.run_doctor.toggle(),
            FieldKey::ScanChoice => self.draft.scan_choice.cycle(),
            FieldKey::AutomationMode => {
                self.draft.automation_mode = match self.draft.automation_mode {
                    AutomationMode::Suggest => AutomationMode::Auto,
                    AutomationMode::Auto => AutomationMode::Suggest,
                };
            }
            _ => {}
        }
    }

    fn current_value(&self, field: FieldKey) -> String {
        match field {
            FieldKey::DatabaseUrl => self.draft.database_url.clone(),
            FieldKey::ApiToken => self.draft.api_token.clone(),
            FieldKey::LlmModel => self.draft.llm_model.clone(),
            FieldKey::LlmApiKeyEnv => self.draft.llm_api_key_env.clone(),
            FieldKey::LlmApiKeyValue => self.draft.llm_api_key_value.clone(),
            FieldKey::Project => self.draft.project.clone(),
            FieldKey::LocalDatabaseUrl => self.draft.local_database_url.clone(),
            FieldKey::LocalLlmApiKeyValue => self.draft.local_llm_api_key_value.clone(),
            FieldKey::AutomationPollInterval => self.draft.automation_poll_interval.clone(),
            FieldKey::AutomationIdleThreshold => self.draft.automation_idle_threshold.clone(),
            FieldKey::AutomationMinChangedFiles => {
                self.draft.automation_min_changed_files.clone()
            }
            FieldKey::AutomationIgnoredPaths => self.draft.automation_ignored_paths.clone(),
            _ => String::new(),
        }
    }

    fn set_field_value(&mut self, field: FieldKey, value: String) {
        match field {
            FieldKey::DatabaseUrl => self.draft.database_url = value,
            FieldKey::ApiToken => self.draft.api_token = value,
            FieldKey::LlmModel => self.draft.llm_model = value,
            FieldKey::LlmApiKeyEnv => self.draft.llm_api_key_env = value,
            FieldKey::LlmApiKeyValue => self.draft.llm_api_key_value = value,
            FieldKey::Project => self.draft.project = value,
            FieldKey::LocalDatabaseUrl => self.draft.local_database_url = value,
            FieldKey::LocalLlmApiKeyValue => self.draft.local_llm_api_key_value = value,
            FieldKey::AutomationPollInterval => self.draft.automation_poll_interval = value,
            FieldKey::AutomationIdleThreshold => self.draft.automation_idle_threshold = value,
            FieldKey::AutomationMinChangedFiles => self.draft.automation_min_changed_files = value,
            FieldKey::AutomationIgnoredPaths => self.draft.automation_ignored_paths = value,
            _ => {}
        }
    }

    fn clamp_selection(&mut self) {
        let count = self.current_items().len();
        if count == 0 {
            self.selected = 0;
        } else if self.selected >= count {
            self.selected = count - 1;
        }
    }
}

fn welcome_items(draft: &WizardDraft) -> Vec<StepItem> {
    let mut items = vec![StepItem {
        key: FieldKey::IncludeGlobal,
        label: "Include shared/global setup".to_string(),
        value: draft.include_global.label().to_string(),
        kind: if draft.repo_available() {
            ItemKind::Choice
        } else {
            ItemKind::Static
        },
    }];
    items.push(StepItem {
        key: FieldKey::Next,
        label: "Next".to_string(),
        value: next_step_label(next_step(Step::Welcome, draft)).to_string(),
        kind: ItemKind::Action,
    });
    items.push(action_item(FieldKey::Cancel, "Cancel"));
    items
}

fn shared_items(draft: &WizardDraft) -> Vec<StepItem> {
    vec![
        text_item(FieldKey::DatabaseUrl, "Database URL", &mask_database_url(&draft.database_url)),
        text_item(FieldKey::ApiToken, "Write API token", &secret_label(&draft.api_token)),
        text_item(FieldKey::LlmModel, "LLM model", &display_empty(&draft.llm_model)),
        text_item(
            FieldKey::LlmApiKeyEnv,
            "LLM API key env var",
            &draft.llm_api_key_env,
        ),
        text_item(
            FieldKey::LlmApiKeyValue,
            "LLM API key value",
            &secret_label(&draft.llm_api_key_value),
        ),
        action_item(FieldKey::Back, "Back"),
        StepItem {
            key: FieldKey::Next,
            label: "Next".to_string(),
            value: next_step_label(next_step(Step::Shared, draft)).to_string(),
            kind: ItemKind::Action,
        },
        action_item(FieldKey::Cancel, "Cancel"),
    ]
}

fn repo_items(draft: &WizardDraft) -> Vec<StepItem> {
    vec![
        text_item(FieldKey::Project, "Project slug", &draft.project),
        choice_item(
            FieldKey::ApplyRepoSetup,
            "Apply repo-local setup",
            draft.apply_repo_setup.label(),
        ),
        text_item(
            FieldKey::LocalDatabaseUrl,
            "Local DB override",
            &display_override(&draft.local_database_url),
        ),
        text_item(
            FieldKey::LocalLlmApiKeyValue,
            "Local LLM API key",
            &secret_override_label(&draft.local_llm_api_key_value),
        ),
        choice_item(
            FieldKey::AutomationEnabled,
            "Automation enabled",
            draft.automation_enabled.label(),
        ),
        choice_item(
            FieldKey::AutomationMode,
            "Automation mode",
            automation_mode_label(&draft.automation_mode),
        ),
        text_item(
            FieldKey::AutomationPollInterval,
            "Poll interval",
            &draft.automation_poll_interval,
        ),
        text_item(
            FieldKey::AutomationIdleThreshold,
            "Idle threshold",
            &draft.automation_idle_threshold,
        ),
        text_item(
            FieldKey::AutomationMinChangedFiles,
            "Min changed files",
            &draft.automation_min_changed_files,
        ),
        choice_item(
            FieldKey::AutomationRequirePassingTest,
            "Require passing test",
            draft.automation_require_passing_test.label(),
        ),
        text_item(
            FieldKey::AutomationIgnoredPaths,
            "Ignored paths",
            &draft.automation_ignored_paths,
        ),
        action_item(FieldKey::Back, "Back"),
        StepItem {
            key: FieldKey::Next,
            label: "Next".to_string(),
            value: next_step_label(next_step(Step::Repo, draft)).to_string(),
            kind: ItemKind::Action,
        },
        action_item(FieldKey::Cancel, "Cancel"),
    ]
}

fn service_items(draft: &WizardDraft) -> Vec<StepItem> {
    let mut items = Vec::new();
    if draft.repo_available() {
        items.push(choice_item(
            FieldKey::EnableWatcher,
            "Enable watcher user service",
            draft.enable_watcher_service.label(),
        ));
    }
    if draft.includes_global() && packaged_service_available() {
        items.push(choice_item(
            FieldKey::EnableBackendService,
            "Enable backend system service",
            draft.enable_backend_service.label(),
        ));
    }
    items.push(choice_item(
        FieldKey::ScanChoice,
        "Initial scan",
        draft.scan_choice.label(),
    ));
    items.push(choice_item(
        FieldKey::RunDoctor,
        "Run doctor after setup",
        draft.run_doctor.label(),
    ));
    items.push(action_item(FieldKey::Back, "Back"));
    items.push(action_item(FieldKey::Next, "Review"));
    items.push(action_item(FieldKey::Cancel, "Cancel"));
    items
}

fn review_items() -> Vec<StepItem> {
    vec![
        action_item(FieldKey::Back, "Back"),
        action_item(FieldKey::Apply, "Apply"),
        action_item(FieldKey::Cancel, "Cancel"),
    ]
}

fn result_items() -> Vec<StepItem> {
    vec![action_item(FieldKey::Finish, "Finish")]
}

fn draw(frame: &mut ratatui::Frame<'_>, app: &WizardApp) {
    let area = frame.area();
    let sections = Layout::default()
        .direction(Direction::Vertical)
        .constraints([
            Constraint::Length(3),
            Constraint::Min(12),
            Constraint::Length(4),
        ])
        .split(area);

    let title = Paragraph::new(vec![
        Line::from(Span::styled(
            wizard_title(app.step),
            Style::default()
                .fg(Color::Cyan)
                .add_modifier(Modifier::BOLD),
        )),
        Line::from(Span::styled(
            step_status(app.step, &app.draft),
            Style::default().fg(Color::Gray),
        )),
    ])
    .block(Block::default().borders(Borders::ALL).title("Wizard"));
    frame.render_widget(title, sections[0]);

    let body = Layout::default()
        .direction(Direction::Vertical)
        .constraints([Constraint::Percentage(42), Constraint::Percentage(58)])
        .split(sections[1]);

    draw_items(frame, body[0], app);
    draw_context(frame, body[1], app);

    let footer = Paragraph::new(footer_lines(app))
        .wrap(Wrap { trim: false })
        .block(Block::default().borders(Borders::ALL).title("Help"));
    frame.render_widget(footer, sections[2]);
}

fn draw_items(frame: &mut ratatui::Frame<'_>, area: Rect, app: &WizardApp) {
    let items = app.current_items();
    let inner_height = area.height.saturating_sub(2) as usize;
    let scroll = if app.selected >= inner_height {
        app.selected + 1 - inner_height
    } else {
        0
    };

    let lines = items
        .iter()
        .enumerate()
        .skip(scroll)
        .take(inner_height)
        .map(|(index, item)| {
            let selected = index == app.selected && app.step != Step::Result;
            let marker = if selected { ">" } else { " " };
            let base_style = match item.kind {
                ItemKind::Action => Style::default().fg(Color::Yellow),
                ItemKind::Choice => Style::default().fg(Color::Green),
                ItemKind::Text => Style::default().fg(Color::White),
                ItemKind::Static => Style::default().fg(Color::DarkGray),
            };
            let style = if selected {
                base_style
                    .bg(Color::Blue)
                    .fg(Color::Black)
                    .add_modifier(Modifier::BOLD)
            } else {
                base_style
            };
            let content = format!("{marker} {:<28} {}", item.label, item.value);
            Line::from(Span::styled(content, style))
        })
        .collect::<Vec<_>>();

    let title = if app.step == Step::Result {
        "Action"
    } else {
        "Current Step"
    };
    let widget = Paragraph::new(lines)
        .wrap(Wrap { trim: false })
        .block(Block::default().borders(Borders::ALL).title(title));
    frame.render_widget(widget, area);
}

fn draw_context(frame: &mut ratatui::Frame<'_>, area: Rect, app: &WizardApp) {
    let block = Block::default()
        .borders(Borders::ALL)
        .title(if app.step == Step::Result { "Result" } else { "Context" });

    let lines = if app.step == Step::Result {
        let result = app.result.as_ref().expect("result step requires result");
        let mut lines = vec![Line::from(Span::styled(
            &result.title,
            Style::default()
                .fg(if result.success { Color::Green } else { Color::Red })
                .add_modifier(Modifier::BOLD),
        ))];
        lines.push(Line::from(""));
        lines.extend(result.lines.iter().map(|line| Line::from(line.as_str())));
        lines
    } else {
        review_lines(&app.draft, app.step, &app.status)
    };

    let widget = Paragraph::new(lines)
        .wrap(Wrap { trim: false })
        .block(block);
    frame.render_widget(widget, area);
}

fn footer_lines(app: &WizardApp) -> Vec<Line<'static>> {
    if app.step == Step::Result {
        return vec![Line::from(
            "Enter or q closes the wizard. The result screen stays open until you exit explicitly.",
        )];
    }

    let mut lines = vec![Line::from(
        "Up/Down or j/k move. Enter edits or activates. Choice fields cycle through menu options.",
    )];
    match &app.input_mode {
        InputMode::Normal => lines.push(Line::from(
            "Back and Next move between steps. Apply is only available from Review.",
        )),
        InputMode::Editing { field, .. } => lines.push(Line::from(format!(
            "Editing {}. Type, Enter to save, Esc to cancel.",
            field_label(*field)
        ))),
    }
    lines
}

fn review_lines(draft: &WizardDraft, step: Step, status: &str) -> Vec<Line<'static>> {
    let mut lines = vec![Line::from(Span::styled(
        "Planned changes",
        Style::default()
            .fg(Color::LightCyan)
            .add_modifier(Modifier::BOLD),
    ))];
    lines.push(Line::from(""));

    match step {
        Step::Welcome => {
            lines.push(Line::from(format!(
                "Shared/global setup this run: {}",
                draft.include_global.label()
            )));
            if draft.repo_available() {
                lines.push(Line::from("Next: shared config or repo-local config."));
            } else {
                lines.push(Line::from("No repo detected. Repo-local setup will be skipped."));
            }
        }
        Step::Shared | Step::Review => {
            if draft.includes_global() {
                lines.push(Line::from(format!(
                    "Shared config file: {}",
                    draft.global_config_path.display()
                )));
                lines.push(Line::from(format!(
                    "Shared env file: {}",
                    draft.shared_env_path.display()
                )));
                lines.push(Line::from(format!(
                    "Database URL: {}",
                    mask_database_url(&draft.database_url)
                )));
                lines.push(Line::from(format!(
                    "Write API token: {}",
                    secret_label(&draft.api_token)
                )));
                lines.push(Line::from(format!(
                    "LLM model: {}",
                    display_empty(&draft.llm_model)
                )));
                lines.push(Line::from(format!(
                    "LLM API key env/value: {} / {}",
                    draft.llm_api_key_env,
                    secret_label(&draft.llm_api_key_value)
                )));
            } else {
                lines.push(Line::from("Shared/global files will be left unchanged."));
            }
        }
        _ => {}
    }

    if matches!(step, Step::Repo | Step::Services | Step::Review) && draft.repo_available() {
        lines.push(Line::from(""));
        lines.push(Line::from(Span::styled(
            "Repo-local setup",
            Style::default()
                .fg(Color::LightCyan)
                .add_modifier(Modifier::BOLD),
        )));
        lines.push(Line::from(format!("Project slug: {}", draft.project)));
        lines.push(Line::from(format!(
            "Apply repo-local setup: {}",
            draft.apply_repo_setup.label()
        )));
        if draft.applies_repo_setup() {
            lines.push(Line::from(format!(
                "Local DB override: {}",
                display_override(&draft.local_database_url)
            )));
            lines.push(Line::from(format!(
                "Local LLM API key: {}",
                secret_override_label(&draft.local_llm_api_key_value)
            )));
            lines.push(Line::from(format!(
                "Automation enabled/mode: {} / {}",
                draft.automation_enabled.label(),
                automation_mode_label(&draft.automation_mode)
            )));
            lines.push(Line::from(format!(
                "Thresholds: poll={} idle={} min_changed_files={}",
                draft.automation_poll_interval,
                draft.automation_idle_threshold,
                draft.automation_min_changed_files
            )));
            lines.push(Line::from(format!(
                "Require passing test: {}",
                draft.automation_require_passing_test.label()
            )));
            lines.push(Line::from(format!(
                "Ignored paths: {}",
                draft.automation_ignored_paths
            )));
        }
    }

    if matches!(step, Step::Services | Step::Review) {
        lines.push(Line::from(""));
        lines.push(Line::from(Span::styled(
            "Selected actions",
            Style::default()
                .fg(Color::LightCyan)
                .add_modifier(Modifier::BOLD),
        )));
        if draft.repo_available() {
            lines.push(Line::from(format!(
                "Watcher user service: {}",
                draft.enable_watcher_service.label()
            )));
        }
        if draft.includes_global() && packaged_service_available() {
            lines.push(Line::from(format!(
                "Backend system service: {}",
                draft.enable_backend_service.label()
            )));
        }
        lines.push(Line::from(format!(
            "Initial scan: {}",
            draft.scan_choice.label()
        )));
        lines.push(Line::from(format!(
            "Run doctor after setup: {}",
            draft.run_doctor.label()
        )));
    }

    lines.push(Line::from(""));
    lines.push(Line::from(Span::styled(
        "Notes",
        Style::default()
            .fg(Color::LightCyan)
            .add_modifier(Modifier::BOLD),
    )));
    lines.push(Line::from(status.to_string()));
    lines
}

fn next_step(current: Step, draft: &WizardDraft) -> Step {
    match current {
        Step::Welcome => {
            if draft.includes_global() {
                Step::Shared
            } else if draft.repo_available() {
                Step::Repo
            } else {
                Step::Services
            }
        }
        Step::Shared => {
            if draft.repo_available() {
                Step::Repo
            } else {
                Step::Services
            }
        }
        Step::Repo => Step::Services,
        Step::Services => Step::Review,
        Step::Review => Step::Result,
        Step::Result => Step::Result,
    }
}

fn previous_step(current: Step, draft: &WizardDraft) -> Step {
    match current {
        Step::Welcome => Step::Welcome,
        Step::Shared => Step::Welcome,
        Step::Repo => {
            if draft.includes_global() {
                Step::Shared
            } else {
                Step::Welcome
            }
        }
        Step::Services => {
            if draft.repo_available() {
                Step::Repo
            } else if draft.includes_global() {
                Step::Shared
            } else {
                Step::Welcome
            }
        }
        Step::Review => Step::Services,
        Step::Result => Step::Review,
    }
}

fn step_status(step: Step, draft: &WizardDraft) -> &'static str {
    match step {
        Step::Welcome => "Step 1. Choose what this run should configure.",
        Step::Shared => "Step 2. Configure the shared database and LLM settings.",
        Step::Repo => "Step 3. Configure repo-local project and automation settings.",
        Step::Services => {
            if draft.repo_available() {
                "Step 4. Choose optional actions and services."
            } else {
                "Step 3. Choose optional actions and services."
            }
        }
        Step::Review => {
            if draft.repo_available() {
                "Step 5. Review everything before writing changes."
            } else {
                "Step 4. Review everything before writing changes."
            }
        }
        Step::Result => "Setup finished. Review the result before exiting.",
    }
}

fn wizard_title(step: Step) -> &'static str {
    match step {
        Step::Welcome => "Memory Layer Wizard: Scope",
        Step::Shared => "Memory Layer Wizard: Shared Config",
        Step::Repo => "Memory Layer Wizard: Repo Config",
        Step::Services => "Memory Layer Wizard: Services",
        Step::Review => "Memory Layer Wizard: Review",
        Step::Result => "Memory Layer Wizard: Result",
    }
}

fn next_step_label(step: Step) -> &'static str {
    match step {
        Step::Shared => "Shared config",
        Step::Repo => "Repo config",
        Step::Services => "Services",
        Step::Review => "Review",
        Step::Result | Step::Welcome => "Next",
    }
}

async fn apply_draft(draft: &WizardDraft) -> Result<WizardResult> {
    let mut lines = Vec::new();

    if draft.includes_global() {
        write_global_config(draft)?;
        lines.push(format!(
            "Updated shared config at {}",
            draft.global_config_path.display()
        ));
        if !draft.llm_api_key_value.trim().is_empty() {
            write_shared_env_file(
                &draft.shared_env_path,
                &draft.llm_api_key_env,
                &draft.llm_api_key_value,
            )?;
            lines.push(format!(
                "Updated shared env file at {}",
                draft.shared_env_path.display()
            ));
        }
    } else {
        lines.push("Left shared/global files unchanged.".to_string());
    }

    if let Some(repo_root) = &draft.repo_root {
        if draft.applies_repo_setup() {
            apply_repo_setup(repo_root, draft)?;
            lines.push(format!(
                "Updated repo-local Memory Layer config for project `{}` at {}.",
                draft.project,
                repo_root.display()
            ));
            write_optional_env_file(
                &repo_root.join(".mem").join("memory-layer.env"),
                &draft.llm_api_key_env,
                &draft.local_llm_api_key_value,
            )?;
            if draft.local_llm_api_key_value.trim().is_empty() {
                lines.push("Cleared repo-local LLM API key override.".to_string());
            } else {
                lines.push(format!(
                    "Updated repo-local env override at {}",
                    repo_root.join(".mem").join("memory-layer.env").display()
                ));
            }
        }
        if draft.enable_watcher_service.is_yes() {
            lines.extend(split_lines(enable_watch_service(repo_root, &draft.project)?));
        }
    }

    if draft.includes_global()
        && draft.enable_backend_service.is_yes()
        && packaged_service_available()
    {
        run_systemctl_system(["daemon-reload"])?;
        run_systemctl_system(["enable", "--now", "memory-layer.service"])?;
        lines.push("Enabled memory-layer.service".to_string());
    }

    if !matches!(draft.scan_choice, ScanChoice::Skip) {
        let config = AppConfig::load_from_path(None).context("reload config after wizard")?;
        let client = Client::builder()
            .timeout(config.service.request_timeout)
            .build()
            .context("build http client")?;
        let api = ApiClient::new(client, config);
        let repo_root = draft
            .repo_root
            .as_deref()
            .ok_or_else(|| anyhow::anyhow!("scan requested without a repository"))?;
        let report = scan::run_scan(
            &api,
            repo_root,
            &draft.project,
            None,
            matches!(draft.scan_choice, ScanChoice::DryRun),
        )
        .await?;
        lines.extend(format_scan_report(&report));
    }

    if draft.run_doctor.is_yes() {
        if let Some(repo_root) = &draft.repo_root {
            let report = run_doctor(None, repo_root, &draft.project, false).await?;
            lines.extend(format_doctor_report(&report));
        }
    }

    Ok(WizardResult {
        title: "Wizard applied".to_string(),
        lines,
        success: true,
    })
}

fn apply_repo_setup(repo_root: &Path, draft: &WizardDraft) -> Result<()> {
    repair_repo_bootstrap(repo_root, &draft.project)?;
    fs::write(
        repo_root.join(".mem").join("config.toml"),
        render_local_repo_config(repo_root, draft),
    )
    .with_context(|| format!("write {}", repo_root.join(".mem/config.toml").display()))?;
    fs::write(
        repo_root.join(".mem").join("project.toml"),
        render_project_metadata(&draft.project, repo_root),
    )
    .with_context(|| format!("write {}", repo_root.join(".mem/project.toml").display()))
}

fn write_global_config(draft: &WizardDraft) -> Result<()> {
    let parent = draft
        .global_config_path
        .parent()
        .ok_or_else(|| anyhow::anyhow!("global config path has no parent"))?;
    fs::create_dir_all(parent).with_context(|| format!("create {}", parent.display()))?;
    fs::write(&draft.global_config_path, render_global_config(draft))
        .with_context(|| format!("write {}", draft.global_config_path.display()))
}

fn render_global_config(draft: &WizardDraft) -> String {
    format!(
        "# Shared Memory Layer defaults and secrets.\n# Repo-local overrides should live in .mem/config.toml inside each project.\n\n[service]\nbind_addr = \"127.0.0.1:4040\"\ncapnp_unix_socket = \"/tmp/memory-layer.capnp.sock\"\ncapnp_tcp_addr = \"127.0.0.1:4041\"\napi_token = \"{}\"\nrequest_timeout = \"30s\"\n\n[database]\nurl = \"{}\"\n\n[features]\nllm_curation = false\n\n[llm]\nprovider = \"{}\"\nbase_url = \"{}\"\napi_key_env = \"{}\"\nmodel = \"{}\"\ntemperature = 0.0\nmax_input_bytes = 120000\nmax_output_tokens = 3000\n\n[automation]\nenabled = false\nmode = \"suggest\"\npoll_interval = \"10s\"\nidle_threshold = \"5m\"\nmin_changed_files = 2\nrequire_passing_test = false\nignored_paths = [\".git/\", \"target/\", \".memory-layer/\"]\n# repo_root = \"/path/to/repo\"\n# audit_log_path = \"/path/to/repo/.memory-layer/automation.log\"\n# state_file_path = \"/path/to/repo/.memory-layer/automation-state.json\"\n",
        draft.api_token,
        draft.database_url,
        draft.llm_provider,
        draft.llm_base_url,
        draft.llm_api_key_env,
        draft.llm_model,
    )
}

fn render_local_repo_config(repo_root: &Path, draft: &WizardDraft) -> String {
    let ignored_paths = draft
        .automation_ignored_paths
        .split(',')
        .map(|value| value.trim())
        .filter(|value| !value.is_empty())
        .map(|value| format!("\"{value}\""))
        .collect::<Vec<_>>()
        .join(", ");
    let mut content = format!(
        "# Repo-local overrides for this project.\n# Put shared defaults and secrets in the global config.\n\n"
    );
    if !draft.local_database_url.trim().is_empty() {
        content.push_str(&format!(
            "[database]\nurl = \"{}\"\n\n",
            draft.local_database_url.trim()
        ));
    }
    content.push_str(&format!(
        "[automation]\nenabled = {}\nmode = \"{}\"\nrepo_root = \"{}\"\npoll_interval = \"{}\"\nidle_threshold = \"{}\"\nmin_changed_files = {}\nrequire_passing_test = {}\nignored_paths = [{}]\naudit_log_path = \"{}/.mem/runtime/automation.log\"\nstate_file_path = \"{}/.mem/runtime/automation-state.json\"\n",
        draft.automation_enabled.is_yes(),
        automation_mode_label(&draft.automation_mode),
        repo_root.display(),
        draft.automation_poll_interval.trim(),
        draft.automation_idle_threshold.trim(),
        draft.automation_min_changed_files.trim(),
        draft.automation_require_passing_test.is_yes(),
        ignored_paths,
        repo_root.display(),
        repo_root.display(),
    ));
    content
}

fn write_optional_env_file(path: &Path, key: &str, value: &str) -> Result<()> {
    let parent = path
        .parent()
        .ok_or_else(|| anyhow::anyhow!("env file path has no parent"))?;
    fs::create_dir_all(parent).with_context(|| format!("create {}", parent.display()))?;
    let mut lines = if path.exists() {
        fs::read_to_string(path)
            .with_context(|| format!("read {}", path.display()))?
            .lines()
            .map(ToOwned::to_owned)
            .collect::<Vec<_>>()
    } else {
        Vec::new()
    };
    lines.retain(|line| {
        line.split_once('=')
            .map(|(existing, _)| existing.trim() != key)
            .unwrap_or(true)
    });
    if !value.trim().is_empty() {
        lines.push(format!("{key}={value}"));
    }
    if lines.is_empty() {
        if path.exists() {
            fs::remove_file(path).with_context(|| format!("remove {}", path.display()))?;
        }
        return Ok(());
    }
    let mut content = lines.join("\n");
    if !content.ends_with('\n') {
        content.push('\n');
    }
    fs::write(path, content).with_context(|| format!("write {}", path.display()))
}

fn format_scan_report(report: &ScanReport) -> Vec<String> {
    let mut lines = vec![format!(
        "Scan: {} files, {} commits, {} candidates, written={}",
        report.files_considered, report.commits_considered, report.candidate_count, report.written
    )];
    lines.push(format!("Scan report: {}", report.report_path));
    if let Some(capture_id) = &report.capture_id {
        lines.push(format!("Capture: {capture_id}"));
    }
    if let Some(run_id) = &report.curate_run_id {
        lines.push(format!("Curate run: {run_id}"));
    }
    lines
}

fn format_doctor_report(report: &DoctorReport) -> Vec<String> {
    let mut lines = vec![format!(
        "Doctor: project={} repo={}",
        report.project, report.repo_root
    )];
    for check in &report.checks {
        let icon = match check.status {
            DoctorStatus::Ok => "OK",
            DoctorStatus::Warn => "WARN",
            DoctorStatus::Fail => "FAIL",
            DoctorStatus::Skipped => "SKIP",
        };
        let mut line = format!("[{icon}] {} - {}", check.id, check.summary);
        if let Some(details) = &check.details {
            line.push_str(&format!(" | {details}"));
        }
        lines.push(line);
    }
    lines
}

fn split_lines(value: String) -> Vec<String> {
    value.lines().map(ToOwned::to_owned).collect()
}

fn text_item(key: FieldKey, label: &str, value: &str) -> StepItem {
    StepItem {
        key,
        label: label.to_string(),
        value: value.to_string(),
        kind: ItemKind::Text,
    }
}

fn choice_item(key: FieldKey, label: &str, value: &str) -> StepItem {
    StepItem {
        key,
        label: label.to_string(),
        value: value.to_string(),
        kind: ItemKind::Choice,
    }
}

fn action_item(key: FieldKey, label: &str) -> StepItem {
    StepItem {
        key,
        label: label.to_string(),
        value: String::new(),
        kind: ItemKind::Action,
    }
}

fn field_label(field: FieldKey) -> &'static str {
    match field {
        FieldKey::IncludeGlobal => "Include shared/global setup",
        FieldKey::DatabaseUrl => "Database URL",
        FieldKey::ApiToken => "Write API token",
        FieldKey::LlmModel => "LLM model",
        FieldKey::LlmApiKeyEnv => "LLM API key env var",
        FieldKey::LlmApiKeyValue => "LLM API key value",
        FieldKey::Project => "Project slug",
        FieldKey::LocalDatabaseUrl => "Local DB override",
        FieldKey::LocalLlmApiKeyValue => "Local LLM API key",
        FieldKey::ApplyRepoSetup => "Apply repo-local setup",
        FieldKey::AutomationEnabled => "Automation enabled",
        FieldKey::AutomationMode => "Automation mode",
        FieldKey::AutomationPollInterval => "Poll interval",
        FieldKey::AutomationIdleThreshold => "Idle threshold",
        FieldKey::AutomationMinChangedFiles => "Min changed files",
        FieldKey::AutomationRequirePassingTest => "Require passing test",
        FieldKey::AutomationIgnoredPaths => "Ignored paths",
        FieldKey::EnableBackendService => "Enable backend system service",
        FieldKey::EnableWatcher => "Enable watcher user service",
        FieldKey::ScanChoice => "Initial scan",
        FieldKey::RunDoctor => "Run doctor after setup",
        FieldKey::Next => "Next",
        FieldKey::Back => "Back",
        FieldKey::Apply => "Apply",
        FieldKey::Finish => "Finish",
        FieldKey::Cancel => "Cancel",
    }
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

fn read_local_database_override(repo_root: &Path) -> Option<String> {
    let config_path = repo_root.join(".mem").join("config.toml");
    let content = fs::read_to_string(config_path).ok()?;
    let mut in_database = false;
    for line in content.lines() {
        let trimmed = line.trim();
        if trimmed.starts_with('[') {
            in_database = trimmed == "[database]";
            continue;
        }
        if in_database {
            if let Some(value) = trimmed.strip_prefix("url = ") {
                return Some(value.trim_matches('"').to_string());
            }
        }
    }
    None
}

fn read_local_env_override(repo_root: &Path, key: &str) -> Option<String> {
    shared_env_lookup(&repo_root.join(".mem").join("memory-layer.env"), key)
}

fn default_include_global(repo_available: bool, prefer_global: bool) -> ToggleChoice {
    if repo_available {
        if prefer_global {
            ToggleChoice::Yes
        } else {
            ToggleChoice::No
        }
    } else {
        ToggleChoice::Yes
    }
}

fn toggle_from_bool(value: bool) -> ToggleChoice {
    if value {
        ToggleChoice::Yes
    } else {
        ToggleChoice::No
    }
}

fn duration_to_string(duration: std::time::Duration) -> String {
    let seconds = duration.as_secs();
    if seconds % 60 == 0 && seconds >= 60 {
        format!("{}m", seconds / 60)
    } else {
        format!("{seconds}s")
    }
}

fn automation_mode_label(mode: &AutomationMode) -> &'static str {
    match mode {
        AutomationMode::Suggest => "suggest",
        AutomationMode::Auto => "auto",
    }
}

fn display_empty(value: &str) -> String {
    if value.trim().is_empty() {
        "<empty>".to_string()
    } else {
        value.to_string()
    }
}

fn secret_label(value: &str) -> String {
    if value.trim().is_empty() {
        "<unset>".to_string()
    } else {
        "<configured>".to_string()
    }
}

fn display_override(value: &str) -> String {
    if value.trim().is_empty() {
        "<inherit shared/global>".to_string()
    } else if value.contains("://") {
        mask_database_url(value)
    } else {
        value.to_string()
    }
}

fn secret_override_label(value: &str) -> String {
    if value.trim().is_empty() {
        "<inherit shared/global>".to_string()
    } else {
        "<configured locally>".to_string()
    }
}

#[cfg(test)]
mod tests {
    use std::fs;
    use std::path::PathBuf;

    use mem_api::AutomationMode;
    use super::{Step, WizardDraft, default_include_global, next_step, read_project_slug};

    #[test]
    fn wizard_defaults_to_local_scope_inside_repo() {
        assert_eq!(default_include_global(true, false), super::ToggleChoice::No);
        assert_eq!(default_include_global(true, true), super::ToggleChoice::Yes);
    }

    #[test]
    fn wizard_defaults_to_global_outside_repo() {
        assert_eq!(default_include_global(false, false), super::ToggleChoice::Yes);
        assert_eq!(default_include_global(false, true), super::ToggleChoice::Yes);
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

    #[test]
    fn wizard_skips_shared_step_when_not_selected() {
        let draft = WizardDraft {
            global_config_path: PathBuf::from("/tmp/global.toml"),
            shared_env_path: PathBuf::from("/tmp/global.env"),
            repo_root: Some(PathBuf::from("/tmp/repo")),
            project: "repo".to_string(),
            include_global: super::ToggleChoice::No,
            database_url: String::new(),
            api_token: String::new(),
            llm_provider: String::new(),
            llm_base_url: String::new(),
            llm_api_key_env: String::new(),
            llm_model: String::new(),
            llm_api_key_value: String::new(),
            local_database_url: String::new(),
            local_llm_api_key_value: String::new(),
            apply_repo_setup: super::ToggleChoice::Yes,
            automation_enabled: super::ToggleChoice::No,
            automation_mode: AutomationMode::Suggest,
            automation_poll_interval: "10s".to_string(),
            automation_idle_threshold: "5m".to_string(),
            automation_min_changed_files: "2".to_string(),
            automation_require_passing_test: super::ToggleChoice::No,
            automation_ignored_paths: ".git/".to_string(),
            enable_backend_service: super::ToggleChoice::No,
            enable_watcher_service: super::ToggleChoice::No,
            scan_choice: super::ScanChoice::Skip,
            run_doctor: super::ToggleChoice::No,
        };

        assert_eq!(next_step(Step::Welcome, &draft), Step::Repo);
    }
}
