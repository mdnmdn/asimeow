use asimeow::config::Rule;
use asimeow::rules::Matcher;

fn rule(name: &str, file_match: &str) -> Rule {
    Rule {
        name: name.to_string(),
        file_match: file_match.to_string(),
        exclusions: vec![],
    }
}

fn matched(matcher: &Matcher, file_name: &str) -> Option<String> {
    matcher.rule_for(file_name).map(|r| r.name.clone())
}

#[test]
fn test_literal_names_match_case_insensitively() {
    let m = Matcher::new(vec![rule("rust", "Cargo.toml")], &[]);
    assert_eq!(matched(&m, "Cargo.toml").as_deref(), Some("rust"));
    assert_eq!(matched(&m, "cargo.TOML").as_deref(), Some("rust"));
    assert_eq!(matched(&m, "Cargo.lock"), None);
}

#[test]
fn test_glob_patterns_match() {
    let m = Matcher::new(vec![rule("net", "*.csproj")], &[]);
    assert_eq!(matched(&m, "App.csproj").as_deref(), Some("net"));
    assert_eq!(matched(&m, "App.sln"), None);
}

#[test]
fn test_first_rule_in_config_order_wins() {
    let glob_first = Matcher::new(
        vec![rule("any-json", "*.json"), rule("node", "package.json")],
        &[],
    );
    assert_eq!(
        matched(&glob_first, "package.json").as_deref(),
        Some("any-json")
    );

    let literal_first = Matcher::new(
        vec![
            rule("node", "package.json"),
            rule("any-json", "*.json"),
            rule("node-again", "package.json"),
        ],
        &[],
    );
    assert_eq!(
        matched(&literal_first, "package.json").as_deref(),
        Some("node")
    );
}

#[test]
fn test_ignore_patterns_match_directory_names() {
    let m = Matcher::new(vec![], &[".git".to_string(), "cache-*".to_string()]);
    assert!(m.is_ignored(".git"));
    assert!(m.is_ignored("cache-v2"));
    assert!(!m.is_ignored("src"));
}
