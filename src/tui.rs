//! `asimeow --visual`: a full-screen view of a scan.
//!
//! One screen instead of a stream of lines: live progress, the space the exclusions save,
//! where that space is (excluded folders grouped by area, sorted by size) and recent
//! activity. It stays open after the scan so the folders can be browsed. On quit, the
//! changes and the summary are printed to the normal screen so they stay in scrollback.
//!
//! [`App`] holds the screen state and draws it; [`TuiReporter`] connects it to a scan and
//! to the terminal. Colors are the 16 ANSI colors, so the terminal's theme applies.

use crate::cache::LastRun;
use crate::report::{format_bytes, group_digits, truncate_left, CompactReporter, Reporter};
use crate::scan::{Event, Kind, Options, Progress, Summary};
use ratatui::crossterm::event::{self as term, KeyCode, KeyEvent, KeyEventKind, KeyModifiers};
use ratatui::layout::{Constraint, Layout, Rect};
use ratatui::style::{Color, Modifier, Style};
use ratatui::text::{Line, Span};
use ratatui::widgets::{Block, Gauge, List, ListItem, ListState, Paragraph};
use ratatui::{DefaultTerminal, Frame};
use std::collections::{BTreeMap, HashSet, VecDeque};
use std::io;
use std::path::{Path, PathBuf};
use std::sync::atomic::Ordering;
use std::time::{Duration, Instant};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Phase {
    Walking,
    Sizing,
    Done,
    Cancelled,
}

/// An excluded folder as shown in the tree.
#[derive(Debug, Clone)]
struct Folder {
    rule: String,
    kind: Kind,
    bytes: Option<u64>,
}

/// Excluded folders that share a parent area, with their total size.
struct Group<'a> {
    path: PathBuf,
    total: u64,
    folders: Vec<(&'a PathBuf, &'a Folder)>,
}

/// One visible line of the tree.
#[derive(Debug, Clone, PartialEq, Eq)]
enum Row {
    Group(PathBuf),
    Item(PathBuf),
}

/// The screen state of `--visual`, independent of the terminal.
pub struct App {
    dry_run: bool,
    threads: usize,
    color: bool,
    home: Option<PathBuf>,
    previous: Option<LastRun>,
    started: Instant,

    phase: Phase,
    elapsed: Duration,
    dirs: usize,
    to_size: usize,
    sized: usize,
    current: PathBuf,
    summary: Option<Summary>,

    folders: BTreeMap<PathBuf, Folder>,
    activity: VecDeque<(Color, String)>,
    /// Changes worth printing after the screen closes.
    changes: Vec<Event>,

    collapsed: HashSet<PathBuf>,
    only_changes: bool,
    list: ListState,
    quit: bool,
}

impl App {
    /// `previous` is the last run from the cache (for the progress estimate and the
    /// change in savings); `known` is the number of folders the cache remembers.
    pub fn new(
        opts: &Options,
        previous: Option<LastRun>,
        known: usize,
        color: bool,
        home: Option<PathBuf>,
    ) -> Self {
        let mut app = App {
            dry_run: opts.dry_run,
            threads: opts.threads.max(1),
            color,
            home,
            previous,
            started: Instant::now(),
            phase: Phase::Walking,
            elapsed: Duration::ZERO,
            dirs: 0,
            to_size: 0,
            sized: 0,
            current: PathBuf::new(),
            summary: None,
            folders: BTreeMap::new(),
            activity: VecDeque::new(),
            changes: Vec::new(),
            collapsed: HashSet::new(),
            only_changes: false,
            list: ListState::default().with_selected(Some(0)),
            quit: false,
        };
        let note = match &app.previous {
            Some(run) => format!(
                "Cache: {} known exclusions, {} at the last run",
                group_digits(known),
                format_bytes(run.excluded_bytes)
            ),
            None => "No cache yet: every folder gets measured".to_string(),
        };
        app.log(Color::DarkGray, note);
        app
    }

    pub fn quit_requested(&self) -> bool {
        self.quit
    }

    pub fn phase(&self) -> Phase {
        self.phase
    }

    fn log(&mut self, color: Color, text: String) {
        self.activity.push_back((color, text));
        if self.activity.len() > 50 {
            self.activity.pop_front();
        }
    }

