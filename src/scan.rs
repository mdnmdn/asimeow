//! The scan engine: walks the roots, matches rules, sets exclusion marks, then measures
//! the excluded folders (reusing sizes from the previous run where they are still fresh).
//!
//! It never prints. Findings go out as [`Event`]s over a channel; fast-changing progress
//! (folders visited, current folder) lives in [`Progress`], which a reporter samples.

use crate::cache::{self, Cache, Entry, LastRun};
use crate::config::{Config, Rule};
use crate::rules::Matcher;
use crate::tm;
use anyhow::Result;
use std::collections::{BTreeMap, HashSet, VecDeque};
use std::fs;
use std::os::unix::fs::MetadataExt;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};
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
    /// Was excluded on an earlier run but lost its mark, usually because the folder was
    /// deleted and recreated (`rm -rf node_modules && npm install`). Marked again.
    Recreated,
    /// Already carried the mark.
    Already,
}

/// A cached size older than this is measured again, even if the folder looks unchanged:
/// a folder's mtime doesn't change when files deeper inside it grow.
pub const SIZE_TTL_SECS: u64 = 7 * 24 * 3600;

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Event {
    /// An existing folder that a rule excludes.
    Exclusion {
        path: PathBuf,
        rule: String,
        kind: Kind,
    },
    /// The size of an exclusion folder, sent after the walk. `measured` is false when the
    /// size came from the cache.
    Sized {
        path: PathBuf,
        bytes: u64,
        measured: bool,
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
    /// Exclusion folders to size, set when the walk is done (0 while walking).
    pub to_size: AtomicUsize,
    pub sized: AtomicUsize,
    /// Set by a reporter to stop the scan early (the user quit).
    pub cancel: AtomicBool,
    current: Mutex<PathBuf>,
}

impl Progress {
    fn cancelled(&self) -> bool {
        self.cancel.load(Ordering::Relaxed)
    }

    /// The folder a worker started on most recently.
    pub fn current(&self) -> PathBuf {
        self.current.lock().unwrap().clone()
    }

    fn set_current(&self, path: &Path) {
        // Skip rather than wait if a reporter is reading it right now.
        if let Ok(mut current) = self.current.try_lock() {
            *current = path.to_path_buf();
        }
    }
}

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Summary {
    pub dirs: usize,
    /// Existing exclusion folders found, new and already marked.
    pub exclusions: usize,
    /// Marked by this run (or would be, in a dry run), recreated folders included.
    pub newly_excluded: usize,
    /// The part of `newly_excluded` that was excluded before and lost its mark.
    pub recreated: usize,
    pub failed: usize,
    pub unreadable: usize,
    /// Total size of all exclusion folders found.
    pub excluded_bytes: u64,
    /// Size of the folders newly excluded by this run.
    pub new_bytes: u64,
    /// Folders measured by this run; the others' sizes came from the cache.
    pub measured: usize,
    /// `excluded_bytes` of the previous run, if there was one.
    pub previous_bytes: Option<u64>,
    /// The scan was stopped early: counts and sizes are incomplete; don't save the cache.
    pub cancelled: bool,
    pub elapsed: Duration,
}

/// Scans all roots of `config`, sending findings to `events`. `previous` is the cache
/// from the last run: it tells recreated folders apart and provides sizes.
///
/// Returns when the whole tree is done (`events` is dropped then, closing the channel),
/// with the summary and the cache to save for the next run. Don't save it after a dry
/// run: it lists folders as excluded that were never marked.
pub fn scan(
    config: Config,
    opts: &Options,
    previous: &Cache,
    events: Sender<Event>,
    progress: &Progress,
) -> Result<(Summary, Cache)> {
    let started = Instant::now();
    let walker = Walker {
        matcher: Matcher::new(config.rules, &config.ignore),
        opts,
        previous,
        events,
        progress,
        found: Mutex::new(Vec::new()),
        exclusions: AtomicUsize::new(0),
        newly_excluded: AtomicUsize::new(0),
        recreated: AtomicUsize::new(0),
        failed: AtomicUsize::new(0),
        unreadable: AtomicUsize::new(0),
        measured: AtomicUsize::new(0),
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

    // Walk: find exclusion folders and mark them.
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

    // Size every exclusion folder found, in parallel.
    let found = std::mem::take(&mut *walker.found.lock().unwrap());
    progress.to_size.store(found.len(), Ordering::Relaxed);
    let now = cache::now();
    let next = AtomicUsize::new(0);
    let sized = Mutex::new(BTreeMap::new());
    thread::scope(|scope| {
        for _ in 0..opts.threads.max(1) {
            scope.spawn(|| {
                while let Some(item) = found.get(next.fetch_add(1, Ordering::Relaxed)) {
                    if progress.cancelled() {
                        break;
                    }
                    if let Some(entry) = walker.size(item, now) {
                        sized.lock().unwrap().insert(item.path.clone(), entry);
                    }
                    progress.sized.fetch_add(1, Ordering::Relaxed);
                }
            });
        }
    });
    let sized = sized.into_inner().unwrap();

    let bytes_of = |path: &PathBuf| sized.get(path).map_or(0, |e| e.bytes);
    let excluded_bytes = found.iter().map(|f| bytes_of(&f.path)).sum();
    let new_bytes = found
        .iter()
        .filter(|f| f.kind != Kind::Already)
        .map(|f| bytes_of(&f.path))
        .sum();
    let dirs = progress.dirs.load(Ordering::Relaxed);

    let summary = Summary {
        dirs,
        exclusions: walker.exclusions.load(Ordering::Relaxed),
        newly_excluded: walker.newly_excluded.load(Ordering::Relaxed),
        recreated: walker.recreated.load(Ordering::Relaxed),
        failed: walker.failed.load(Ordering::Relaxed),
        unreadable: walker.unreadable.load(Ordering::Relaxed),
        excluded_bytes,
        new_bytes,
        measured: walker.measured.load(Ordering::Relaxed),
        previous_bytes: previous.last_run.as_ref().map(|r| r.excluded_bytes),
        cancelled: progress.cancelled(),
        elapsed: started.elapsed(),
    };
    let next_cache = Cache {
        last_run: Some(LastRun {
            finished_at: cache::now(),
            dirs,
            excluded_bytes,
        }),
        exclusions: sized,
        ..Cache::default()
    };
    Ok((summary, next_cache))
}

/// An exclusion folder found by the walk.
struct Found {
    path: PathBuf,
    rule: String,
    kind: Kind,
}

/// Total size of the files under `root`: logical sizes (what a backup would copy), each
/// hard-linked file counted once, symlinks not followed.
pub fn folder_size(root: &Path) -> u64 {
    let mut total = 0;
    let mut linked = HashSet::new();
    let mut stack = vec![root.to_path_buf()];
    while let Some(dir) = stack.pop() {
        let Ok(entries) = fs::read_dir(&dir) else {
            continue;
        };
        for entry in entries.flatten() {
            // DirEntry::metadata does not follow symlinks.
            let Ok(meta) = entry.metadata() else {
                continue;
            };
            if meta.is_dir() {
                stack.push(entry.path());
            } else if meta.is_file()
                && (meta.nlink() < 2 || linked.insert((meta.dev(), meta.ino())))
            {
                total += meta.len();
            }
        }
    }
    total
}

/// Everything the workers share during one scan.
struct Walker<'a> {
    matcher: Matcher,
    opts: &'a Options,
    previous: &'a Cache,
    events: Sender<Event>,
    progress: &'a Progress,
    found: Mutex<Vec<Found>>,
    exclusions: AtomicUsize,
    newly_excluded: AtomicUsize,
    recreated: AtomicUsize,
    failed: AtomicUsize,
    unreadable: AtomicUsize,
    measured: AtomicUsize,
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
        // Once cancelled, every queued folder finishes at once and the walk drains.
        if self.progress.cancelled() {
            return Vec::new();
        }
        if let Some(name) = path.file_name() {
            if self.matcher.is_ignored(&name.to_string_lossy()) {
                self.trace(|| format!("Skipping ignored directory: {}", path.display()));
                return Vec::new();
            }
        }

        self.progress.dirs.fetch_add(1, Ordering::Relaxed);
        self.progress.set_current(path);
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
                if self.previous.exclusions.contains_key(&path) {
                    self.recreated.fetch_add(1, Ordering::Relaxed);
                    Kind::Recreated
                } else {
                    Kind::New
                }
            } else {
                self.failed.fetch_add(1, Ordering::Relaxed);
                self.send(Event::Failed {
                    path,
                    rule: rule.name.clone(),
                });
                continue;
            };
            self.send(Event::Exclusion {
                path: path.clone(),
                rule: rule.name.clone(),
                kind,
            });
            self.found.lock().unwrap().push(Found {
                path,
                rule: rule.name.clone(),
                kind,
            });
        }
    }

    /// The folder's size: from the cache if the folder is the same one (same inode and
    /// mtime) and the size is younger than [`SIZE_TTL_SECS`], measured otherwise.
    fn size(&self, found: &Found, now: u64) -> Option<Entry> {
        let meta = fs::metadata(&found.path).ok()?;
        let (inode, mtime_ns) = (meta.ino(), meta.mtime() * 1_000_000_000 + meta.mtime_nsec());
        let fresh = self.previous.exclusions.get(&found.path).filter(|e| {
            e.inode == inode
                && e.mtime_ns == mtime_ns
                && now.saturating_sub(e.sized_at) < SIZE_TTL_SECS
        });
        let (bytes, sized_at, measured) = match fresh {
            Some(entry) => (entry.bytes, entry.sized_at, false),
            None => {
                self.progress.set_current(&found.path);
                self.measured.fetch_add(1, Ordering::Relaxed);
                (folder_size(&found.path), now, true)
            }
        };
        self.send(Event::Sized {
            path: found.path.clone(),
            bytes,
            measured,
        });
        Some(Entry {
            rule: found.rule.clone(),
            inode,
            mtime_ns,
            bytes,
            sized_at,
        })
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
