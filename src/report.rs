//! Turns scan events into output.
//!
//! - [`LogReporter`]: plain lines for scripts and log files (stdout is not a terminal).
//! - [`CompactReporter`]: a live status line in a terminal, plus one line per change.
//!
//! Both print only what changed by default (newly excluded folders, failures, warnings)
//! and finish with a summary. `-v` adds already-excluded folders, unreadable folders and
//! traces.

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
pub fn run(config: Config, opts: &Options, reporter: &mut dyn Reporter) -> Result<Summary> {
    let progress = Progress::default();
    let (tx, rx) = mpsc::channel();
    let summary = thread::scope(|s| {
        let scan = s.spawn(|| scan::scan(config, opts, tx, &progress));
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
    Ok(summary)
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
            Event::Exclusion { path, rule, .. } if self.verbose => {
                writeln!(self.out, "🟡 {} - {}", path.display(), rule)
            }
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
            "summary: folders={} exclusions={} new={} failed={} unreadable={} seconds={:.2}{}",
            s.dirs,
            s.exclusions,
            s.newly_excluded,
            s.failed,
            s.unreadable,
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
            Event::Trace(message) => self.dim(message),
            _ => return,
        };
        self.line(text);
    }

    fn tick(&mut self, progress: &Progress) {
        const SPINNER: [char; 10] = ['⠋', '⠙', '⠹', '⠸', '⠼', '⠴', '⠦', '⠧', '⠇', '⠏'];
        self.frame = (self.frame + 1) % SPINNER.len();
        let head = format!(
            "{} {} folders · ",
            SPINNER[self.frame],
            group_digits(progress.dirs.load(Ordering::Relaxed))
        );
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
        if s.failed > 0 {
            text += &self.paint("31", &format!(", {} failed", s.failed));
        }
        if self.dry_run {
            text += &self.dim(" · dry run, nothing written");
        }
        self.line(text);
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
