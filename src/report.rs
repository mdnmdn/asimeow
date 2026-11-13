//! Turns scan events into output.
//!
//! - [`LogReporter`]: plain lines for scripts and log files (stdout is not a terminal).
//! - [`CompactReporter`]: a live status line in a terminal, plus one line per change.
//!
//! Both print only what changed by default (newly excluded folders, failures, warnings)
//! and finish with a summary that includes how much space the exclusions save. `-v` adds
//! already-excluded folders, unreadable folders, folder sizes and traces.

use crate::cache::Cache;
use crate::config::Config;
use crate::scan::{self, Event, Kind, Options, Progress, Summary};
use anyhow::Result;
use std::io::Write;
use std::path::{Path, PathBuf};
use std::sync::atomic::Ordering;
use std::sync::mpsc::{self, RecvTimeoutError};
use std::thread;
use std::time::{Duration, Instant};

/// How often the live status line is redrawn.
const TICK: Duration = Duration::from_millis(80);

pub trait Reporter {
    fn event(&mut self, event: &Event);
    /// Called about every [`TICK`] while the scan runs.
    fn tick(&mut self, _progress: &Progress) {}
    fn finish(&mut self, summary: &Summary);
}

/// Runs a scan on a worker thread and feeds its events to `reporter` as they arrive.
/// Returns the summary and the cache to save for the next run (see [`scan::scan`]).
pub fn run(
    config: Config,
    opts: &Options,
    previous: &Cache,
    reporter: &mut dyn Reporter,
) -> Result<(Summary, Cache)> {
    let progress = Progress::default();
    let (tx, rx) = mpsc::channel();
    let (summary, cache) = thread::scope(|s| {
        let scan = s.spawn(|| scan::scan(config, opts, previous, tx, &progress));
        let mut last_tick = Instant::now();
        loop {
            match rx.recv_timeout(TICK) {
                Ok(event) => reporter.event(&event),
                Err(RecvTimeoutError::Timeout) => {}
                Err(RecvTimeoutError::Disconnected) => break,
            }
            if last_tick.elapsed() >= TICK {
                reporter.tick(&progress);
                last_tick = Instant::now();
            }
        }
        scan.join().expect("scan thread panicked")
    })?;
    reporter.finish(&summary);
    Ok((summary, cache))
}

/// Plain lines with absolute paths, for scripts and log files. Errors go to `err`.
/// The last line is a `key=value` summary.
pub struct LogReporter<O: Write, E: Write> {
    out: O,
    err: E,
    verbose: bool,
    dry_run: bool,
}

impl<O: Write, E: Write> LogReporter<O, E> {
    pub fn new(out: O, err: E, opts: &Options) -> Self {
        LogReporter {
            out,
            err,
            verbose: opts.verbose,
            dry_run: opts.dry_run,
        }
    }
}

impl<O: Write, E: Write> Reporter for LogReporter<O, E> {
    fn event(&mut self, event: &Event) {
        let dry = if self.dry_run { " (dry run)" } else { "" };
        let _ = match event {
            Event::Exclusion {
                path,
                rule,
                kind: Kind::New,
            } => writeln!(self.out, "✅ {} - {}{}", path.display(), rule, dry),
            Event::Exclusion {
                path,
                rule,
                kind: Kind::Recreated,
            } => writeln!(
                self.out,
                "✅ {} - {} (recreated){}",
                path.display(),
                rule,
                dry
            ),
            Event::Exclusion { path, rule, .. } if self.verbose => {
                writeln!(self.out, "🟡 {} - {}", path.display(), rule)
            }
            Event::Sized {
                path,
                bytes,
                measured,
            } if self.verbose => writeln!(
                self.out,
                "Size of {}: {} ({})",
                path.display(),
                format_bytes(*bytes),
                if *measured { "measured" } else { "cached" }
            ),
            Event::Failed { path, rule } => writeln!(
                self.err,
                "❌ {} - {} (could not be excluded)",
                path.display(),
                rule
            ),
            Event::Unreadable { path, error } if self.verbose => {
                writeln!(self.err, "⚠️  Could not read {}: {}", path.display(), error)
            }
            Event::Warning(message) => writeln!(self.err, "⚠️  {message}"),
            Event::Trace(message) => writeln!(self.out, "{message}"),
            _ => Ok(()),
        };
    }

    fn finish(&mut self, s: &Summary) {
        let _ = writeln!(
            self.out,
            "summary: folders={} exclusions={} new={} recreated={} failed={} unreadable={} \
             excluded_bytes={} new_bytes={} seconds={:.2}{}",
            s.dirs,
            s.exclusions,
            s.newly_excluded,
            s.recreated,
            s.failed,
            s.unreadable,
            s.excluded_bytes,
            s.new_bytes,
            s.elapsed.as_secs_f64(),
            if self.dry_run { " dry_run=true" } else { "" }
        );
    }
}

/// A live status line for terminals: a spinner, the folder count and the current folder,
/// redrawn in place. Changes print above it; a short summary replaces it at the end.
pub struct CompactReporter<W: Write> {
    out: W,
    verbose: bool,
    dry_run: bool,
    color: bool,
    home: Option<PathBuf>,
    frame: usize,
    status_shown: bool,
}

