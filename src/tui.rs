use std::{
    io::{self, stdout},
    sync::mpsc::{self, Receiver},
    thread,
    time::Duration,
};

use crossterm::{
    event::{self, Event, KeyCode, KeyEventKind},
    execute,
    terminal::{disable_raw_mode, enable_raw_mode, EnterAlternateScreen, LeaveAlternateScreen},
};
use ratatui::{
    backend::CrosstermBackend,
    layout::{Alignment, Constraint, Direction, Layout},
    style::{Color, Modifier, Style},
    text::{Line, Span},
    widgets::{Block, Borders, List, ListItem, ListState, Paragraph, Wrap},
    Frame, Terminal,
};

use crate::{
    config::{Config, WorktreeBackend},
    git::{self, HeadState},
    herdr,
    wt::{self, BranchEntry, EntryKind, RemovalSafety},
};

#[derive(Debug, Clone)]
pub enum TuiResult {
    Cancelled,
    Created,
}

#[derive(Debug, Clone)]
enum AppState {
    Picking,
    NewIntent {
        selected: usize,
    },
    SelectingBase,
    Naming {
        input: String,
        base: Option<BranchEntry>,
        intent: NamingIntent,
    },
    RemoteConflict {
        remote: BranchEntry,
        input: String,
        selected: usize,
    },
    Creating,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum NamingIntent {
    NewBranch,
    OpenReference,
}

const ACCENT: Color = Color::Rgb(255, 158, 100);
const TEXT: Color = Color::Rgb(192, 202, 245);
const GREEN: Color = Color::Rgb(158, 206, 106);
const YELLOW: Color = Color::Rgb(224, 175, 104);
const BLUE: Color = Color::Rgb(122, 162, 247);
const TEAL: Color = Color::Rgb(125, 207, 255);
const SURFACE: Color = Color::Rgb(36, 40, 59);

fn entry_indicator(kind: &EntryKind) -> (&'static str, Style) {
    match kind {
        EntryKind::WorktreeCurrent => ("> here  ", Style::default().fg(ACCENT)),
        EntryKind::WorktreeMain => ("* main  ", Style::default().fg(GREEN)),
        EntryKind::WorktreeOther => ("[] tree ", Style::default().fg(TEAL)),
        EntryKind::BranchLocal => ("~ local ", Style::default().fg(TEXT)),
        EntryKind::BranchRemote => ("v remote", Style::default().fg(BLUE)),
    }
}

struct App {
    entries: Vec<BranchEntry>,
    filtered: Vec<usize>,
    filter: String,
    list_state: ListState,
    show_remotes: bool,
    config: Config,
    repo_root: String,
    head: HeadState,
    state: AppState,
    saved_filter: String,
    status: Option<String>,
    error: Option<String>,
    fetch: Option<Receiver<Result<Vec<BranchEntry>, String>>>,
    create: Option<Receiver<Result<(), String>>>,
    resume_state: Option<AppState>,
}

impl App {
    fn new(config: Config, repo_root: String, entries: Vec<BranchEntry>, head: HeadState) -> Self {
        let mut list_state = ListState::default();
        list_state.select(Some(0));
        let mut app = Self {
            entries,
            filtered: Vec::new(),
            filter: String::new(),
            list_state,
            show_remotes: false,
            config,
            repo_root,
            head,
            state: AppState::Picking,
            saved_filter: String::new(),
            status: None,
            error: None,
            fetch: None,
            create: None,
            resume_state: None,
        };
        app.apply_filter();
        app
    }

    fn apply_filter(&mut self) {
        let query = self.filter.to_lowercase();
        self.filtered = self
            .entries
            .iter()
            .enumerate()
            .filter(|(_, entry)| {
                // Hide remote branches when show_remotes is false
                if !self.show_remotes && matches!(entry.kind, EntryKind::BranchRemote) {
                    return false;
                }
                // Hide main branch worktree (redundant with other entries)
                if matches!(entry.kind, EntryKind::WorktreeMain) {
                    return false;
                }
                entry.reference().to_lowercase().contains(&query)
            })
            .map(|(index, _)| index)
            .collect();
        self.list_state.select(Some(0));
    }

