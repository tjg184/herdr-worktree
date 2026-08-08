use crossterm::{
    event::{self, Event, KeyCode, KeyEventKind},
    execute,
    terminal::{disable_raw_mode, enable_raw_mode, EnterAlternateScreen, LeaveAlternateScreen},
};
use ratatui::{
    backend::CrosstermBackend,
    layout::{Alignment, Constraint, Direction, Layout, Rect},
    style::{Color, Modifier, Style},
    text::{Line, Span},
    widgets::{Block, Borders, Clear, List, ListItem, ListState, Paragraph, Wrap},
    Frame, Terminal,
};
use std::io::{self, stdout};

use crate::config::Config;
use crate::wt::{BranchEntry, EntryKind};

#[derive(Debug, Clone)]
pub enum TuiResult {
    Selected(BranchEntry),
    Cancelled,
    ToggleRemotes,
}

#[derive(Debug, Clone)]
pub enum AppState {
    Picking,
    CreatingNew { input: String },
}

pub struct App {
    pub entries: Vec<BranchEntry>,
    pub filtered: Vec<usize>, // indices into entries (does not include pinned entry)
    pub filter: String,
    pub list_state: ListState,
    pub show_remotes: bool,
    pub config: Config,
    pub repo_root: String,
    pub loading: bool,
    pub error: Option<String>,
    pub state: AppState,
    // Saved state for returning from CreatingNew
    pub saved_filter: String,
}

impl App {
    pub fn new(config: Config, repo_root: String, entries: Vec<BranchEntry>, show_remotes: bool) -> Self {
        let mut state = ListState::default();
        state.select(Some(0));
        Self {
            entries,
            filtered: Vec::new(),
            filter: String::new(),
            list_state: state,
            show_remotes,
            config,
            repo_root,
            loading: false,
            error: None,
            state: AppState::Picking,
            saved_filter: String::new(),
        }
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
        if matches!(self.state, AppState::CreatingNew { .. }) {
            return None;
        }
        // Index 0 is the pinned "+ New worktree..." entry
        let selected = self.list_state.selected()?;
        if selected == 0 {
            return None; // Pinned entry is not in entries vec
        }
        // Adjust for pinned entry
        let entry_idx = self.filtered.get(selected - 1)?;
        self.entries.get(*entry_idx)
    }

    pub fn move_up(&mut self) {
        let count = self.visible_count();
        if count == 0 {
            return;
        }
        let current = self.list_state.selected().unwrap_or(0);
        let new = if current == 0 { count - 1 } else { current - 1 };
        self.list_state.select(Some(new));
    }

    pub fn move_down(&mut self) {
        let count = self.visible_count();
        if count == 0 {
            return;
        }
        let current = self.list_state.selected().unwrap_or(0);
        let new = if current >= count - 1 { 0 } else { current + 1 };
        self.list_state.select(Some(new));
    }

    fn visible_count(&self) -> usize {
        match self.state {
            AppState::Picking => 1 + self.filtered.len(), // +1 for pinned entry
            AppState::CreatingNew { .. } => 1, // Single "↵ to create" row
        }
    }

    pub fn handle_input(&mut self, c: char) {
        match &mut self.state {
            AppState::Picking => {
                self.filter.push(c);
                self.apply_filter();
            }
            AppState::CreatingNew { input } => {
                input.push(c);
            }
        }
    }

    pub fn backspace(&mut self) {
        match &mut self.state {
            AppState::Picking => {
                self.filter.pop();
                self.apply_filter();
            }
            AppState::CreatingNew { input } => {
                input.pop();
            }
        }
    }

    pub fn start_creating_new(&mut self) {
        self.saved_filter = self.filter.clone();
        self.state = AppState::CreatingNew { input: String::new() };
    }

    pub fn cancel_creating_new(&mut self) {
        self.filter = self.saved_filter.clone();
        self.apply_filter();
        self.state = AppState::Picking;
    }
}