    fn short(&self, path: &Path) -> String {
        match self.home.as_deref().and_then(|h| path.strip_prefix(h).ok()) {
            Some(rest) => format!("~/{}", rest.display()),
            None => path.display().to_string(),
        }
    }

    pub fn on_event(&mut self, event: &Event) {
        match event {
            Event::Exclusion { path, rule, kind } => {
                self.folders.insert(
                    path.clone(),
                    Folder {
                        rule: rule.clone(),
                        kind: *kind,
                        bytes: None,
                    },
                );
                let short = self.short(path);
                let verb = if self.dry_run { "would be" } else { "was" };
                match kind {
                    Kind::New => {
                        self.log(Color::Green, format!("+ {short} {verb} excluded ({rule})"))
                    }
                    Kind::Recreated => self.log(
                        Color::Yellow,
                        format!("↻ {short} {verb} excluded again: the folder was recreated"),
                    ),
                    Kind::Already => return,
                }
                self.changes.push(event.clone());
            }
            Event::Sized { path, bytes, .. } => {
                if let Some(folder) = self.folders.get_mut(path) {
                    folder.bytes = Some(*bytes);
                }
            }
            Event::Failed { path, .. } => {
                let text = format!("✗ {} could not be excluded", self.short(path));
                self.log(Color::Red, text);
                self.changes.push(event.clone());
            }
            Event::Warning(message) => {
                self.log(Color::Yellow, format!("! {message}"));
                self.changes.push(event.clone());
            }
            Event::Unreadable { .. } | Event::Trace(_) => {}
        }
    }

    pub fn on_progress(&mut self, progress: &Progress) {
        self.elapsed = self.started.elapsed();
        self.dirs = progress.dirs.load(Ordering::Relaxed);
        self.to_size = progress.to_size.load(Ordering::Relaxed);
        self.sized = progress.sized.load(Ordering::Relaxed);
        self.current = progress.current();
        if self.to_size > 0 && self.phase == Phase::Walking {
            self.phase = Phase::Sizing;
        }
    }

    pub fn on_finish(&mut self, summary: &Summary) {
        self.elapsed = summary.elapsed;
        self.dirs = summary.dirs;
        self.phase = if summary.cancelled {
            Phase::Cancelled
        } else {
            Phase::Done
        };
        let text = if summary.cancelled {
            "Stopped early: sizes are incomplete and the cache was not saved".to_string()
        } else if self.dry_run {
            format!(
                "Done: {} would be excluded (dry run, nothing written)",
                group_digits(summary.newly_excluded)
            )
        } else {
            format!(
                "Done: {} newly excluded, {} failed",
                group_digits(summary.newly_excluded),
                group_digits(summary.failed)
            )
        };
        let color = if summary.failed > 0 {
            Color::Red
        } else {
            Color::Green
        };
        self.log(color, format!("✓ {text}"));
        self.summary = Some(summary.clone());
    }

    /// Handles one key press.
    pub fn on_key(&mut self, key: KeyEvent) {
        let rows = self.rows();
        let selected = self
            .list
            .selected()
            .unwrap_or(0)
            .min(rows.len().saturating_sub(1));
        match key.code {
            KeyCode::Char('q') | KeyCode::Esc => self.quit = true,
            KeyCode::Char('c') if key.modifiers.contains(KeyModifiers::CONTROL) => self.quit = true,
            KeyCode::Up | KeyCode::Char('k') => self.list.select(Some(selected.saturating_sub(1))),
            KeyCode::Down | KeyCode::Char('j') => self.list.select(Some(selected + 1)),
            KeyCode::PageUp => self.list.select(Some(selected.saturating_sub(10))),
            KeyCode::PageDown => self.list.select(Some(selected + 10)),
            KeyCode::Home | KeyCode::Char('g') => self.list.select(Some(0)),
            KeyCode::End | KeyCode::Char('G') => self.list.select(Some(rows.len())),
            KeyCode::Left | KeyCode::Char('h') => match rows.get(selected) {
                Some(Row::Group(group)) => {
                    self.collapsed.insert(group.clone());
                }
                Some(Row::Item(path)) => {
                    // Jump to the item's group.
                    let group = group_of(path);
                    if let Some(i) = rows.iter().position(|r| *r == Row::Group(group.clone())) {
                        self.list.select(Some(i));
                    }
                }
                None => {}
            },
            KeyCode::Right | KeyCode::Char('l') => {
                if let Some(Row::Group(group)) = rows.get(selected) {
                    self.collapsed.remove(group);
                }
            }
            KeyCode::Enter | KeyCode::Char(' ') => {
                if let Some(Row::Group(group)) = rows.get(selected) {
                    if !self.collapsed.remove(group) {
                        self.collapsed.insert(group.clone());
                    }
                }
            }
            KeyCode::Char('n') => {
                self.only_changes = !self.only_changes;
                self.list.select(Some(0));
            }
            _ => {}
        }
        let len = self.rows().len();
        if let Some(i) = self.list.selected() {
            self.list.select(Some(i.min(len.saturating_sub(1))));
        }
    }

