//! The scan engine: walks the roots, matches rules and sets exclusion marks.
//!
//! It never prints. Findings go out as [`Event`]s over a channel; fast-changing progress
//! (folders visited, current folder) lives in [`Progress`], which a reporter samples.

use crate::config::{Config, Rule};
use crate::rules::Matcher;
use crate::tm;
use anyhow::Result;
use std::collections::{HashSet, VecDeque};
use std::fs;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::mpsc::Sender;
use std::sync::{Condvar, Mutex};
use std::thread;
use std::time::{Duration, Instant};

#[derive(Debug, Clone, Default)]
pub struct Options {
    pub threads: usize,
    /// Also send [`Event::Trace`] details.
    pub verbose: bool,
    /// Report new exclusions without writing them.
    pub dry_run: bool,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Kind {
    /// Marked by this run (or would be, in a dry run).
    New,
    /// Already carried the mark.
    Already,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Event {
    /// An existing folder that a rule excludes.
    Exclusion {
        path: PathBuf,
        rule: String,
        kind: Kind,
    },
    /// Setting the mark failed.
    Failed { path: PathBuf, rule: String },
    /// A folder that could not be read, often a privacy-protected one like `~/.Trash`.
    Unreadable { path: PathBuf, error: String },
    /// Something to always show, like a root that doesn't exist.
    Warning(String),
    /// Detail that is only sent with `verbose`.
    Trace(String),
}

/// Live progress, updated by the workers and sampled by a reporter.
#[derive(Default)]
pub struct Progress {
    pub dirs: AtomicUsize,
    current: Mutex<PathBuf>,
}

impl Progress {
    /// The folder a worker started on most recently.
    pub fn current(&self) -> PathBuf {
        self.current.lock().unwrap().clone()
    }
}

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Summary {
    pub dirs: usize,
    /// Existing exclusion folders found, new and already marked.
    pub exclusions: usize,
    pub newly_excluded: usize,
    pub failed: usize,
    pub unreadable: usize,
    pub elapsed: Duration,
}

/// Scans all roots of `config`, sending findings to `events`.
/// Returns when the whole tree is done; `events` is dropped then, closing the channel.
pub fn scan(
    config: Config,
    opts: &Options,
    events: Sender<Event>,
    progress: &Progress,
) -> Result<Summary> {
    let started = Instant::now();
    let walker = Walker {
        matcher: Matcher::new(config.rules, &config.ignore),
        opts,
        events,
        progress,
        exclusions: AtomicUsize::new(0),
        newly_excluded: AtomicUsize::new(0),
        failed: AtomicUsize::new(0),
        unreadable: AtomicUsize::new(0),
    };

    let mut roots = Vec::new();
    for root in &config.roots {
        let path = crate::config::expand_tilde(&root.path)?;
        if path.is_dir() {
            roots.push(path);
        } else {
            walker.send(Event::Warning(format!(
                "Root is not a directory: {}",
                path.display()
            )));
        }
    }

    let queue = Queue::new(roots);
    thread::scope(|scope| {
        for _ in 0..opts.threads.max(1) {
            scope.spawn(|| {
                while let Some(dir) = queue.pop() {
                    queue.finish(walker.process_dir(&dir));
                }
            });
        }
    });

    Ok(Summary {
        dirs: progress.dirs.load(Ordering::Relaxed),
        exclusions: walker.exclusions.into_inner(),
        newly_excluded: walker.newly_excluded.into_inner(),
        failed: walker.failed.into_inner(),
        unreadable: walker.unreadable.into_inner(),
        elapsed: started.elapsed(),
    })
}

/// Everything the workers share during one scan.
struct Walker<'a> {
    matcher: Matcher,
    opts: &'a Options,
    events: Sender<Event>,
    progress: &'a Progress,
    exclusions: AtomicUsize,
    newly_excluded: AtomicUsize,
    failed: AtomicUsize,
    unreadable: AtomicUsize,
}

impl Walker<'_> {
    fn send(&self, event: Event) {
        // The receiver only goes away if the reporter stopped; nothing left to tell then.
        let _ = self.events.send(event);
    }

    fn trace(&self, message: impl FnOnce() -> String) {
        if self.opts.verbose {
            self.send(Event::Trace(message()));
        }
    }

