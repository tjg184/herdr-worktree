use ratatui::{
    crossterm::event::{self, Event, KeyCode, KeyEvent, KeyEventKind},
    layout::{Constraint, Direction, Layout, Rect},
    style::{Color, Modifier, Style},
    text::{Line, Span},
    widgets::{Block, Borders, Clear, List, ListItem, ListState, Paragraph},
    Frame, Terminal,
};
use ratatui::backend::CrosstermBackend;
use std::io;

use crate::config::Config;
use crate::git::normalize_branch_name;
use crate::herdr::{close_plugin_pane, get_plugin_pane_id, herdr_json, worktree_open};
use crate::wt::{wt_list, wt_switch, BranchEntry, EntryKind};
use std::env;

pub struct App {
    pub entries: Vec<BranchEntry>,
    pub filtered: Vec<usize>, // indices into entries
    pub filter: String,
    pub list_state: ListState,
    pub show_remotes: bool,
    pub config: Config,
    pub repo_root: String,
    pub loading: bool,
    pub error: Option<String>,
}

impl App {
    pub fn new(config: Config, repo_root: String) -> Self {
        let mut state = ListState::default();
        state.select(Some(0));
        Self {
            entries: Vec::new(),
            filtered: Vec::new(),
            filter: String::new(),
            list_state: state,
            show_remotes: false,
            config,
            repo_root,
            loading: true,
            error: None,
        }
    }

    pub fn refresh(&mut self) {
        self.entries.clear();
        
        // Load worktrees and local branches
        match wt_list(&self.repo_root, true, false) {
            Ok(json) => {
                use crate::wt::parse_wt_list;
                self.entries = parse_wt_list(&json);
            }
            Err(e) => {
                self.error = Some(format!("Failed to list worktrees: {}", e));
            }
        }

        // Load remotes if enabled
        if self.show_remotes {
            match wt_list(&self.repo_root, false, true) {
                Ok(json) => {
                    use crate::wt::parse_wt_list;
                    let remotes = parse_wt_list(&json);
                    self.entries.extend(remotes);
                }
                Err(e) => {
                    self.error = Some(format!("Failed to list remotes: {}", e));
                }
            }
        }

        self.apply_filter();
        self.loading = false;
    }

    pub fn apply_filter(&mut self) {
        let filter_lower = self.filter.to_lowercase();
        self.filtered = self
            .entries
            .iter()
            .enumerate()
            .filter(|(_, e)| e.branch.to_lowercase().contains(&filter_lower))
            .map(|(i, _)| i)
            .collect();
        
        // Reset selection
        if !self.filtered.is_empty() {
            self.list_state.select(Some(0));
        } else {
            self.list_state.select(None);
        }
    }

    pub fn selected_branch(&self) -> Option<&BranchEntry> {
        self.list_state
            .selected()
            .and_then(|idx| self.filtered.get(idx))
            .and_then(|&entry_idx| self.entries.get(entry_idx))
    }

    pub fn toggle_remotes(&mut self) {
        self.show_remotes = !self.show_remotes;
        self.refresh();
    }

    pub fn handle_input(&mut self, c: char) {
        self.filter.push(c);
        self.apply_filter();
    }

    pub fn backspace(&mut self) {
        self.filter.pop();
        self.apply_filter();
    }

    pub fn move_up(&mut self) {
        let count = self.filtered.len();
        if count == 0 {
            return;
        }
        let current = self.list_state.selected().unwrap_or(0);
        let new = if current == 0 { count - 1 } else { current - 1 };
        self.list_state.select(Some(new));
    }

    pub fn move_down(&mut self) {
        let count = self.filtered.len();
        if count == 0 {
            return;
        }
        let current = self.list_state.selected().unwrap_or(0);
        let new = if current >= count - 1 { 0 } else { current + 1 };
        self.list_state.select(Some(new));
    }
}