pub fn run_tui(repo_root: String, config: Config, entries: Vec<BranchEntry>, show_remotes: bool) -> io::Result<TuiResult> {
    // Phase 1: TUI setup (after all subprocess calls are done)
    enable_raw_mode()?;
    let mut stdout = stdout();
    execute!(stdout, EnterAlternateScreen)?;
    let backend = CrosstermBackend::new(stdout);
    let mut terminal = Terminal::new(backend)?;

    let mut app = App::new(config, repo_root, entries, show_remotes);
    app.apply_filter();

    // Phase 2: TUI loop - NO subprocess calls here
    let result = loop {
        terminal.draw(|f| draw(f, &mut app))?;

        if let Event::Key(key) = event::read()? {
            if key.kind != KeyEventKind::Press {
                continue;
            }

            // Global keys
            match key.code {
                KeyCode::Esc => {
                    match &app.state {
                        AppState::Picking => {
                            disable_raw_mode()?;
                            execute!(terminal.backend_mut(), LeaveAlternateScreen)?;
                            terminal.show_cursor()?;
                            return Ok(TuiResult::Cancelled);
                        }
                        AppState::CreatingNew { .. } => {
                            app.cancel_creating_new();
                            continue;
                        }
                    }
                }
                KeyCode::Char('r') if key.modifiers.contains(event::KeyModifiers::ALT) => {
                    if matches!(app.state, AppState::Picking) {
                        disable_raw_mode()?;
                        execute!(terminal.backend_mut(), LeaveAlternateScreen)?;
                        terminal.show_cursor()?;
                        return Ok(TuiResult::ToggleRemotes);
                    }
                }
                _ => {}
            }

            // State-specific keys
            match &app.state {
                AppState::Picking => {
                    match key.code {
                        KeyCode::Char(c) => app.handle_input(c),
                        KeyCode::Backspace => app.backspace(),
                        KeyCode::Up => app.move_up(),
                        KeyCode::Down => app.move_down(),
                        KeyCode::Enter => {
                            let selected = app.list_state.selected().unwrap_or(0);
                            if selected == 0 {
                                // Pinned "+ New worktree..." entry
                                app.start_creating_new();
                            } else {
                                // Real entry
                                if let Some(entry) = app.selected_branch() {
                                    disable_raw_mode()?;
                                    execute!(terminal.backend_mut(), LeaveAlternateScreen)?;
                                    terminal.show_cursor()?;
                                    return Ok(TuiResult::Selected(entry.clone()));
                                }
                            }
                        }
                        _ => {}
                    }
                }
                AppState::CreatingNew { input } => {
                    match key.code {
                        KeyCode::Char(c) => app.handle_input(c),
                        KeyCode::Backspace => app.backspace(),
                        KeyCode::Enter => {
                            if !input.is_empty() {
                                let entry = BranchEntry {
                                    kind: EntryKind::NewWorktree,
                                    branch: input.clone(),
                                    path: None,
                                    symbols: String::new(),
                                };
                                disable_raw_mode()?;
                                execute!(terminal.backend_mut(), LeaveAlternateScreen)?;
                                terminal.show_cursor()?;
                                return Ok(TuiResult::Selected(entry));
                            }
                        }
                        _ => {}
                    }
                }
            }
        }
    };
}

