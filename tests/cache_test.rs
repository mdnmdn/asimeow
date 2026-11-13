use anyhow::Result;
use asimeow::cache::{Cache, Entry, LastRun, VERSION};
use std::fs;
use tempfile::tempdir;

#[test]
fn test_cache_round_trip() -> Result<()> {
    let dir = tempdir()?;
    let path = dir.path().join("nested").join("state.json");
    let mut cache = Cache {
        last_run: Some(LastRun {
            finished_at: 1_700_000_000,
            dirs: 42,
            excluded_bytes: 1_000,
        }),
        ..Cache::default()
    };
    cache.exclusions.insert(
        "/x/node_modules".into(),
        Entry {
            rule: "node".to_string(),
            inode: 7,
            mtime_ns: 1_700_000_000_000_000_000,
            bytes: 1_000,
            sized_at: 1_700_000_000,
        },
    );

    cache.save(&path)?;

    assert_eq!(Cache::load(&path), cache);
    assert!(
        !path.with_extension("json.tmp").exists(),
        "temp file renamed away"
    );
    Ok(())
}

#[test]
fn test_missing_corrupt_or_outdated_cache_is_empty() -> Result<()> {
    let dir = tempdir()?;
    let path = dir.path().join("state.json");
    assert_eq!(Cache::load(&path), Cache::default());

    fs::write(&path, "{ not json")?;
    assert_eq!(Cache::load(&path), Cache::default());

    let old = Cache {
        version: VERSION + 1,
        ..Cache::default()
    };
    old.save(&path)?;
    assert_eq!(Cache::load(&path), Cache::default());
    Ok(())
}
