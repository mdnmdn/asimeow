//! What asimeow remembers between runs: every exclusion folder it manages, with the
//! folder's size, and totals of the last run.
//!
//! Stored as JSON in `~/Library/Caches/asimeow/state.json`. Time Machine skips
//! `~/Library/Caches`, and losing the file only costs re-measuring sizes once.

use anyhow::{Context, Result};
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;
use std::fs;
use std::path::{Path, PathBuf};
use std::time::{SystemTime, UNIX_EPOCH};

/// Bump when the format changes; an older file is then ignored.
pub const VERSION: u32 = 1;

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Cache {
    pub version: u32,
    pub last_run: Option<LastRun>,
    pub exclusions: BTreeMap<PathBuf, Entry>,
}

impl Default for Cache {
    fn default() -> Self {
        Cache {
            version: VERSION,
            last_run: None,
            exclusions: BTreeMap::new(),
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct LastRun {
    /// Unix seconds.
    pub finished_at: u64,
    pub dirs: usize,
    pub excluded_bytes: u64,
}

/// One exclusion folder.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Entry {
    pub rule: String,
    /// Identifies the folder itself: it changes when the folder is deleted and recreated.
    pub inode: u64,
    /// The folder's own mtime in Unix nanoseconds. It only changes when entries directly
    /// inside it are added, removed or renamed, not when deeper files change.
    pub mtime_ns: i64,
    pub bytes: u64,
    /// Unix seconds.
    pub sized_at: u64,
}

impl Cache {
    /// `~/Library/Caches/asimeow/state.json`.
    pub fn default_path() -> Option<PathBuf> {
        dirs::cache_dir().map(|dir| dir.join("asimeow").join("state.json"))
    }

    /// Reads the cache; a missing, unreadable or outdated file gives an empty cache.
    pub fn load(path: &Path) -> Cache {
        fs::read(path)
            .ok()
            .and_then(|bytes| serde_json::from_slice::<Cache>(&bytes).ok())
            .filter(|cache| cache.version == VERSION)
            .unwrap_or_default()
    }

    /// Writes the cache atomically (temporary file, then rename).
    pub fn save(&self, path: &Path) -> Result<()> {
        if let Some(dir) = path.parent() {
            fs::create_dir_all(dir)
                .with_context(|| format!("Failed to create {}", dir.display()))?;
        }
        let tmp = path.with_extension("json.tmp");
        fs::write(&tmp, serde_json::to_vec_pretty(self)?)
            .with_context(|| format!("Failed to write {}", tmp.display()))?;
        fs::rename(&tmp, path).with_context(|| format!("Failed to write {}", path.display()))
    }
}

/// Current time in Unix seconds.
pub fn now() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or(0)
}
