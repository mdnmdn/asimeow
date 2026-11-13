use crate::config::Rule;
use glob::Pattern;
use std::collections::HashMap;

/// Rules and ignore patterns, compiled once per run.
///
/// Most rules match a literal file name (`Cargo.toml`, `package.json`), so those are a
/// hash lookup; only real glob patterns (`*.csproj`) are matched one by one.
pub struct Matcher {
    rules: Vec<Rule>,
    literal: HashMap<String, usize>,
    globs: Vec<(Pattern, usize)>,
    ignore: Vec<Pattern>,
}

impl Matcher {
    pub fn new(rules: Vec<Rule>, ignore: &[String]) -> Self {
        let mut literal = HashMap::new();
        let mut globs = Vec::new();
        for (index, rule) in rules.iter().enumerate() {
            let pattern = rule.file_match.to_lowercase();
            if is_literal(&pattern) {
                literal.entry(pattern).or_insert(index);
            } else {
                globs.push((compile(&pattern), index));
            }
        }
        let ignore = ignore.iter().map(|p| compile(p)).collect();
        Matcher {
            rules,
            literal,
            globs,
            ignore,
        }
    }

    pub fn rules(&self) -> &[Rule] {
        &self.rules
    }

    /// The first rule, in config order, whose `file_match` matches `name` (case-insensitive).
    pub fn rule_for(&self, name: &str) -> Option<&Rule> {
        let name = name.to_lowercase();
        let by_literal = self.literal.get(&name).copied();
        let by_glob = self
            .globs
            .iter()
            .find(|(pattern, _)| pattern.matches(&name))
            .map(|(_, index)| *index);
        let index = match (by_literal, by_glob) {
            (Some(a), Some(b)) => a.min(b),
            (a, b) => a.or(b)?,
        };
        Some(&self.rules[index])
    }

    /// True if a directory with this name should be skipped entirely (case-sensitive).
    pub fn is_ignored(&self, dir_name: &str) -> bool {
        self.ignore.iter().any(|p| p.matches(dir_name))
    }
}

fn is_literal(pattern: &str) -> bool {
    !pattern.contains(['*', '?', '['])
}

/// Compiles a glob; an invalid one is warned about once and matched literally.
fn compile(pattern: &str) -> Pattern {
    Pattern::new(pattern).unwrap_or_else(|_| {
        eprintln!("Warning: invalid pattern '{pattern}', using literal match");
        Pattern::new(&Pattern::escape(pattern)).expect("escaped pattern is valid")
    })
}