    fn visible(&self, folder: &Folder) -> bool {
        !self.only_changes || folder.kind != Kind::Already
    }

    /// Groups with their visible folders, biggest first.
    fn groups(&self) -> Vec<Group<'_>> {
        let mut by_path: BTreeMap<PathBuf, Vec<(&PathBuf, &Folder)>> = BTreeMap::new();
        for (path, folder) in &self.folders {
            if self.visible(folder) {
                by_path
                    .entry(group_of(path))
                    .or_default()
                    .push((path, folder));
            }
        }
        let mut groups: Vec<Group> = by_path
            .into_iter()
            .map(|(path, mut folders)| {
                folders.sort_by(|a, b| b.1.bytes.cmp(&a.1.bytes).then(a.0.cmp(b.0)));
                let total = folders.iter().filter_map(|(_, f)| f.bytes).sum();
                Group {
                    path,
                    total,
                    folders,
                }
            })
            .collect();
        groups.sort_by(|a, b| b.total.cmp(&a.total).then(a.path.cmp(&b.path)));
        groups
    }

    fn rows(&self) -> Vec<Row> {
        let mut rows = Vec::new();
        for group in self.groups() {
            let open = !self.collapsed.contains(&group.path);
            rows.push(Row::Group(group.path));
            if open {
                rows.extend(
                    group
                        .folders
                        .into_iter()
                        .map(|(path, _)| Row::Item(path.clone())),
                );
            }
        }
        rows
    }

    fn fg(&self, color: Color) -> Style {
        if self.color {
            Style::new().fg(color)
        } else {
            Style::new()
        }
    }

    fn dim(&self) -> Style {
        self.fg(Color::DarkGray)
    }

    fn block<'a>(&self, title: &'a str) -> Block<'a> {
        Block::bordered()
            .border_style(self.dim())
            .title(Span::styled(
                format!(" {title} "),
                Style::new().add_modifier(Modifier::BOLD),
            ))
    }

    /// Draws the whole screen.
    pub fn render(&mut self, frame: &mut Frame) {
        let [header, savings, tree, activity, footer] = Layout::vertical([
            Constraint::Length(5),
            Constraint::Length(4),
            Constraint::Min(5),
            Constraint::Length(5),
            Constraint::Length(1),
        ])
        .areas(frame.area());
        self.render_header(frame, header);
        self.render_savings(frame, savings);
        self.render_tree(frame, tree);
        self.render_activity(frame, activity);
        let keys = format!(
            " q quit · ↑↓ move · ←→ fold · n {}{}",
            if self.only_changes {
                "show all"
            } else {
                "only changes"
            },
            if self.dry_run {
                " · dry run: nothing is written"
            } else {
                ""
            }
        );
        frame.render_widget(Paragraph::new(keys).style(self.dim()), footer);
    }

    fn render_header(&self, frame: &mut Frame, area: Rect) {
        let secs = self.elapsed.as_secs_f64();
        let clock = format!("{}:{:04.1}", (secs / 60.0) as u64, secs % 60.0);
        let status = match self.phase {
            Phase::Walking => format!(" scanning · {clock} "),
            Phase::Sizing => format!(" measuring · {clock} "),
            Phase::Done => format!(" done in {secs:.1}s "),
            Phase::Cancelled => " stopped ".to_string(),
        };
        let block = self
            .block(concat!("asimeow ", env!("CARGO_PKG_VERSION")))
            .title_top(Line::from(status).right_aligned());
        let inner = block.inner(area);
        frame.render_widget(block, area);
        let [gauge_area, stats_area, current_area] =
            Layout::vertical([Constraint::Length(1); 3]).areas(inner);

        let estimate = self.previous.as_ref().map(|r| r.dirs).filter(|d| *d > 0);
        let rate = self.dirs as f64 / secs.max(0.001);
        let (ratio, label) = match self.phase {
            Phase::Walking => match estimate {
                Some(total) => {
                    let ratio = (self.dirs as f64 / total as f64).min(0.99);
                    let left = total.saturating_sub(self.dirs) as f64 / rate.max(1.0);
                    (ratio, format!("{:.0}% · ~{left:.0}s left", ratio * 100.0))
                }
                None => (0.0, "first scan: no estimate yet".to_string()),
            },
            Phase::Sizing => (
                self.sized as f64 / self.to_size.max(1) as f64,
                format!(
                    "measuring {}/{}",
                    group_digits(self.sized),
                    group_digits(self.to_size)
                ),
            ),
            Phase::Done => (1.0, "100%".to_string()),
            Phase::Cancelled => (0.0, "stopped".to_string()),
        };
        frame.render_widget(
            Gauge::default()
                .ratio(ratio.clamp(0.0, 1.0))
                .label(label)
                .use_unicode(true)
                .gauge_style(self.fg(Color::Blue)),
            gauge_area,
        );

        let stats = match (self.phase, &self.summary) {
            (Phase::Done | Phase::Cancelled, Some(s)) => {
                let mut text = format!(
                    "{} folders · {} exclusions, {} new",
                    group_digits(s.dirs),
                    group_digits(s.exclusions),
                    group_digits(s.newly_excluded)
                );
                if s.recreated > 0 {
                    text += &format!(" ({} recreated)", group_digits(s.recreated));
                }
                if s.unreadable > 0 {
                    text += &format!(" · {} unreadable", group_digits(s.unreadable));
                }
                text
            }
            _ => match estimate {
                Some(total) => format!(
                    "{} / ~{} folders · {}/s · {} workers",
                    group_digits(self.dirs),
                    group_digits(total),
                    group_digits(rate as usize),
                    self.threads
                ),
                None => format!(
                    "{} folders · {}/s · {} workers",
                    group_digits(self.dirs),
                    group_digits(rate as usize),
                    self.threads
                ),
            },
        };
        frame.render_widget(Paragraph::new(stats), stats_area);

        let width = current_area.width as usize;
        let current = match self.phase {
            Phase::Walking | Phase::Sizing => Line::from(vec![
                Span::styled("▸ ", self.fg(Color::Blue)),
                Span::styled(
                    truncate_left(&self.short(&self.current), width.saturating_sub(2)),
                    self.dim(),
                ),
            ]),
            Phase::Done => Line::from(vec![
                Span::styled("✓ ", self.fg(Color::Green)),
                Span::raw("Scan complete. Browse the folders below, q to quit."),
            ]),
            Phase::Cancelled => Line::styled(
                "Stopped early: sizes are incomplete and the cache was not saved.",
                self.fg(Color::Yellow),
            ),
        };
        frame.render_widget(Paragraph::new(current), current_area);
    }

    fn render_savings(&self, frame: &mut Frame, area: Rect) {
        let block = self.block("Savings");
        let inner = block.inner(area);
        frame.render_widget(block, area);

        let total: u64 = self.folders.values().filter_map(|f| f.bytes).sum();
        let changed: Vec<&Folder> = self
            .folders
            .values()
            .filter(|f| f.kind != Kind::Already)
            .collect();
        let new_bytes: u64 = changed.iter().filter_map(|f| f.bytes).sum();
        let recreated = changed.iter().filter(|f| f.kind == Kind::Recreated).count();
        let to_measure = self.folders.values().filter(|f| f.bytes.is_none()).count();
        let finished = matches!(self.phase, Phase::Done);

        let half = inner.width as usize / 2;
        let label = |text: &str| Span::styled(format!("{text:<13}"), self.dim());
        let value = |text: String, style: Style| {
            Span::styled(format!("{text:<10}"), style.add_modifier(Modifier::BOLD))
        };
        let pad = |spans: &[Span]| {
            let used: usize = spans.iter().map(|s| s.content.chars().count()).sum();
            Span::raw(" ".repeat(half.saturating_sub(used)))
        };

        let mut first = vec![
            label("Excluded"),
            value(format_bytes(total), Style::new()),
            Span::styled(
                format!("in {} folders", group_digits(self.folders.len())),
                self.dim(),
            ),
        ];
        first.push(pad(&first));
        first.extend([
            label("New this run"),
            value(
                format!("+{}", format_bytes(new_bytes)),
                self.fg(Color::Green),
            ),
            Span::styled(
                format!(
                    "{} new · {} recreated",
                    group_digits(changed.len() - recreated),
                    group_digits(recreated)
                ),
                self.dim(),
            ),
        ]);

        let mut second = match &self.previous {
            Some(run) => vec![
                label("Last run"),
                value(format_bytes(run.excluded_bytes), Style::new()),
            ],
            None => vec![label("Last run"), Span::styled("none yet", self.dim())],
        };
        second.push(pad(&second));
        if finished {
            if let Some(run) = &self.previous {
                let (sign, delta) = if total >= run.excluded_bytes {
                    ("+", total - run.excluded_bytes)
                } else {
                    ("−", run.excluded_bytes - total)
                };
                second.extend([
                    label("Change"),
                    Span::raw(format!("{sign}{} since last run", format_bytes(delta))),
                ]);
            }
        } else {
            second.extend([
                label("To measure"),
                Span::raw(format!("{} folders", group_digits(to_measure))),
            ]);
        }

        let text = vec![Line::from(first), Line::from(second)];
        frame.render_widget(Paragraph::new(text), inner);
    }

    fn render_tree(&mut self, frame: &mut Frame, area: Rect) {
        let block = self
            .block("Where the space goes")
            .title_top(Line::from(" sorted by size ").right_aligned());
        let width = block.inner(area).width as usize;
        let groups = self.groups();
        let max_group = groups.iter().map(|g| g.total).max().unwrap_or(0).max(1);
        let bar_width = if width >= 60 { 16 } else { 0 };

        let mut items: Vec<ListItem> = Vec::new();
        for Group {
            path: group,
            total,
            folders,
        } in &groups
        {
            let open = !self.collapsed.contains(group);
            let name_width = width.saturating_sub(2 + 10 + 1 + bar_width);
            let bar_len = ((*total as f64 / max_group as f64) * bar_width as f64).round() as usize;
            items.push(ListItem::new(Line::from(vec![
                Span::styled(if open { "▾ " } else { "▸ " }, self.fg(Color::Blue)),
                Span::styled(
                    fit(&self.short(group), name_width),
                    Style::new().add_modifier(Modifier::BOLD),
                ),
                Span::raw(format!("{:>10} ", format_bytes(*total))),
                Span::styled(
                    "█".repeat(bar_len.max(usize::from(bar_width > 0))),
                    self.fg(Color::Blue),
                ),
            ])));
            if !open {
                continue;
            }
            for (path, folder) in folders {
                let (glyph, color, note) = match folder.kind {
                    Kind::New => ("+", Color::Green, "new"),
                    Kind::Recreated => ("↻", Color::Yellow, "recreated"),
                    Kind::Already => ("·", Color::DarkGray, ""),
                };
                let label_width = width.saturating_sub(4 + 2 + 17 + 10 + 2 + 9);
                let size = folder.bytes.map_or("…".to_string(), format_bytes);
                items.push(ListItem::new(Line::from(vec![
                    Span::raw("    "),
                    Span::styled(format!("{glyph} "), self.fg(color)),
                    Span::raw(fit(&item_label(path), label_width)),
                    Span::styled(fit(&folder.rule, 17), self.dim()),
                    Span::raw(format!("{size:>10}  ")),
                    Span::styled(note, self.fg(color)),
                ])));
            }
        }
        if items.is_empty() {
            let text = match self.phase {
                Phase::Walking | Phase::Sizing => "Waiting for the first excluded folder…",
                _ if self.only_changes => {
                    "Nothing changed in this run. Press n to show all folders."
                }
                _ => "No excluded folders found.",
            };
            items.push(ListItem::new(Span::styled(text, self.dim())));
        }

        let list = List::new(items)
            .block(block)
            .highlight_style(Style::new().add_modifier(Modifier::REVERSED));
        frame.render_stateful_widget(list, area, &mut self.list);
    }

    fn render_activity(&self, frame: &mut Frame, area: Rect) {
        let block = self.block("Activity");
        let inner = block.inner(area);
        frame.render_widget(block, area);
        let lines: Vec<Line> = self
            .activity
            .iter()
            .rev()
            .take(inner.height as usize)
            .rev()
            .map(|(color, text)| {
                Line::styled(truncate_left(text, inner.width as usize), self.fg(*color))
            })
            .collect();
        frame.render_widget(Paragraph::new(lines), inner);
    }
}

