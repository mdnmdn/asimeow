use asimeow::cache::LastRun;
use asimeow::scan::{Event, Kind, Options, Summary};
use asimeow::tui::{App, Phase};
use ratatui::backend::TestBackend;
use ratatui::crossterm::event::{KeyCode, KeyEvent};
use ratatui::Terminal;
use std::path::PathBuf;
use std::time::Duration;

fn options(dry_run: bool) -> Options {
    Options {
        threads: 4,
        verbose: false,
        dry_run,
    }
}

fn excluded(path: &str, rule: &str, kind: Kind, bytes: u64) -> [Event; 2] {
    [
        Event::Exclusion {
            path: PathBuf::from(path),
            rule: rule.to_string(),
            kind,
        },
        Event::Sized {
            path: PathBuf::from(path),
            bytes,
            measured: true,
        },
    ]
}

/// A finished scan: two areas, one new, one recreated and two already excluded folders.
fn finished_app() -> App {
    let previous = LastRun {
        finished_at: 0,
        dirs: 21_000,
        excluded_bytes: 3_000_000_000,
    };
    let mut app = App::new(&options(false), Some(previous), 3, false, Some("/h".into()));
    for event in [
        excluded(
            "/h/projects/tool/target",
            "rust",
            Kind::Already,
            2_140_000_000,
        ),
        excluded(
            "/h/projects/site/node_modules",
            "node",
            Kind::Recreated,
            612_000_000,
        ),
        excluded("/h/projects/notes/.venv", "python", Kind::New, 44_000_000),
        excluded(
            "/h/archive/thesis/.venv",
            "python",
            Kind::Already,
            1_210_000_000,
        ),
    ]
    .iter()
    .flatten()
    {
        app.on_event(event);
    }
    app.on_finish(&Summary {
        dirs: 21_967,
        exclusions: 4,
        newly_excluded: 2,
        recreated: 1,
        excluded_bytes: 4_006_000_000,
        new_bytes: 656_000_000,
        previous_bytes: Some(3_000_000_000),
        elapsed: Duration::from_millis(420),
        ..Summary::default()
    });
    app
}

/// Renders the app into an in-memory terminal and returns the screen as text.
fn screen(app: &mut App) -> String {
    let (width, height) = (90, 28);
    let mut terminal = Terminal::new(TestBackend::new(width, height)).unwrap();
    terminal.draw(|frame| app.render(frame)).unwrap();
    let buffer = terminal.backend().buffer();
    (0..height)
        .map(|y| {
            let line: String = (0..width).map(|x| buffer[(x, y)].symbol()).collect();
            line.trim_end().to_string()
        })
        .collect::<Vec<_>>()
        .join("\n")
}

fn press(app: &mut App, code: KeyCode) {
    app.on_key(KeyEvent::from(code));
}

#[test]
fn test_screen_shows_progress_savings_tree_and_activity() {
    let mut app = finished_app();
    let text = screen(&mut app);
    println!("{text}");

    assert_eq!(app.phase(), Phase::Done);
    assert!(text.contains("done in 0.4s"));
    assert!(text.contains("21,967 folders · 4 exclusions, 2 new (1 recreated)"));
    // Savings: the total, this run, and the change against the last run
    assert!(text.contains("Excluded     4.0 GB    in 4 folders"));
    assert!(text.contains("1 new · 1 recreated"));
    assert!(text.contains("+656 MB"));
    assert!(text.contains("+1.0 GB since last run"));
    // Tree: areas sorted by size, folders with rule, size and what changed
    let projects = text.find("▾ ~/projects").expect("projects group");
    let archive = text.find("▾ ~/archive").expect("archive group");
    assert!(projects < archive, "the bigger area comes first");
    assert!(text.contains("site/node_modules"));
    assert!(text.contains("612 MB  recreated"));
    assert!(text.contains("44.0 MB  new"));
    // Activity
    assert!(text.contains("+ ~/projects/notes/.venv was excluded (python)"));
    assert!(text.contains("✓ Done: 2 newly excluded, 0 failed"));
}

#[test]
fn test_groups_fold_and_unfold() {
    let mut app = finished_app();
    assert!(screen(&mut app).contains("tool/target"));

    press(&mut app, KeyCode::Left); // first row is the ~/projects group
    let folded = screen(&mut app);
    assert!(folded.contains("▸ ~/projects"));
    assert!(!folded.contains("tool/target"));

    press(&mut app, KeyCode::Right);
    assert!(screen(&mut app).contains("tool/target"));
}

#[test]
fn test_only_changes_filter() {
    let mut app = finished_app();
    press(&mut app, KeyCode::Char('n'));
    let text = screen(&mut app);

    assert!(text.contains("site/node_modules"));
    assert!(text.contains("notes/.venv"));
    assert!(!text.contains("tool/target"), "already excluded: hidden");
    assert!(
        !text.contains("~/archive"),
        "a group with no changes disappears"
    );
}

#[test]
fn test_quit_keys() {
    let mut app = finished_app();
    assert!(!app.quit_requested());
    press(&mut app, KeyCode::Char('q'));
    assert!(app.quit_requested());
}

#[test]
fn test_first_scan_without_cache() {
    let mut app = App::new(&options(true), None, 0, false, None);
    let text = screen(&mut app);

    assert!(text.contains("scanning"));
    assert!(text.contains("first scan: no estimate yet"));
    assert!(text.contains("Waiting for the first excluded folder…"));
    assert!(text.contains("No cache yet: every folder gets measured"));
    assert!(text.contains("dry run: nothing is written"));
}
