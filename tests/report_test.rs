use asimeow::cache::{Cache, Entry, LastRun};
use asimeow::report::{
    format_bytes, group_digits, savings, stats, truncate_left, CompactReporter, LogReporter,
    Reporter,
};
use asimeow::scan::{Event, Kind, Options, Summary};
use std::path::{Path, PathBuf};
use std::time::Duration;

fn options(verbose: bool, dry_run: bool) -> Options {
    Options {
        threads: 1,
        verbose,
        dry_run,
    }
}

fn exclusion(path: &str, kind: Kind) -> Event {
    Event::Exclusion {
        path: PathBuf::from(path),
        rule: "node".to_string(),
        kind,
    }
}

fn unreadable() -> Event {
    Event::Unreadable {
        path: PathBuf::from("/x/.Trash"),
        error: "Operation not permitted".to_string(),
    }
}

fn summary() -> Summary {
    Summary {
        dirs: 21967,
        exclusions: 77,
        newly_excluded: 2,
        recreated: 1,
        failed: 0,
        unreadable: 1,
        excluded_bytes: 9_319_000_000,
        new_bytes: 668_000_000,
        measured: 3,
        previous_bytes: Some(8_710_000_000),
        cancelled: false,
        elapsed: Duration::from_millis(220),
    }
}

/// Feeds events to a log reporter and returns (stdout, stderr).
fn log(opts: &Options, events: &[Event]) -> (String, String) {
    let (mut out, mut err) = (Vec::new(), Vec::new());
    {
        let mut reporter = LogReporter::new(&mut out, &mut err, opts);
        for event in events {
            reporter.event(event);
        }
        reporter.finish(&summary());
    }
    (
        String::from_utf8(out).unwrap(),
        String::from_utf8(err).unwrap(),
    )
}

#[test]
fn test_log_prints_only_changes_by_default() {
    let events = [
        exclusion("/x/app/node_modules", Kind::New),
        exclusion("/x/old/node_modules", Kind::Already),
        unreadable(),
    ];
    let (out, err) = log(&options(false, false), &events);

    assert_eq!(
        out,
        "✅ /x/app/node_modules - node\n\
         summary: folders=21967 exclusions=77 new=2 recreated=1 failed=0 unreadable=1 \
         excluded_bytes=9319000000 new_bytes=668000000 seconds=0.22\n"
    );
    assert_eq!(err, "", "unreadable folders are only counted");
}

#[test]
fn test_log_verbose_lists_everything() {
    let events = [
        exclusion("/x/old/node_modules", Kind::Already),
        unreadable(),
    ];
    let (out, err) = log(&options(true, false), &events);

    assert!(out.starts_with("🟡 /x/old/node_modules - node\n"));
    assert!(err.contains("Could not read /x/.Trash: Operation not permitted"));
}

#[test]
fn test_log_marks_dry_runs() {
    let (out, _) = log(&options(false, true), &[exclusion("/x/a", Kind::New)]);
    assert!(out.starts_with("✅ /x/a - node (dry run)\n"));
    assert!(out.trim_end().ends_with("dry_run=true"));
}

#[test]
fn test_compact_summary_replaces_status_line() {
    let mut out = Vec::new();
    {
        let mut reporter = CompactReporter::new(&mut out, &options(false, false), false);
        reporter.event(&exclusion("/x/app/node_modules", Kind::New));
        reporter.event(&exclusion("/x/old/node_modules", Kind::Already));
        reporter.finish(&summary());
    }
    let out = String::from_utf8(out).unwrap();

    assert_eq!(
        out,
        "✅ /x/app/node_modules  node\n\
         ✓ Done in 0.2s · 21,967 folders · 77 exclusions, 2 new (1 recreated)\n  \
         9.3 GB excluded · +668 MB this run · +609 MB since last run\n  \
         1 folder could not be read; run with -v to list them\n"
    );
}

#[test]
fn test_group_digits() {
    assert_eq!(group_digits(0), "0");
    assert_eq!(group_digits(999), "999");
    assert_eq!(group_digits(1000), "1,000");
    assert_eq!(group_digits(21967), "21,967");
    assert_eq!(group_digits(1234567), "1,234,567");
}

#[test]
fn test_truncate_left_keeps_the_end() {
    assert_eq!(truncate_left("~/a/b", 10), "~/a/b");
    assert_eq!(truncate_left("~/projects/app/src", 8), "…app/src");
    assert_eq!(truncate_left("abc", 0), "");
}

#[test]
fn test_log_marks_recreated_folders() {
    let (out, _) = log(
        &options(false, false),
        &[exclusion("/x/a", Kind::Recreated)],
    );
    assert!(out.starts_with("✅ /x/a - node (recreated)\n"));
}

#[test]
fn test_format_bytes() {
    assert_eq!(format_bytes(0), "0 B");
    assert_eq!(format_bytes(999), "999 B");
    assert_eq!(format_bytes(44_876_000), "44.9 MB");
    assert_eq!(format_bytes(668_000_000), "668 MB");
    assert_eq!(format_bytes(9_319_000_000), "9.3 GB");
}

#[test]
fn test_savings_leaves_out_what_is_unknown() {
    let first_run = Summary {
        excluded_bytes: 2_900_000_000,
        ..Summary::default()
    };
    assert_eq!(savings(&first_run), "2.9 GB excluded");

    let shrunk = Summary {
        excluded_bytes: 2_000_000_000,
        previous_bytes: Some(2_500_000_000),
        ..Summary::default()
    };
    assert_eq!(savings(&shrunk), "2.0 GB excluded · −500 MB since last run");
}

#[test]
fn test_stats_lists_the_largest_folders() {
    let entry = |rule: &str, bytes| Entry {
        rule: rule.to_string(),
        inode: 1,
        mtime_ns: 0,
        bytes,
        sized_at: 0,
    };
    let mut cache = Cache {
        last_run: Some(LastRun {
            finished_at: 1_000,
            dirs: 21_967,
            excluded_bytes: 760_000_000,
        }),
        ..Cache::default()
    };
    cache
        .exclusions
        .insert("/h/app/node_modules".into(), entry("node", 212_000_000));
    cache
        .exclusions
        .insert("/h/tool/target".into(), entry("rust", 548_000_000));

    let text = stats(&cache, 1_000 + 7_200, Some(Path::new("/h")));

    assert_eq!(
        text,
        "760 MB excluded in 2 folders · last scan 2 h ago (21,967 folders scanned)\n\
         \n\
         Largest:\n   \
         548 MB  ~/tool/target  rust\n   \
         212 MB  ~/app/node_modules  node\n"
    );
    assert_eq!(
        stats(&Cache::default(), 0, None),
        "No scan recorded yet. Run asimeow first.\n"
    );
}