    fn poll_tasks(&mut self) -> Option<TuiResult> {
        if let Some(result) = self
            .fetch
            .as_ref()
            .and_then(|receiver| receiver.try_recv().ok())
        {
            self.fetch = None;
            match result {
                Ok(entries) => {
                    self.entries = entries;
                    self.apply_filter();
                    self.status = Some("Remote branches refreshed".into());
                    self.error = None;
                }
                Err(error) => {
                    self.status = None;
                    self.error = Some(error);
                }
            }
        }
        if let Some(result) = self
            .create
            .as_ref()
            .and_then(|receiver| receiver.try_recv().ok())
        {
            self.create = None;
            match result {
                Ok(()) => return Some(TuiResult::Created),
                Err(error) => {
                    self.error = Some(error);
                    self.status = None;
                    self.restore_after_creation();
                }
            }
        }
        None
    }

    fn restore_after_creation(&mut self) {
        self.state = self.resume_state.take().unwrap_or(AppState::Picking);
    }

    fn visible_count(&self) -> usize {
        match self.state {
            AppState::Picking => self.filtered.len() + 1,
            AppState::SelectingBase => self.filtered.len(),
            AppState::NewIntent { .. } => 3,
            AppState::RemoteConflict { .. } => 2,
            AppState::Naming { .. } | AppState::Creating => 1,
        }
    }

    fn new_intent_available(&self, selected: usize) -> bool {
        new_intent_available(self.head, self.config.backend, selected)
    }

    fn show_new_intent(&mut self, selected: usize) {
        self.state = AppState::NewIntent { selected };
        self.list_state.select(Some(selected));
    }

    fn move_selection(&mut self, delta: isize) {
        let count = self.visible_count();
        if count == 0 {
            self.list_state.select(None);
            return;
        }
        let current = self.list_state.selected().unwrap_or(0) as isize;
        self.list_state
            .select(Some((current + delta).clamp(0, count as isize - 1) as usize));
    }

    fn selected_entry(&self) -> Option<BranchEntry> {
        let selected = self.list_state.selected()?;
        let offset = matches!(self.state, AppState::Picking) as usize;
        self.filtered
            .get(selected.checked_sub(offset)?)
            .and_then(|index| self.entries.get(*index))
            .cloned()
    }

    fn start_fetch(&mut self) {
        if self.fetch.is_some() {
            return;
        }
        let repo_root = self.repo_root.clone();
        let show_remotes = self.show_remotes;
        let backend = self.config.backend;
        let (sender, receiver) = mpsc::channel();
        thread::spawn(move || {
            let result = git::fetch_all(&repo_root).and_then(|_| match backend {
                WorktreeBackend::Worktrunk => {
                    wt::wt_list(&repo_root, true, show_remotes).map(|json| wt::parse_wt_list(&json))
                }
                WorktreeBackend::Native => git::native_entries(&repo_root, show_remotes),
            });
            let _ = sender.send(result);
        });
        self.fetch = Some(receiver);
        self.status = Some("Fetching all remotes...".into());
        self.error = None;
    }

    fn start_open(&mut self, entry: BranchEntry) {
        let backend = self.config.backend;
        let branch = entry.branch.clone();
        let target = match entry.kind {
            EntryKind::WorktreeCurrent | EntryKind::WorktreeMain | EntryKind::WorktreeOther => {
                entry.path.clone().map(|path| (path, false))
            }
            EntryKind::BranchLocal => Some((entry.branch.clone(), true)),
            EntryKind::BranchRemote => match remote_target(&entry, &self.entries) {
                Ok(Some(target)) => Some((target, true)),
                Ok(None) => {
                    self.state = AppState::RemoteConflict {
                        remote: entry,
                        input: String::new(),
                        selected: 0,
                    };
                    return;
                }
                Err(error) => {
                    self.error = Some(error);
                    return;
                }
            },
        };
        let Some((target, switch)) = target else {
            self.error = Some("Worktree entry is missing its path".into());
            return;
        };
        self.start_task(move |repo_root| {
            if !switch {
                return herdr::worktree_open(&repo_root, &target, true).map(|_| ());
            }
            match backend {
                WorktreeBackend::Worktrunk => {
                    let result = wt::wt_switch(&repo_root, &target, false)?;
                    let path = result
                        .get("path")
                        .and_then(|value| value.as_str())
                        .unwrap_or(&target);
                    herdr::worktree_open(&repo_root, path, true).map(|_| ())
                }
                WorktreeBackend::Native => {
                    let base =
                        matches!(entry.kind, EntryKind::BranchRemote).then_some(target.as_str());
                    herdr::worktree_create(&repo_root, &branch, base).map(|_| ())
                }
            }
        });
    }