pub fn run_tui(repo_root: String, config: Config) -> io::Result<()> {
    let mut terminal = ratatui::init();
    let mut app = App::new(config, repo_root);
    app.refresh();

    let pane_json = herdr_json(["pane", "list"]).unwrap_or(serde_json::Value::Null);
    let pane_id = get_plugin_pane_id(&pane_json, "Worktree Picker");

    let result = loop {
        terminal.draw(|f| draw(f, &mut app))?;

        if let Event::Key(key) = event::read()? {
            if key.kind != KeyEventKind::Press {
                continue;
            }

            match key.code {
                KeyCode::Char(c) => app.handle_input(c),
                KeyCode::Backspace => app.backspace(),
                KeyCode::Up => app.move_up(),
                KeyCode::Down => app.move_down(),
                KeyCode::Enter => {
                    // Either select existing or create new
                    if let Some(entry) = app.selected_branch() {
                        if switch_to_branch(&app, &entry.branch, pane_id.as_deref()).is_ok() {
                            break Ok(());
                        }
                    } else if !app.filter.is_empty() {
                        // Create new branch from filter text
                        if switch_to_branch(&app, &app.filter.clone(), pane_id.as_deref()).is_ok() {
                            break Ok(());
                        }
                    }
                }
                KeyCode::Esc => {
                    if let Some(ref id) = pane_id {
                        let _ = close_plugin_pane(id);
                    }
                    break Ok(());
                }
                KeyCode::Char('r') if key.modifiers.contains(event::KeyModifiers::ALT) => {
                    app.toggle_remotes();
                }
                _ => {}
            }
        }
    };

    ratatui::restore();
    result
}

fn switch_to_branch(app: &App, branch: &str, pane_id: Option<&str>) -> Result<(), String> {
    let normalized = normalize_branch_name(branch, app.config.normalize_jira_prefix);
    
    // Check if branch exists in entries (to determine if we need --create)
    let exists = app.entries.iter().any(|e| e.branch == normalized);
    
    // Run wt switch
    match wt_switch(&app.repo_root, &normalized, !exists) {
        Ok(result) => {
            let path = result
                .get("path")
                .and_then(|v| v.as_str())
                .ok_or("Missing path in wt output")?;
            
            // Open worktree in herdr (with focus)
            match worktree_open(&app.repo_root, path, true) {
                Ok(_) => {
                    // Close the picker pane
                    if let Some(id) = pane_id {
                        let _ = close_plugin_pane(id);
                    }
                    Ok(())
                }
                Err(e) => Err(format!("Failed to open worktree: {}", e)),
            }
        }
        Err(e) => Err(format!("Failed to switch/create worktree: {}", e)),
    }
}

fn draw(f: &mut Frame, app: &mut App) {
    let chunks = Layout::default()
        .direction(Direction::Vertical)
        .constraints([Constraint::Length(3), Constraint::Min(0)])
        .split(f.area());

    // Filter input
    let filter_text = format!(
        "{}{}",
        app.filter,
        if app.loading { " (loading...)" } else { "" }
    );
    let filter_widget = Paragraph::new(filter_text).block(
        Block::default()
            .borders(Borders::ALL)
            .title(if app.show_remotes { 
                "Filter (alt+r to hide remotes)" 
            } else { 
                "Filter (alt+r to show remotes)" 
            }),
    );
    f.render_widget(filter_widget, chunks[0]);

    // List
    let items: Vec<ListItem> = app
        .filtered
        .iter()
        .filter_map(|&idx| app.entries.get(idx))
        .map(|entry| {
            let symbol = match entry.kind {
                EntryKind::WorktreeCurrent => "@",
                EntryKind::WorktreeMain => "^",
                EntryKind::WorktreeOther => "+",
                EntryKind::BranchLocal => "/",
                EntryKind::BranchRemote => "|",
            };
            let style = match entry.kind {
                EntryKind::WorktreeCurrent => Style::default().fg(Color::Yellow),
                EntryKind::WorktreeMain => Style::default().fg(Color::Green),
                _ => Style::default(),
            };
            let text = format!("{} {}  {}", symbol, entry.symbols, entry.branch);
            ListItem::new(Line::from(vec![
                Span::styled(text, style),
            ]))
        })
        .collect();

    let list = List::new(items)
        .block(Block::default().borders(Borders::ALL).title("Branches"))
        .highlight_style(Style::default().bg(Color::Blue));

    f.render_stateful_widget(list, chunks[1], &mut app.list_state);
}
