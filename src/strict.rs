// Strict: Lint mode — validates every code file's annotation against its language convention and reports offenders as `path:1: message`. NOT concerned with the tree view. | I/O: (files, Config) -> (report, exit_code)

use std::path::{Path, PathBuf};

use globset::GlobSet;

use crate::annotation;
use crate::config::Config;
use crate::graph;
use crate::rules::{self, Violation};

/// The whole `--strict-check` verdict for one root: annotation linting AND (when the
/// root's config configures any `[rules]`) architectural dependency rules, folded into
/// ONE report + exit code. This is the single composition both surfaces drive — the
/// CLI's strict path and the MCP `strict_check` tool — so a rule violation reports and
/// exits identically whichever asks. Building the graph is skipped entirely when no
/// rule is active (a repo with no `[rules]` does zero extra work, byte-identical output).
pub(crate) fn check_with_rules(
    root: &Path,
    files: &[PathBuf],
    config: &Config,
    excludes: &GlobSet,
) -> (String, i32) {
    let (mut report, mut code) = check(root, files, config);
    if config.rules.is_active() {
        // Same filter as the file walk: the rules graph sees exactly the manifests the
        // tree would show (gitignore/hidden/`tests`/`-I` honored).
        let graph = graph::build(
            &[root.to_path_buf()],
            config.display.gitignore,
            config.display.include_tests,
            excludes,
        );
        let violations = rules::evaluate(&graph.packages, &config.rules);
        let (rule_report, rule_code) = report_violations(&violations);
        report.push_str(&rule_report);
        code = code.max(rule_code);
    }
    (report, code)
}

/// Validate `files` (already collected under `root`) and produce a report plus an
/// exit code: 0 = all pass, 1 = at least one failure.
pub fn check(root: &Path, files: &[PathBuf], config: &Config) -> (String, i32) {
    let mut errors: Vec<(String, String)> = Vec::new();

    for path in files {
        let Some(lang) = config.language_for_path(path) else {
            continue;
        };
        let annotation = annotation::extract(path, lang);
        if let Some(message) = annotation::validate(annotation.as_deref(), lang) {
            let rel = path.strip_prefix(root).unwrap_or(path);
            errors.push((crate::util::to_unix_path(rel), message));
        }
    }

    errors.sort();

    let mut out = String::new();
    for (path, message) in &errors {
        out.push_str(&format!("{path}:1: {message}\n"));
    }
    if errors.is_empty() {
        out.push_str(&format!("All {} files passed\n", files.len()));
        (out, 0)
    } else {
        out.push_str(&format!(
            "\nFound {} error(s) in {} files checked\n",
            errors.len(),
            files.len()
        ));
        (out, 1)
    }
}

/// Render architectural rule findings into the same `--strict-check` report: one
/// `rule: <message>` line each, exit 1 when any exist. The observable contract is
/// preserved — line-per-finding to stdout, nonzero exit — rule violations are just
/// another class of finding alongside annotation errors.
pub fn report_violations(violations: &[Violation]) -> (String, i32) {
    if violations.is_empty() {
        return (String::new(), 0);
    }
    let mut out = String::new();
    for v in violations {
        out.push_str(&format!("rule: {}\n", v.message));
    }
    out.push_str(&format!("\nFound {} rule violation(s)\n", violations.len()));
    (out, 1)
}