    fn submit_name(&mut self, input: String, base: Option<BranchEntry>, intent: NamingIntent) {
        let input = crate::git::normalize_branch_name(&input, self.config.normalize_jira_prefix);
        if intent == NamingIntent::NewBranch {
            if is_reference(&input) {
                self.error = Some("Enter a new branch name, not a PR, MR, or URL".into());
                return;
            }
            if let Err(error) = git::validate_new_branch_name(&self.repo_root, &input) {
                self.error = Some(error);
                return;
            }
        } else if self.config.backend == WorktreeBackend::Native {
            self.error = Some("Pull and merge requests require Worktrunk".into());
            return;
        } else if let Err(error) = validate_reference(&input) {
            self.error = Some(error);
            return;
        }
        let backend = self.config.backend;
        if let Some(base) = base {
            let base_ref = base.reference();
            self.start_task(move |repo_root| match backend {
                WorktreeBackend::Worktrunk => {
                    let result = wt::wt_switch_with_base(&repo_root, &input, &base_ref)?;
                    let path = result
                        .get("path")
                        .and_then(|value| value.as_str())
                        .unwrap_or(&input);
                    herdr::worktree_open(&repo_root, path, true).map(|_| ())
                }
                WorktreeBackend::Native => {
                    herdr::worktree_create(&repo_root, &input, Some(&base_ref)).map(|_| ())
                }
            });
        } else {
            self.start_task(move |repo_root| match backend {
                WorktreeBackend::Worktrunk => {
                    let result = if intent == NamingIntent::NewBranch {
                        wt::wt_switch_with_base(
                            &repo_root,
                            &input,
                            &git::resolve_head(&repo_root)?,
                        )?
                    } else {
                        wt::wt_switch(&repo_root, &input, false)?
                    };
                    let path = result
                        .get("path")
                        .and_then(|value| value.as_str())
                        .unwrap_or(&input);
                    herdr::worktree_open(&repo_root, path, true).map(|_| ())
                }
                WorktreeBackend::Native => herdr::worktree_create(
                    &repo_root,
                    &input,
                    Some(&git::resolve_head(&repo_root)?),
                )
                .map(|_| ()),
            });
        }
    }