impl<W: Write> CompactReporter<W> {
    pub fn new(out: W, opts: &Options, color: bool) -> Self {
        CompactReporter {
            out,
            verbose: opts.verbose,
            dry_run: opts.dry_run,
            color,
            home: dirs::home_dir(),
            frame: 0,
            status_shown: false,
        }
    }

    fn paint(&self, code: &str, text: &str) -> String {
        if self.color {
            format!("\x1b[{code}m{text}\x1b[0m")
        } else {
            text.to_string()
        }
    }

    fn dim(&self, text: &str) -> String {
        self.paint("2", text)
    }

    fn short(&self, path: &Path) -> String {
        match self.home.as_deref().and_then(|h| path.strip_prefix(h).ok()) {
            Some(rest) => format!("~/{}", rest.display()),
            None => path.display().to_string(),
        }
    }

    fn clear_status(&mut self) {
        if self.status_shown {
            let _ = write!(self.out, "\r\x1b[2K");
            self.status_shown = false;
        }
    }

    fn line(&mut self, text: String) {
        self.clear_status();
        let _ = writeln!(self.out, "{text}");
    }
}

impl<W: Write> Reporter for CompactReporter<W> {
    fn event(&mut self, event: &Event) {
        let text = match event {
            Event::Exclusion {
                path,
                rule,
                kind: Kind::New,
            } => {
                let dry = if self.dry_run { " (dry run)" } else { "" };
                format!(
                    "✅ {}  {}",
                    self.short(path),
                    self.dim(&format!("{rule}{dry}"))
                )
            }
            Event::Exclusion {
                path,
                rule,
                kind: Kind::Recreated,
            } => {
                let dry = if self.dry_run { " (dry run)" } else { "" };
                format!(
                    "✅ {}  {}",
                    self.short(path),
                    self.dim(&format!("{rule} · recreated{dry}"))
                )
            }
            Event::Exclusion { path, rule, .. } if self.verbose => {
                format!("🟡 {}  {}", self.short(path), self.dim(rule))
            }
            Event::Failed { path, rule } => format!(
                "❌ {}  {}  {}",
                self.short(path),
                self.dim(rule),
                self.paint("31", "could not be excluded")
            ),
            Event::Unreadable { path, error } if self.verbose => {
                let message = format!("could not read {}: {error}", self.short(path));
                format!("⚠️  {}", self.paint("33", &message))
            }
            Event::Warning(message) => format!("⚠️  {}", self.paint("33", message)),
            Event::Sized {
                path,
                bytes,
                measured,
            } if self.verbose => {
                let how = if *measured { "measured" } else { "cached" };
                self.dim(&format!(
                    "{} {}  {how}",
                    format_bytes(*bytes),
                    self.short(path)
                ))
            }
            Event::Trace(message) => self.dim(message),
            _ => return,
        };
        self.line(text);
    }

    fn tick(&mut self, progress: &Progress) {
        const SPINNER: [char; 10] = ['⠋', '⠙', '⠹', '⠸', '⠼', '⠴', '⠦', '⠧', '⠇', '⠏'];
        self.frame = (self.frame + 1) % SPINNER.len();
        let to_size = progress.to_size.load(Ordering::Relaxed);
        let head = if to_size == 0 {
            format!(
                "{} {} folders · ",
                SPINNER[self.frame],
                group_digits(progress.dirs.load(Ordering::Relaxed))
            )
        } else {
            format!(
                "{} sizing {}/{} · ",
                SPINNER[self.frame],
                group_digits(progress.sized.load(Ordering::Relaxed)),
                group_digits(to_size)
            )
        };
        // Keep the line one column short of the width so it never wraps.
        let room = terminal_width().saturating_sub(head.chars().count() + 1);
        let current = truncate_left(&self.short(&progress.current()), room);
        let status = self.dim(&format!("{head}{current}"));
        self.clear_status();
        let _ = write!(self.out, "{status}");
        let _ = self.out.flush();
        self.status_shown = true;
    }

    fn finish(&mut self, s: &Summary) {
        let mut text = format!(
            "✓ Done in {:.1}s · {} · {}, {} new",
            s.elapsed.as_secs_f64(),
            counted(s.dirs, "folder", "folders"),
            counted(s.exclusions, "exclusion", "exclusions"),
            group_digits(s.newly_excluded)
        );
        if s.recreated > 0 {
            text += &format!(" ({} recreated)", group_digits(s.recreated));
        }
        if s.failed > 0 {
            text += &self.paint("31", &format!(", {} failed", s.failed));
        }
        if self.dry_run {
            text += &self.dim(" · dry run, nothing written");
        }
        self.line(text);
        self.line(format!("  {}", savings(s)));
        if s.unreadable > 0 && !self.verbose {
            let hint = format!(
                "  {} not be read; run with -v to list them",
                counted(s.unreadable, "folder could", "folders could")
            );
            self.line(self.dim(&hint));
        }
        let _ = self.out.flush();
    }
}