/// The tree groups folders by the folder that holds their project:
/// `~/projects/app/node_modules` is listed as `app/node_modules` under
/// `~/projects`.
fn group_of(path: &Path) -> PathBuf {
    path.parent()
        .and_then(Path::parent)
        .or_else(|| path.parent())
        .unwrap_or(path)
        .to_path_buf()
}

/// `app/node_modules` for `~/projects/app/node_modules`.
fn item_label(path: &Path) -> String {
    let group = group_of(path);
    path.strip_prefix(&group)
        .map(|p| p.display().to_string())
        .unwrap_or_else(|_| path.display().to_string())
}

/// Pads or cuts `text` to exactly `width` characters.
fn fit(text: &str, width: usize) -> String {
    let count = text.chars().count();
    if count <= width {
        format!("{text}{}", " ".repeat(width - count))
    } else if width == 0 {
        String::new()
    } else {
        let mut cut: String = text.chars().take(width - 1).collect();
        cut.push('…');
        cut
    }
}

/// Connects [`App`] to a scan and to the terminal: switches to the alternate screen on
/// start, redraws on every tick, reads keys, and cancels the scan when the user quits.
pub struct TuiReporter {
    terminal: DefaultTerminal,
    app: App,
}

impl TuiReporter {
    /// Takes over the terminal (alternate screen, raw mode). Call [`TuiReporter::close`]
    /// afterwards; ratatui also restores the terminal if the program panics.
    pub fn start(app: App) -> Self {
        TuiReporter {
            terminal: ratatui::init(),
            app,
        }
    }