    fn start_task(
        &mut self,
        operation: impl FnOnce(String) -> Result<(), String> + Send + 'static,
    ) {
        let repo_root = self.repo_root.clone();
        let (sender, receiver) = mpsc::channel();
        thread::spawn(move || {
            let _ = sender.send(operation(repo_root));
        });
        self.create = Some(receiver);
        self.resume_state = Some(self.state.clone());
        self.state = AppState::Creating;
        self.status = Some("Creating and opening worktree...".into());
        self.error = None;
    }
}

fn new_intent_available(head: HeadState, backend: WorktreeBackend, selected: usize) -> bool {
    match selected {
        0 => head == HeadState::Branch,
        1 => head != HeadState::Unborn,
        2 => backend == WorktreeBackend::Worktrunk,
        _ => false,
    }
}

fn is_reference(input: &str) -> bool {
    input.starts_with("pr:")
        || input.starts_with("mr:")
        || input.starts_with("http://")
        || input.starts_with("https://")
}

fn validate_reference(input: &str) -> Result<(), String> {
    if let Some(number) = input
        .strip_prefix("pr:")
        .or_else(|| input.strip_prefix("mr:"))
    {
        return (!number.is_empty() && number.bytes().all(|byte| byte.is_ascii_digit()))
            .then_some(())
            .ok_or_else(|| "Enter pr:N or mr:N with a numeric ID".into());
    }
    if input
        .strip_prefix("https://")
        .or_else(|| input.strip_prefix("http://"))
        .is_some_and(|url| !url.is_empty())
    {
        return Ok(());
    }
    Err("Enter pr:N, mr:N, or a pull/merge request URL".into())
}

/// Some(None) means a conflicting local branch needs an alternate name.
fn remote_target(entry: &BranchEntry, entries: &[BranchEntry]) -> Result<Option<String>, String> {
    let remote = entry
        .remote
        .as_deref()
        .ok_or_else(|| format!("Remote branch {} has no remote name", entry.branch))?;
    let qualified = format!("{remote}/{}", entry.branch);
    match entries.iter().find(|candidate| {
        candidate.kind != EntryKind::BranchRemote && candidate.branch == entry.branch
    }) {
        None => Ok(Some(qualified)),
        Some(local) if local.upstream.as_deref() == Some(qualified.as_str()) => {
            Ok(Some(local.branch.clone()))
        }
        Some(_) => Ok(None),
    }
}

pub fn run_tui(
    repo_root: String,
    config: Config,
    entries: Vec<BranchEntry>,
    head: HeadState,
) -> io::Result<TuiResult> {
    enable_raw_mode()?;
    let mut output = stdout();
    execute!(output, EnterAlternateScreen)?;
    let terminal_backend = CrosstermBackend::new(output);
    let mut terminal = Terminal::new(terminal_backend)?;
    let mut app = App::new(config, repo_root, entries, head);
    let result = loop {
        terminal.draw(|frame| draw(frame, &mut app))?;
        if let Some(result) = app.poll_tasks() {
            break result;
        }
        if !event::poll(Duration::from_millis(50))? {
            continue;
        }
        let Event::Key(key) = event::read()? else {
            continue;
        };
        if key.kind != KeyEventKind::Press || matches!(app.state, AppState::Creating) {
            continue;
        }
        if app.config.keybindings.cancel.matches(key) {
            match app.state {
                AppState::Picking => break TuiResult::Cancelled,
                AppState::NewIntent { .. } => app.state = AppState::Picking,
                AppState::SelectingBase => {
                    app.filter = app.saved_filter.clone();
                    app.apply_filter();
                    app.show_new_intent(1);
                }
                AppState::Naming {
                    ref input,
                    ref base,
                    intent,
                } => {
                    let _ = (input, base);
                    app.show_new_intent(if intent == NamingIntent::OpenReference {
                        2
                    } else if base.is_some() {
                        1
                    } else {
                        0
                    });
                }
                AppState::RemoteConflict { .. } => app.state = AppState::Picking,
                AppState::Creating => unreachable!(),
            }
            continue;
        }
        // ctrl+r toggles remotes and fetches them
        if app.config.keybindings.refresh.matches(key)
            && matches!(app.state, AppState::Picking | AppState::SelectingBase)
        {
            app.show_remotes = !app.show_remotes;
            app.apply_filter();
            if app.show_remotes {
                app.start_fetch();
            }
            continue;
        }
        match app.state.clone() {
            AppState::Picking => match key.code {
                _ if app.config.keybindings.confirm.matches(key) => {
                    if app.list_state.selected() == Some(0) {
                        app.show_new_intent(0);
                    } else if let Some(entry) = app.selected_entry() {
                        app.start_open(entry);
                    }
                }
                KeyCode::Char('u')
                    if key
                        .modifiers
                        .contains(crossterm::event::KeyModifiers::CONTROL) =>
                {
                    app.filter.clear();
                    app.apply_filter();
                }
                KeyCode::Char(c) => {
                    app.filter.push(c);
                    app.apply_filter();
                }
                KeyCode::Backspace => {
                    app.filter.pop();
                    app.apply_filter();
                }
                KeyCode::Up => app.move_selection(-1),
                KeyCode::Down => app.move_selection(1),
                _ => {}
            },
            AppState::NewIntent { mut selected } => match key.code {
                KeyCode::Up => {
                    selected = selected.saturating_sub(1);
                    app.show_new_intent(selected);
                }
                KeyCode::Down => {
                    selected = (selected + 1).min(2);
                    app.show_new_intent(selected);
                }
                _ if app.config.keybindings.confirm.matches(key) => {
                    if !app.new_intent_available(selected) {
                        continue;
                    }
                    if selected == 0 {
                        app.state = AppState::Naming {
                            input: String::new(),
                            base: None,
                            intent: NamingIntent::NewBranch,
                        };
                    } else if selected == 1 {
                        app.saved_filter = app.filter.clone();
                        app.filter.clear();
                        app.apply_filter();
                        app.state = AppState::SelectingBase;
                    } else if selected == 2 {
                        app.state = AppState::Naming {
                            input: String::new(),
                            base: None,
                            intent: NamingIntent::OpenReference,
                        };
                    }
                }
                _ => {}
            },
            AppState::SelectingBase => match key.code {
                _ if app.config.keybindings.confirm.matches(key) => {
                    if let Some(base) = app.selected_entry() {
                        app.state = AppState::Naming {
                            input: String::new(),
                            base: Some(base),
                            intent: NamingIntent::NewBranch,
                        };
                    }
                }
                KeyCode::Char('u')
                    if key
                        .modifiers
                        .contains(crossterm::event::KeyModifiers::CONTROL) =>
                {
                    app.filter.clear();
                    app.apply_filter();
                }
                KeyCode::Char(c) => {
                    app.filter.push(c);
                    app.apply_filter();
                }
                KeyCode::Backspace => {
                    app.filter.pop();
                    app.apply_filter();
                }
                KeyCode::Up => app.move_selection(-1),
                KeyCode::Down => app.move_selection(1),
                _ => {}
            },
            AppState::Naming {
                mut input,
                base,
                intent,
            } => match key.code {
                _ if app.config.keybindings.confirm.matches(key) => {
                    app.submit_name(input, base, intent)
                }
                KeyCode::Char('u')
                    if key
                        .modifiers
                        .contains(crossterm::event::KeyModifiers::CONTROL) =>
                {
                    input.clear();
                    app.state = AppState::Naming {
                        input,
                        base,
                        intent,
                    };
                }
                KeyCode::Char(c) => {
                    input.push(c);
                    app.state = AppState::Naming {
                        input,
                        base,
                        intent,
                    };
                }
                KeyCode::Backspace => {
                    input.pop();
                    app.state = AppState::Naming {
                        input,
                        base,
                        intent,
                    };
                }
                _ => {}
            },
            AppState::RemoteConflict {
                remote,
                input,
                mut selected,
            } => match key.code {
                KeyCode::Up => {
                    selected = selected.saturating_sub(1);
                    app.state = AppState::RemoteConflict {
                        remote,
                        input,
                        selected,
                    };
                    app.list_state.select(Some(selected));
                }
                KeyCode::Down => {
                    selected = (selected + 1).min(1);
                    app.state = AppState::RemoteConflict {
                        remote,
                        input,
                        selected,
                    };
                    app.list_state.select(Some(selected));
                }
                _ if app.config.keybindings.confirm.matches(key) => {
                    if selected == 0 {
                        app.state = AppState::Naming {
                            input,
                            base: Some(remote),
                            intent: NamingIntent::NewBranch,
                        };
                    } else {
                        app.state = AppState::Picking;
                    }
                }
                _ => {}
            },
            AppState::Creating => unreachable!(),
        }
    };
    disable_raw_mode()?;
    execute!(terminal.backend_mut(), LeaveAlternateScreen)?;
    terminal.show_cursor()?;
    Ok(result)
}

fn draw(frame: &mut Frame, app: &mut App) {
    let chunks = Layout::default()
        .direction(Direction::Vertical)
        .constraints([
            Constraint::Length(3),
            Constraint::Min(0),
            Constraint::Length(1),
        ])
        .split(frame.area());
    let (title, text) = match &app.state {
        AppState::Picking => (
            format!(
                "Filter ({} remotes, {} cancel)",
                app.config.keybindings.refresh.display(),
                app.config.keybindings.cancel.display()
            ),
            app.filter.clone(),
        ),
        AppState::NewIntent { .. } => ("Create worktree".into(), "Choose a starting point".into()),
        AppState::SelectingBase => (
            format!(
                "Select base branch ({} remotes, {} cancel)",
                app.config.keybindings.refresh.display(),
                app.config.keybindings.cancel.display()
            ),
            app.filter.clone(),
        ),
        AppState::Naming {
            input,
            base,
            intent,
        } => match intent {
            NamingIntent::NewBranch => (
                format!(
                    "New branch from {}",
                    base.as_ref()
                        .map(BranchEntry::reference)
                        .unwrap_or_else(|| "current HEAD".into())
                ),
                input.clone(),
            ),
            NamingIntent::OpenReference => ("Open pull or merge request".into(), input.clone()),
        },
        AppState::RemoteConflict { remote, .. } => (
            "Remote name conflict".into(),
            format!(
                "{} already has a different local branch",
                remote.reference()
            ),
        ),
        AppState::Creating => ("Creating".into(), app.status.clone().unwrap_or_default()),
    };
    if matches!(app.state, AppState::NewIntent { .. }) {
        frame.render_widget(
            Paragraph::new(vec![
                Line::styled(
                    title,
                    Style::default().fg(ACCENT).add_modifier(Modifier::BOLD),
                ),
                Line::styled(text, Style::default().fg(TEXT)),
            ]),
            chunks[0],
        );
    } else {
        frame.render_widget(
            Paragraph::new(text).block(Block::default().borders(Borders::ALL).title(title)),
            chunks[0],
        );
    }
    let items = match &app.state {
        AppState::Picking => {
            let mut rows = vec![ListItem::new(Span::styled(
                "+ new     New worktree...",
                Style::default().fg(YELLOW).add_modifier(Modifier::BOLD),
            ))];
            rows.extend(
                app.filtered
                    .iter()
                    .filter_map(|index| app.entries.get(*index))
                    .map(render_entry),
            );
            rows
        }
        AppState::SelectingBase => app
            .filtered
            .iter()
            .filter_map(|index| app.entries.get(*index))
            .map(render_entry)
            .collect(),
        AppState::NewIntent { .. } => (0..3)
            .map(|selected| render_new_intent(selected, app.head, app.config.backend))
            .collect(),
        AppState::Naming { intent, .. } => vec![ListItem::new(match intent {
            NamingIntent::NewBranch => "Enter a new branch name",
            NamingIntent::OpenReference => {
                "GitHub: pr:123   GitLab: mr:123   Or enter a request URL"
            }
        })],
        AppState::RemoteConflict { .. } => vec![
            ListItem::new("Create with another local name"),
            ListItem::new("Back"),
        ],
        AppState::Creating => vec![ListItem::new("Working...")],
    };
    let list_title = if matches!(app.state, AppState::NewIntent { .. }) {
        "Starting point"
    } else {
        "Worktrees and branches"
    };
    let list = List::new(items)
        .block(Block::default().borders(Borders::ALL).title(list_title))
        .highlight_style(Style::default().bg(SURFACE).add_modifier(Modifier::BOLD))
        .highlight_symbol("> ");
    frame.render_stateful_widget(list, chunks[1], &mut app.list_state);
    let controls = match app.state {
        AppState::NewIntent { .. } => "↑/↓ select   ↵ continue   Esc back",
        _ => "",
    };
    let message = app
        .error
        .as_deref()
        .or(app.status.as_deref())
        .unwrap_or(controls);
    frame.render_widget(
        Paragraph::new(message).style(Style::default().fg(if app.error.is_some() {
            Color::Red
        } else {
            GREEN
        })),
        chunks[2],
    );
}

fn render_new_intent(
    selected: usize,
    head: HeadState,
    backend: WorktreeBackend,
) -> ListItem<'static> {
    let (label, description, color, unavailable) = match selected {
        0 => (
            "CURRENT HEAD",
            "Create a new branch from your current checkout",
            GREEN,
            match head {
                HeadState::Branch => None,
                HeadState::Detached => Some("Unavailable: detached HEAD"),
                HeadState::Unborn => Some("Unavailable: create an initial commit first"),
            },
        ),
        1 => (
            "ANOTHER BRANCH",
            "Choose a local or remote base, then name the branch",
            BLUE,
            if head == HeadState::Unborn {
                Some("Unavailable: create an initial commit first")
            } else {
                None
            },
        ),
        2 => (
            "PULL / MERGE REQUEST",
            "Open pr:123, mr:123, or a request URL",
            YELLOW,
            (backend == WorktreeBackend::Native).then_some("Requires Worktrunk"),
        ),
        _ => unreachable!(),
    };
    let description = unavailable.unwrap_or(description);
    let label_style = if unavailable.is_some() {
        Style::default().fg(Color::DarkGray)
    } else {
        Style::default().fg(color).add_modifier(Modifier::BOLD)
    };
    let description_style = if unavailable.is_some() {
        Style::default().fg(Color::DarkGray)
    } else {
        Style::default().fg(TEXT)
    };
    ListItem::new(vec![
        Line::from(Span::styled(label, label_style)),
        Line::from(Span::styled(description, description_style)),
    ])
}