fn draw(f: &mut Frame, app: &mut App) {
    let chunks = Layout::default()
        .direction(Direction::Vertical)
        .constraints([Constraint::Length(3), Constraint::Min(0)])
        .split(f.area());

    // Filter/input block
    let (title, text) = match &app.state {
        AppState::Picking => {
            let title = if app.show_remotes {
                "Filter (alt+r to hide remotes, Esc to cancel)"
            } else {
                "Filter (alt+r to show remotes, Esc to cancel)"
            };
            let text = format!(
                "{}{}",
                app.filter,
                if app.loading { " (loading...)" } else { "" }
            );
            (title, text)
        }
        AppState::CreatingNew { input } => {
            let title = "Branch, pr:N, or URL — Esc to go back";
            (title, input.clone())
        }
    };
    
    let filter_widget = Paragraph::new(text).block(
        Block::default()
            .borders(Borders::ALL)
            .title(title),
    );
    f.render_widget(filter_widget, chunks[0]);

    // List
    let items: Vec<ListItem> = match &app.state {
        AppState::Picking => {
            // Pinned "+ New worktree..." entry
            let mut items = vec![
                ListItem::new(Line::from(vec![
                    Span::styled("+ New worktree...", Style::default().add_modifier(Modifier::BOLD).fg(Color::Cyan)),
                ]))
            ];
            
            // Real entries
            let entry_items: Vec<ListItem> = app
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
                        EntryKind::NewWorktree => "?",
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
            items.extend(entry_items);
            items
        }
        AppState::CreatingNew { .. } => {
            vec![ListItem::new(Line::from(vec![
                Span::styled("↵ to create worktree", Style::default().fg(Color::DarkGray)),
            ]))]
        }
    };

    let list = List::new(items)
        .block(Block::default().borders(Borders::ALL).title("Branches"))
        .highlight_style(Style::default().bg(Color::Blue));

    f.render_stateful_widget(list, chunks[1], &mut app.list_state);
}

/// Run confirm remove TUI - returns true if confirmed, false if cancelled
pub fn run_confirm_tui(statusline: &str) -> io::Result<bool> {
    enable_raw_mode()?;
    let mut stdout = stdout();
    execute!(stdout, EnterAlternateScreen)?;
    let backend = CrosstermBackend::new(stdout);
    let mut terminal = Terminal::new(backend)?;

    let result = run_confirm_dialog(&mut terminal, statusline);

    execute!(terminal.backend_mut(), LeaveAlternateScreen)?;
    disable_raw_mode()?;

    result
}

fn run_confirm_dialog(terminal: &mut Terminal<CrosstermBackend<io::Stdout>>, statusline: &str) -> io::Result<bool> {
    let mut confirmed = false;

    loop {
        terminal.draw(|f| draw_confirm_ui(f, statusline))?;

        if let Event::Key(key) = event::read()? {
            if key.kind == KeyEventKind::Press {
                match key.code {
                    KeyCode::Char('y') | KeyCode::Char('Y') => {
                        confirmed = true;
                        break;
                    }
                    KeyCode::Char('n') | KeyCode::Char('N') | KeyCode::Esc => {
                        break;
                    }
                    _ => {}
                }
            }
        }
    }

    Ok(confirmed)
}

fn draw_confirm_ui(frame: &mut Frame, statusline: &str) {
    let size = frame.area();

    // Full-screen block with border
    let block = Block::default()
        .title(Line::from(vec![
            Span::styled("Remove worktree?", Style::default().add_modifier(Modifier::BOLD)),
        ]))
        .borders(Borders::ALL)
        .border_style(Style::default().fg(Color::Yellow));

    frame.render_widget(&block, size);

    let inner = block.inner(size);

    // Vertically center content with flexible spacing
    let layout = Layout::default()
        .direction(Direction::Vertical)
        .constraints([
            Constraint::Fill(1),      // flexible space above
            Constraint::Length(1),    // statusline
            Constraint::Length(1),    // spacer
            Constraint::Length(1),    // buttons
            Constraint::Fill(1),      // flexible space below
        ])
        .split(inner);

    // Statusline (worktree info)
    let status_para = Paragraph::new(statusline)
        .alignment(Alignment::Center)
        .wrap(Wrap { trim: true });
    frame.render_widget(status_para, layout[1]);

    // Buttons
    let buttons = Paragraph::new(Line::from(vec![
        Span::styled("[y]", Style::default().fg(Color::Green).add_modifier(Modifier::BOLD)),
        Span::raw(" Remove    "),
        Span::styled("[n/Esc]", Style::default().fg(Color::Red).add_modifier(Modifier::BOLD)),
        Span::raw(" Cancel"),
    ]))
    .alignment(Alignment::Center);
    frame.render_widget(buttons, layout[3]);
}
