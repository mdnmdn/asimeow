use asimeow::report::{group_digits, truncate_left, CompactReporter, LogReporter, Reporter};
use asimeow::scan::{Event, Kind, Options, Summary};
use std::path::PathBuf;
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
        failed: 0,
        unreadable: 1,
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
         summary: folders=21967 exclusions=77 new=2 failed=0 unreadable=1 seconds=0.22\n"
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
         ✓ Done in 0.2s · 21,967 folders · 77 exclusions, 2 new\n  \
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