fn render_entry(entry: &BranchEntry) -> ListItem<'static> {
    let (indicator, style) = entry_indicator(&entry.kind);
    ListItem::new(Line::from(vec![
        Span::styled(indicator, style),
        Span::raw(format!("  {}  ", entry.symbols)),
        Span::styled(entry.reference(), Style::default().fg(TEXT)),
    ]))
}

pub fn run_confirm_tui(
    statusline: &str,
    safety: &RemovalSafety,
    backend: WorktreeBackend,
) -> io::Result<bool> {
    enable_raw_mode()?;
    let mut output = stdout();
    execute!(output, EnterAlternateScreen)?;
    let terminal_backend = CrosstermBackend::new(output);
    let mut terminal = Terminal::new(terminal_backend)?;
    let result = loop {
        terminal.draw(|frame| draw_confirm_ui(frame, statusline, safety, backend))?;
        if let Event::Key(key) = event::read()? {
            if key.kind == KeyEventKind::Press {
                match key.code {
                    KeyCode::Char('y') | KeyCode::Char('Y') if safety.allows_removal() => {
                        break true
                    }
                    KeyCode::Char('n') | KeyCode::Char('N') | KeyCode::Esc => break false,
                    _ => {}
                }
            }
        }
    };
    execute!(terminal.backend_mut(), LeaveAlternateScreen)?;
    disable_raw_mode()?;
    Ok(result)
}

