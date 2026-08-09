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
use crate::wt::{BranchEntry, EntryKind, RemovalSafety};

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

const TOKYO_NIGHT_ACCENT: Color = Color::Rgb(255, 158, 100);
const TOKYO_NIGHT_TEXT: Color = Color::Rgb(192, 202, 245);
const TOKYO_NIGHT_GREEN: Color = Color::Rgb(158, 206, 106);
const TOKYO_NIGHT_YELLOW: Color = Color::Rgb(224, 175, 104);
const TOKYO_NIGHT_BLUE: Color = Color::Rgb(122, 162, 247);
const TOKYO_NIGHT_TEAL: Color = Color::Rgb(125, 207, 255);
const TOKYO_NIGHT_SURFACE: Color = Color::Rgb(36, 40, 59);

fn entry_indicator(kind: &EntryKind) -> (&'static str, Style) {
    match kind {
        EntryKind::WorktreeCurrent => ("▶ here  ", Style::default().fg(TOKYO_NIGHT_ACCENT)),
        EntryKind::WorktreeMain => ("★ main  ", Style::default().fg(TOKYO_NIGHT_GREEN)),
        EntryKind::WorktreeOther => ("□ tree  ", Style::default().fg(TOKYO_NIGHT_TEAL)),
        EntryKind::BranchLocal => ("⑂ local ", Style::default().fg(TOKYO_NIGHT_TEXT)),
        EntryKind::BranchRemote => ("⇣ remote", Style::default().fg(TOKYO_NIGHT_BLUE)),
        EntryKind::NewWorktree => ("+ new   ", Style::default().fg(TOKYO_NIGHT_YELLOW).add_modifier(Modifier::BOLD)),
    }
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
            if app.config.keybindings.cancel.matches(key) {
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
            if app.config.keybindings.toggle_remotes.matches(key)
                && matches!(app.state, AppState::Picking)
            {
                disable_raw_mode()?;
                execute!(terminal.backend_mut(), LeaveAlternateScreen)?;
                terminal.show_cursor()?;
                return Ok(TuiResult::ToggleRemotes);
            }

            // State-specific keys
            match &app.state {
                AppState::Picking => {
                    match key.code {
                        _ if app.config.keybindings.confirm.matches(key) => {
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
                        KeyCode::Char(c) => app.handle_input(c),
                        KeyCode::Backspace => app.backspace(),
                        KeyCode::Up => app.move_up(),
                        KeyCode::Down => app.move_down(),
                        _ => {}
                    }
                }
                AppState::CreatingNew { input } => {
                    match key.code {
                        _ if app.config.keybindings.confirm.matches(key) => {
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
                        KeyCode::Char(c) => app.handle_input(c),
                        KeyCode::Backspace => app.backspace(),
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
                format!(
                    "Filter ({} to hide remotes, {} to cancel)",
                    app.config.keybindings.toggle_remotes.display(),
                    app.config.keybindings.cancel.display()
                )
            } else {
                format!(
                    "Filter ({} to show remotes, {} to cancel)",
                    app.config.keybindings.toggle_remotes.display(),
                    app.config.keybindings.cancel.display()
                )
            };
            let text = format!(
                "{}{}",
                app.filter,
                if app.loading { " (loading...)" } else { "" }
            );
            (title, text)
        }
        AppState::CreatingNew { input } => {
            let title = format!(
                "Branch, pr:N, or URL - {} to go back",
                app.config.keybindings.cancel.display()
            );
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
            let (new_indicator, new_style) = entry_indicator(&EntryKind::NewWorktree);
            let mut items = vec![
                ListItem::new(Line::from(vec![
                    Span::styled(format!("{}  New worktree...", new_indicator), new_style),
                ]))
            ];
            
            // Real entries
            let entry_items: Vec<ListItem> = app
                .filtered
                .iter()
                .filter_map(|&idx| app.entries.get(idx))
                .map(|entry| {
                    let (indicator, style) = entry_indicator(&entry.kind);
                    ListItem::new(Line::from(vec![
                        Span::styled(indicator, style),
                        Span::raw(format!("  {}  ", entry.symbols)),
                        Span::styled(&entry.branch, Style::default().fg(TOKYO_NIGHT_TEXT)),
                    ]))
                })
                .collect();
            items.extend(entry_items);
            items
        }
        AppState::CreatingNew { .. } => {
            vec![ListItem::new(Line::from(vec![
                Span::styled(
                    format!("{} to create worktree", app.config.keybindings.confirm.display()),
                    Style::default().fg(Color::DarkGray),
                ),
            ]))]
        }
    };

    let list = List::new(items)
        .block(Block::default().borders(Borders::ALL).title("Branches"))
        .highlight_style(Style::default().bg(TOKYO_NIGHT_SURFACE).add_modifier(Modifier::BOLD));

    f.render_stateful_widget(list, chunks[1], &mut app.list_state);
}

/// Run confirm remove TUI - returns true if confirmed, false if cancelled
pub fn run_confirm_tui(statusline: &str, safety: &RemovalSafety) -> io::Result<bool> {
    enable_raw_mode()?;
    let mut stdout = stdout();
    execute!(stdout, EnterAlternateScreen)?;
    let backend = CrosstermBackend::new(stdout);
    let mut terminal = Terminal::new(backend)?;

    let result = run_confirm_dialog(&mut terminal, statusline, safety);

    execute!(terminal.backend_mut(), LeaveAlternateScreen)?;
    disable_raw_mode()?;

    result
}

fn run_confirm_dialog(
    terminal: &mut Terminal<CrosstermBackend<io::Stdout>>,
    statusline: &str,
    safety: &RemovalSafety,
) -> io::Result<bool> {
    let mut confirmed = false;

    loop {
        terminal.draw(|f| draw_confirm_ui(f, statusline, safety))?;

        if let Event::Key(key) = event::read()? {
            if key.kind == KeyEventKind::Press {
                match key.code {
                    KeyCode::Char('y') | KeyCode::Char('Y') if safety.allows_removal() => {
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

fn draw_confirm_ui(frame: &mut Frame, statusline: &str, safety: &RemovalSafety) {
    let size = frame.area();

    // Vertically center content with flexible spacing
    let layout = Layout::default()
        .direction(Direction::Vertical)
        .constraints([
            Constraint::Fill(1),      // flexible space above
            Constraint::Length(1),    // statusline
            Constraint::Length(1),    // safety status
            Constraint::Length(1),    // spacer
            Constraint::Length(1),    // buttons
            Constraint::Fill(1),      // flexible space below
        ])
        .split(size);

    // Statusline (worktree info)
    let status_para = Paragraph::new(statusline)
        .alignment(Alignment::Center)
        .wrap(Wrap { trim: true });
    frame.render_widget(status_para, layout[1]);

    let (message, color) = match safety {
        RemovalSafety::Safe => (
            "Safe to remove: worktree and branch will be deleted.",
            Color::Green,
        ),
        RemovalSafety::Dirty => (
            "Cannot remove: worktree has uncommitted changes.",
            Color::Red,
        ),
        RemovalSafety::BranchCheckedOutElsewhere => (
            "Safe to remove: worktree will be deleted; branch is checked out elsewhere.",
            Color::Yellow,
        ),
        RemovalSafety::BranchNotIntegrated => (
            "Safe to remove: worktree will be deleted; branch changes are not integrated.",
            Color::Yellow,
        ),
        RemovalSafety::Unknown => (
            "Cannot verify safe removal. Worktrunk status is unavailable.",
            Color::Yellow,
        ),
    };
    let safety_para = Paragraph::new(Span::styled(message, Style::default().fg(color)))
        .alignment(Alignment::Center)
        .wrap(Wrap { trim: true });
    frame.render_widget(safety_para, layout[2]);

    // Buttons
    let buttons = if safety.allows_removal() {
        Line::from(vec![
            Span::styled("[y]", Style::default().fg(Color::Green).add_modifier(Modifier::BOLD)),
            Span::raw(" Remove    "),
            Span::styled("[n/Esc]", Style::default().fg(Color::Red).add_modifier(Modifier::BOLD)),
            Span::raw(" Cancel"),
        ])
    } else {
        Line::from(vec![
            Span::styled("[Esc]", Style::default().fg(Color::Red).add_modifier(Modifier::BOLD)),
            Span::raw(" Cancel"),
        ])
    };
    let buttons = Paragraph::new(buttons)
    .alignment(Alignment::Center);
    frame.render_widget(buttons, layout[3]);
}

#[cfg(test)]
mod tests {
    use super::*;
    use ratatui::{backend::TestBackend, Terminal};

    #[test]
    fn entry_indicators_use_distinct_symbols_labels_and_colors() {
        let indicators = [
            (EntryKind::WorktreeCurrent, "▶ here  ", TOKYO_NIGHT_ACCENT),
            (EntryKind::WorktreeMain, "★ main  ", TOKYO_NIGHT_GREEN),
            (EntryKind::WorktreeOther, "□ tree  ", TOKYO_NIGHT_TEAL),
            (EntryKind::BranchLocal, "⑂ local ", TOKYO_NIGHT_TEXT),
            (EntryKind::BranchRemote, "⇣ remote", TOKYO_NIGHT_BLUE),
            (EntryKind::NewWorktree, "+ new   ", TOKYO_NIGHT_YELLOW),
        ];

        for (kind, label, color) in indicators {
            let (indicator, style) = entry_indicator(&kind);
            assert_eq!(indicator, label);
            assert_eq!(style.fg, Some(color));
        }
    }

    #[test]
    fn branch_list_renders_hybrid_indicators() {
        let entries = vec![
            BranchEntry {
                kind: EntryKind::WorktreeCurrent,
                branch: "current".into(),
                path: None,
                symbols: String::new(),
            },
            BranchEntry {
                kind: EntryKind::WorktreeMain,
                branch: "main".into(),
                path: None,
                symbols: String::new(),
            },
            BranchEntry {
                kind: EntryKind::WorktreeOther,
                branch: "tree".into(),
                path: None,
                symbols: String::new(),
            },
            BranchEntry {
                kind: EntryKind::BranchLocal,
                branch: "local".into(),
                path: None,
                symbols: String::new(),
            },
            BranchEntry {
                kind: EntryKind::BranchRemote,
                branch: "remote".into(),
                path: None,
                symbols: String::new(),
            },
        ];
        let backend = TestBackend::new(40, 12);
        let mut terminal = Terminal::new(backend).unwrap();
        let mut app = App::new(Config::default(), String::new(), entries, false);
        app.apply_filter();

        terminal.draw(|frame| draw(frame, &mut app)).unwrap();

        let buffer = terminal.backend().buffer();
        let rendered = (0..buffer.area.height)
            .map(|y| {
                (0..buffer.area.width)
                    .map(|x| buffer.get(x, y).symbol())
                    .collect::<String>()
            })
            .collect::<Vec<_>>()
            .join("\n");

        for label in ["▶ here", "★ main", "□ tree", "⑂ local", "⇣ remote", "+ new"] {
            assert!(rendered.contains(label), "missing indicator: {label}");
        }
        assert!(buffer.content().iter().any(|cell| cell.bg == TOKYO_NIGHT_SURFACE));
        assert!(buffer.content().iter().any(|cell| cell.symbol() == "c" && cell.fg == TOKYO_NIGHT_TEXT));
    }

    #[test]
    fn confirmation_ui_leaves_the_title_to_herdr() {
        let backend = TestBackend::new(40, 10);
        let mut terminal = Terminal::new(backend).unwrap();

        terminal
            .draw(|frame| draw_confirm_ui(frame, "repo: branch", &RemovalSafety::Safe))
            .unwrap();

        let buffer = terminal.backend().buffer();
        let rendered = (0..buffer.area.height)
            .map(|y| {
                (0..buffer.area.width)
                    .map(|x| buffer.get(x, y).symbol())
                    .collect::<String>()
            })
            .collect::<Vec<_>>()
            .join("\n");

        assert!(rendered.contains("repo: branch"));
        assert!(rendered.contains("Safe to remove"));
        assert!(rendered.contains("[y] Remove"));
        assert!(!rendered.contains("Remove worktree?"));
    }

    #[test]
    fn unintegrated_confirmation_ui_keeps_the_branch() {
        let backend = TestBackend::new(80, 10);
        let mut terminal = Terminal::new(backend).unwrap();

        terminal
            .draw(|frame| {
                draw_confirm_ui(frame, "repo: branch", &RemovalSafety::BranchNotIntegrated)
            })
            .unwrap();

        let buffer = terminal.backend().buffer();
        let rendered = (0..buffer.area.height)
            .map(|y| {
                (0..buffer.area.width)
                    .map(|x| buffer.get(x, y).symbol())
                    .collect::<String>()
            })
            .collect::<Vec<_>>()
            .join("\n");

        assert!(rendered.contains("worktree will be deleted; branch changes are not integrated"));
        assert!(rendered.contains("[y] Remove"));
    }
}