/// `9.3 GB excluded · +668 MB this run · +0.6 GB since last run`.
/// The last two parts are left out when there's nothing new or no previous run.
pub fn savings(s: &Summary) -> String {
    let mut text = format!("{} excluded", format_bytes(s.excluded_bytes));
    if s.newly_excluded > 0 {
        text += &format!(" · +{} this run", format_bytes(s.new_bytes));
    }
    if let Some(previous) = s.previous_bytes {
        let (sign, delta) = if s.excluded_bytes >= previous {
            ("+", s.excluded_bytes - previous)
        } else {
            ("−", previous - s.excluded_bytes)
        };
        text += &format!(" · {sign}{} since last run", format_bytes(delta));
    }
    text
}

/// What `asimeow stats` prints: savings and the largest folders, from the cache alone.
pub fn stats(cache: &Cache, now: u64, home: Option<&Path>) -> String {
    let Some(run) = &cache.last_run else {
        return "No scan recorded yet. Run asimeow first.\n".to_string();
    };
    let short = |path: &Path| match home.and_then(|h| path.strip_prefix(h).ok()) {
        Some(rest) => format!("~/{}", rest.display()),
        None => path.display().to_string(),
    };

    let mut out = format!(
        "{} excluded in {} · last scan {} ({})\n",
        format_bytes(run.excluded_bytes),
        counted(cache.exclusions.len(), "folder", "folders"),
        ago(now.saturating_sub(run.finished_at)),
        counted(run.dirs, "folder scanned", "folders scanned"),
    );
    let mut largest: Vec<_> = cache.exclusions.iter().collect();
    largest.sort_by(|a, b| b.1.bytes.cmp(&a.1.bytes).then(a.0.cmp(b.0)));
    if !largest.is_empty() {
        out += "\nLargest:\n";
    }
    for (path, entry) in largest.iter().take(10) {
        out += &format!(
            "{:>9}  {}  {}\n",
            format_bytes(entry.bytes),
            short(path),
            entry.rule
        );
    }
    out
}

/// `45` → `just now`, `7200` → `2 h ago`, `200000` → `2 days ago`.
fn ago(seconds: u64) -> String {
    match seconds {
        0..60 => "just now".to_string(),
        60..3600 => format!("{} min ago", seconds / 60),
        3600..86400 => format!("{} h ago", seconds / 3600),
        _ => counted((seconds / 86400) as usize, "day ago", "days ago"),
    }
}

/// Decimal units, like Finder: `668000000` → `668 MB`, `9319000000` → `9.3 GB`.
pub fn format_bytes(bytes: u64) -> String {
    const UNITS: [&str; 5] = ["B", "KB", "MB", "GB", "TB"];
    let mut value = bytes as f64;
    let mut unit = 0;
    while value >= 1000.0 && unit < UNITS.len() - 1 {
        value /= 1000.0;
        unit += 1;
    }
    if unit == 0 || value >= 100.0 {
        format!("{value:.0} {}", UNITS[unit])
    } else {
        format!("{value:.1} {}", UNITS[unit])
    }
}

/// `1, "folder", "folders"` → `1 folder`; `2000, …` → `2,000 folders`.
fn counted(n: usize, one: &str, many: &str) -> String {
    format!("{} {}", group_digits(n), if n == 1 { one } else { many })
}

/// `12345` → `12,345`.
pub fn group_digits(n: usize) -> String {
    let digits = n.to_string();
    let mut out = String::new();
    for (i, c) in digits.chars().enumerate() {
        if i > 0 && (digits.len() - i).is_multiple_of(3) {
            out.push(',');
        }
        out.push(c);
    }
    out
}

/// Keeps the end of `text` (the interesting part of a path) within `max` characters.
pub fn truncate_left(text: &str, max: usize) -> String {
    let count = text.chars().count();
    if count <= max {
        return text.to_string();
    }
    if max == 0 {
        return String::new();
    }
    let tail: String = text.chars().skip(count - (max - 1)).collect();
    format!("…{tail}")
}

/// True unless the `NO_COLOR` convention asks for no color.
pub fn color_enabled() -> bool {
    std::env::var_os("NO_COLOR").is_none_or(|v| v.is_empty())
}

/// Columns of the terminal on stdout, or 80 if unknown.
fn terminal_width() -> usize {
    use std::ffi::{c_int, c_ulong};

    #[repr(C)]
    struct Winsize {
        rows: u16,
        cols: u16,
        x_pixels: u16,
        y_pixels: u16,
    }
    extern "C" {
        fn ioctl(fd: c_int, request: c_ulong, ...) -> c_int;
    }
    const TIOCGWINSZ: c_ulong = 0x4008_7468; // macOS

    let mut size = Winsize {
        rows: 0,
        cols: 0,
        x_pixels: 0,
        y_pixels: 0,
    };
    // SAFETY: TIOCGWINSZ writes one Winsize into the pointer we pass.
    let ok = unsafe { ioctl(1, TIOCGWINSZ, &mut size as *mut Winsize) } == 0;
    if ok && size.cols > 0 {
        size.cols as usize
    } else {
        80
    }
}