fn draw_confirm_ui(
    frame: &mut Frame,
    statusline: &str,
    safety: &RemovalSafety,
    backend: WorktreeBackend,
) {
    let layout = Layout::default()
        .direction(Direction::Vertical)
        .constraints([
            Constraint::Fill(1),
            Constraint::Length(1),
            Constraint::Length(1),
            Constraint::Length(1),
            Constraint::Fill(1),
        ])
        .split(frame.area());
    frame.render_widget(
        Paragraph::new(statusline)
            .alignment(Alignment::Center)
            .wrap(Wrap { trim: true }),
        layout[1],
    );
    let (message, color) = match safety {
        RemovalSafety::Safe => match backend {
            WorktreeBackend::Worktrunk => (
                "Safe to remove: worktree and branch will be deleted.",
                Color::Green,
            ),
            WorktreeBackend::Native => (
                "Safe to remove: worktree will be removed; branch will be kept.",
                Color::Green,
            ),
        },
        RemovalSafety::Dirty => (
            "Cannot remove: worktree has uncommitted changes.",
            Color::Red,
        ),
        RemovalSafety::BranchCheckedOutElsewhere => (
            "Safe to remove: branch is checked out elsewhere.",
            Color::Yellow,
        ),
        RemovalSafety::BranchNotIntegrated => (
            "Safe to remove: branch changes are not integrated.",
            Color::Yellow,
        ),
        RemovalSafety::Unknown => ("Cannot verify safe removal.", Color::Yellow),
    };
    frame.render_widget(
        Paragraph::new(message)
            .style(Style::default().fg(color))
            .alignment(Alignment::Center),
        layout[2],
    );
    frame.render_widget(
        Paragraph::new(if safety.allows_removal() {
            "[y] Remove    [n/Esc] Cancel"
        } else {
            "[Esc] Cancel"
        })
        .alignment(Alignment::Center),
        layout[3],
    );
}