    pub fn quit_requested(&self) -> bool {
        self.app.quit_requested()
    }

    fn draw(&mut self) {
        let app = &mut self.app;
        let _ = self.terminal.draw(|frame| app.render(frame));
    }

    /// Handles all key presses that are waiting, without blocking.
    fn read_keys(&mut self) {
        while term::poll(Duration::ZERO).unwrap_or(false) {
            if let Ok(term::Event::Key(key)) = term::read() {
                if key.kind == KeyEventKind::Press {
                    self.app.on_key(key);
                }
            }
        }
    }

    /// After the scan: keeps the screen up for browsing until the user quits.
    pub fn wait_for_quit(&mut self) {
        while !self.app.quit_requested() {
            self.draw();
            if term::poll(Duration::from_millis(250)).unwrap_or(false) {
                self.read_keys();
            }
        }
    }

    /// Restores the terminal and prints the changes and the summary to the normal screen.
    pub fn close(self, summary: Option<&Summary>, opts: &Options, color: bool) {
        ratatui::restore();
        let Some(summary) = summary else {
            return;
        };
        let mut compact = CompactReporter::new(io::stdout(), opts, color);
        for event in &self.app.changes {
            compact.event(event);
        }
        compact.finish(summary);
    }
}

impl Reporter for TuiReporter {
    fn event(&mut self, event: &Event) {
        self.app.on_event(event);
    }

    fn tick(&mut self, progress: &Progress) {
        self.app.on_progress(progress);
        self.read_keys();
        if self.app.quit_requested() {
            progress.cancel.store(true, Ordering::Relaxed);
        }
        self.draw();
    }

    fn finish(&mut self, summary: &Summary) {
        self.app.on_finish(summary);
        self.draw();
    }
}