    /// Processes one directory: matches its entries against the rules, excludes what they
    /// name, and returns the subdirectories to walk next. Symlinks are never followed.
    fn process_dir(&self, path: &Path) -> Vec<PathBuf> {
        if let Some(name) = path.file_name() {
            if self.matcher.is_ignored(&name.to_string_lossy()) {
                self.trace(|| format!("Skipping ignored directory: {}", path.display()));
                return Vec::new();
            }
        }

        self.progress.dirs.fetch_add(1, Ordering::Relaxed);
        if let Ok(mut current) = self.progress.current.try_lock() {
            *current = path.to_path_buf();
        }
        self.trace(|| format!("Processing path: {}", path.display()));

        let entries = match fs::read_dir(path) {
            Ok(entries) => entries,
            Err(e) => {
                self.unreadable.fetch_add(1, Ordering::Relaxed);
                self.send(Event::Unreadable {
                    path: path.to_path_buf(),
                    error: e.to_string(),
                });
                return Vec::new();
            }
        };

        // Phase 1: match every entry against the rules and collect the subdirectories.
        let mut matched: Vec<&Rule> = Vec::new();
        let mut subdirs: Vec<(String, PathBuf)> = Vec::new();
        for entry in entries.flatten() {
            let name = entry.file_name().to_string_lossy().into_owned();
            if let Some(rule) = self.matcher.rule_for(&name) {
                self.trace(|| {
                    format!(
                        "Found match for rule '{}' at: {}",
                        rule.name,
                        entry.path().display()
                    )
                });
                matched.push(rule);
            }
            // file_type() comes from readdir and does not follow symlinks.
            if entry.file_type().is_ok_and(|t| t.is_dir()) {
                subdirs.push((name, entry.path()));
            }
        }

        // Phase 2: exclude what the rules name; never descend into excluded folders.
        let mut excluded_names: HashSet<String> = HashSet::new();
        for rule in matched {
            self.process_exclusion(path, rule, &mut excluded_names);
            // A rule that excludes "." or ".." stops the walk below this directory.
            if rule.exclusions.iter().any(|e| e == "." || e == "..") {
                return Vec::new();
            }
        }

        subdirs
            .into_iter()
            .filter(|(name, _)| !excluded_names.contains(name))
            .map(|(_, path)| path)
            .collect()
    }

    /// Excludes each existing exclusion folder of `rule` inside `dir`, skipping names
    /// already handled for this directory (two rules can list the same folder).
    fn process_exclusion(&self, dir: &Path, rule: &Rule, seen: &mut HashSet<String>) {
        for exclusion in &rule.exclusions {
            if !seen.insert(exclusion.clone()) {
                continue;
            }
            let path = dir.join(exclusion);
            if !path.exists() {
                continue;
            }
            self.exclusions.fetch_add(1, Ordering::Relaxed);

            let kind = if tm::has_exclusion_mark(&path) {
                Kind::Already
            } else if self.opts.dry_run || tm::set_exclusion_mark(&path, true) {
                self.newly_excluded.fetch_add(1, Ordering::Relaxed);
                Kind::New
            } else {
                self.failed.fetch_add(1, Ordering::Relaxed);
                self.send(Event::Failed {
                    path,
                    rule: rule.name.clone(),
                });
                continue;
            };
            self.send(Event::Exclusion {
                path,
                rule: rule.name.clone(),
                kind,
            });
        }
    }
}

/// Work queue for the walk. `pending` counts directories that are queued or being
/// processed, and both live under one lock, so "no work left" can't be observed while
/// another worker is between taking a directory and queueing its children.
struct Queue {
    inner: Mutex<(VecDeque<PathBuf>, usize)>,
    changed: Condvar,
}

impl Queue {
    fn new(roots: Vec<PathBuf>) -> Self {
        let pending = roots.len();
        Queue {
            inner: Mutex::new((roots.into(), pending)),
            changed: Condvar::new(),
        }
    }

    /// Next directory to process, or None once the whole tree is done.
    fn pop(&self) -> Option<PathBuf> {
        let mut inner = self.inner.lock().unwrap();
        loop {
            if let Some(path) = inner.0.pop_front() {
                return Some(path);
            }
            if inner.1 == 0 {
                return None;
            }
            inner = self.changed.wait(inner).unwrap();
        }
    }

    /// Marks one directory as done and queues its subdirectories.
    fn finish(&self, children: Vec<PathBuf>) {
        let mut inner = self.inner.lock().unwrap();
        inner.1 = inner.1 + children.len() - 1;
        inner.0.extend(children);
        self.changed.notify_all();
    }
}