#[cfg(test)]
mod tests {
    use super::*;

    fn branch(name: &str) -> BranchEntry {
        BranchEntry {
            kind: EntryKind::BranchLocal,
            branch: name.into(),
            path: None,
            symbols: String::new(),
            remote: None,
            upstream: None,
        }
    }
    #[test]
    fn remote_collision_requires_rename() {
        let remote = BranchEntry {
            kind: EntryKind::BranchRemote,
            branch: "feature/auth".into(),
            path: None,
            symbols: String::new(),
            remote: Some("origin".into()),
            upstream: None,
        };
        let local = BranchEntry {
            kind: EntryKind::BranchLocal,
            branch: "feature/auth".into(),
            path: None,
            symbols: String::new(),
            remote: None,
            upstream: Some("upstream/feature/auth".into()),
        };
        assert_eq!(
            remote_target(&remote, &[remote.clone(), local]).unwrap(),
            None
        );
    }

    #[test]
    fn new_intent_availability_matches_head_state() {
        assert!(new_intent_available(
            HeadState::Branch,
            WorktreeBackend::Worktrunk,
            0
        ));
        assert!(new_intent_available(
            HeadState::Branch,
            WorktreeBackend::Worktrunk,
            1
        ));
        assert!(!new_intent_available(
            HeadState::Detached,
            WorktreeBackend::Worktrunk,
            0
        ));
        assert!(new_intent_available(
            HeadState::Detached,
            WorktreeBackend::Worktrunk,
            1
        ));
        assert!(!new_intent_available(
            HeadState::Unborn,
            WorktreeBackend::Worktrunk,
            0
        ));
        assert!(!new_intent_available(
            HeadState::Unborn,
            WorktreeBackend::Worktrunk,
            1
        ));
        assert!(new_intent_available(
            HeadState::Unborn,
            WorktreeBackend::Worktrunk,
            2
        ));
        assert!(!new_intent_available(
            HeadState::Branch,
            WorktreeBackend::Native,
            2
        ));
    }

    #[test]
    fn restoring_new_intent_keeps_state_and_highlight_in_sync() {
        let mut app = App::new(Config::default(), "/repo".into(), Vec::new(), HeadState::Branch);

        app.show_new_intent(1);

        assert!(matches!(app.state, AppState::NewIntent { selected: 1 }));
        assert_eq!(app.list_state.selected(), Some(1));
    }
    #[test]
    fn references_are_not_branch_names() {
        assert!(is_reference("pr:42"));
        assert!(!is_reference("feature/new"));
    }

    #[test]
    fn failed_creation_restores_name_and_base() {
        let base = branch("main");
        let mut app = App::new(Config::default(), ".".into(), vec![base.clone()], HeadState::Branch);
        let resume = AppState::Naming {
            input: "feature/new".into(),
            base: Some(base),
            intent: NamingIntent::NewBranch,
        };
        let (sender, receiver) = mpsc::channel();
        sender.send(Err("creation failed".into())).unwrap();
        app.state = AppState::Creating;
        app.resume_state = Some(resume);
        app.create = Some(receiver);

        app.poll_tasks();

        assert!(matches!(app.state, AppState::Naming { input, .. } if input == "feature/new"));
        assert_eq!(app.error.as_deref(), Some("creation failed"));
    }

    #[test]
    fn reference_validation_accepts_only_supported_inputs() {
        assert!(validate_reference("pr:42").is_ok());
        assert!(validate_reference("mr:42").is_ok());
        assert!(validate_reference("https://github.com/org/repo/pull/42").is_ok());
        assert!(validate_reference("pr:abc").is_err());
        assert!(validate_reference("feature/new").is_err());
    }
}
