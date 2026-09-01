// Concern: reports each annotation absent, malformed or over the bound, and each dangling or overfull `.annotation` file | Non-concern: the tree view | IO: (files, Config) -> (report, exit_code)

//! # Strict-check JSON schema (`--strict-check --format json`)
#![doc = concat!("```text\n", include_str!("strict_schema.txt"), "```")]
//!
//! `category` maps `Outcome::Missing` -> `missing_annotation`, `Malformed` ->
//! `malformed_annotation` and `TooLong` -> `annotation_too_long`.

use std::collections::BTreeSet;
use std::path::{Path, PathBuf};

use globset::GlobSet;
use serde::Serialize;

use crate::annotation;
use crate::charter;
use crate::config::Config;
use crate::graph;
use crate::rules;
use crate::sidecar;
use crate::walk::CHARTER_FILE;

/// These name WHAT was checked (no comment marker, so no real language).
const CHARTER_LANGUAGE: &str = "charter";
const SIDECAR_LANGUAGE: &str = "sidecar";

pub const SCHEMA_DOC: &str = include_str!("strict_schema.txt");

/// `Missing.raw` is set even for ordinary code lines; the raw line is exposed via `found` instead
/// of a separate "unrecognized token" label.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum Category {
    MissingAnnotation,
    MalformedAnnotation,
    /// Distinct from `MalformedAnnotation`: "not the shape" vs "the shape, but too wordy".
    AnnotationTooLong,
}

/// Not the only statement of the bound (guide, config, README also point at it) but this owns
/// the DEFINITION — seven copies existed when the bound went per-field to whole-annotation.
pub const LENGTH_RULE: &str = "Fail --strict-check when an annotation is longer than N \
    characters, counted over the whole annotation rather than any one field, with the comment \
    marker excluded. 200 by default; 0 turns the bound off. Overrides \
    `[rules] max_annotation_length`.";

#[derive(Debug, Clone, Copy, Serialize)]
pub struct Expected {
    pub template: &'static str,
    pub required: &'static [&'static str],
    pub recommended: &'static [&'static str],
}

/// `pub(crate)` so the embedded annotation guide renders the SAME template the checker enforces.
pub(crate) const EXPECTED: Expected = Expected {
    template: "Concern: {the file's ONE job, verb-led} | Non-concern: {a concern an agent would expect here but this file does NOT own} | IO: (in) -> out  OR  none",
    required: &[
        crate::annotation::PART_CONCERN,
        crate::annotation::PART_NON_CONCERN,
        crate::annotation::PART_IO,
    ],
    recommended: &[],
};

#[derive(Debug, Clone, Serialize)]
pub struct Defect {
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub missing: Vec<&'static str>,
    /// Present only on `annotation_too_long`; not a per-field measurement.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub length: Option<usize>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub max: Option<usize>,
}

#[derive(Debug, Clone, Serialize)]
pub struct AnnotationViolation {
    pub path: String,
    pub line: usize,
    pub language: String,
    pub category: Category,
    pub marker: String,
    /// A guaranteed-valid concrete instance, distinct from the abstract `expected.template`.
    pub example: String,
    pub defect: Defect,
    pub expected: Expected,
    pub found: Option<String>,
    /// Absent for `annotation_too_long`, where the only text to seed from is the over-length
    /// annotation itself.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub suggestion: Option<String>,
    /// Absent (not null) when the machine fields suffice.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub detail: Option<String>,
}

impl AnnotationViolation {
    pub(crate) fn message(&self) -> String {
        match self.category {
            Category::MissingAnnotation => {
                let mut msg = format!(
                    "{}:{}: missing annotation [{}] — add a `{}` comment.",
                    self.path, self.line, self.language, self.marker,
                );
                if let Some(suggestion) = &self.suggestion {
                    msg.push_str(&format!(" suggestion: {suggestion}"));
                }
                if let Some(found) = &self.found {
                    msg.push_str(&format!(" found: '{found}'"));
                }
                msg
            }
            Category::MalformedAnnotation => {
                // Interpolated right after the language marker, the same slot the too-long message uses, so it reads as the diagnosis rather than as a note on the trailing suggestion.
                let diagnosis = match &self.detail {
                    Some(detail) => format!("{detail}; "),
                    None => String::new(),
                };
                let mut msg = format!(
                    "{}:{}: annotation is malformed [{}] — {}expected '{}'. found: '{}'.",
                    self.path,
                    self.line,
                    self.language,
                    diagnosis,
                    self.expected.template,
                    self.found.as_deref().unwrap_or(""),
                );
                if let Some(suggestion) = &self.suggestion {
                    msg.push_str(&format!(" suggestion: {suggestion}"));
                }
                msg
            }
            Category::AnnotationTooLong => format!(
                "{}:{}: annotation is too long [{}] — {}. found: '{}'",
                self.path,
                self.line,
                self.language,
                self.detail
                    .as_deref()
                    .expect("TooLong always carries a rendered detail"),
                self.found.as_deref().unwrap_or(""),
            ),
        }
    }
}

#[derive(Debug, Clone, Serialize)]
pub struct RuleViolation {
    pub code: rules::RuleCode,
    pub message: String,
    /// `[from, to]` for a denied dependency, the ordered node path for a cycle, the single
    /// package for an orphan / unknown deny.
    pub packages: Vec<String>,
    /// Absent when no single location applies (a cycle, or an absent deny package).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub path: Option<String>,
}

#[derive(Debug, Clone, Serialize)]
pub struct OrphanSidecar {
    pub path: String,
    pub target: String,
    pub message: String,
}

#[derive(Debug, Clone, Serialize)]
pub struct TrailingContent {
    pub path: String,
    /// 1-based line of the first content past line 1 — where to start deleting.
    pub line: usize,
    pub message: String,
}

#[derive(Debug, Clone, Serialize)]
pub struct StrictReport {
    pub passed: bool,
    pub error_count: usize,
    pub files_checked: usize,
    /// Numerator behind the "N of M files annotated" footer.
    pub annotated_count: usize,
    pub violations: Vec<AnnotationViolation>,
    pub orphan_sidecars: Vec<OrphanSidecar>,
    pub trailing_content: Vec<TrailingContent>,
    pub rule_violations: Vec<RuleViolation>,
}

impl StrictReport {
    /// The identity for [`merge`](Self::merge).
    pub fn empty() -> Self {
        StrictReport {
            passed: true,
            error_count: 0,
            files_checked: 0,
            annotated_count: 0,
            violations: Vec::new(),
            orphan_sidecars: Vec::new(),
            trailing_content: Vec::new(),
            rule_violations: Vec::new(),
        }
    }

    pub fn merge(&mut self, other: StrictReport) {
        self.passed = self.passed && other.passed;
        self.error_count += other.error_count;
        self.files_checked += other.files_checked;
        self.annotated_count += other.annotated_count;
        self.violations.extend(other.violations);
        self.orphan_sidecars.extend(other.orphan_sidecars);
        self.trailing_content.extend(other.trailing_content);
        self.rule_violations.extend(other.rule_violations);
    }

    /// `max_per_node` caps each list for humans via the `[+N more …]` idiom; JSON is never capped.
    pub fn to_text(&self, max_per_node: Option<usize>) -> (String, i32) {
        let mut out = String::new();
        push_capped(&mut out, &self.violations, max_per_node, "error", |v| {
            v.message()
        });
        let mut code = crate::exit::SUCCESS;
        if self.violations.is_empty() {
            out.push_str(&format!("All {} files passed\n", self.files_checked));
        } else {
            out.push_str(&format!(
                "\nFound {} error(s) in {} files checked\n",
                self.violations.len(),
                self.files_checked
            ));
            // Once per report; repeating this per violation would drown the findings.
            if let Some(max) = self.violations.iter().find_map(|v| v.defect.max) {
                out.push_str(&format!(
                    "note: the annotation length bound is {max} — change it with `[rules] max_annotation_length = <N>` or `--max-length <N>`, or disable it with `--max-length 0`\n"
                ));
            }
            code = crate::exit::STRICT_FAILURE;
        }
        out.push_str(&format!(
            "{} of {} files annotated\n",
            self.annotated_count, self.files_checked
        ));
        if !self.orphan_sidecars.is_empty() {
            push_capped(
                &mut out,
                &self.orphan_sidecars,
                max_per_node,
                "orphan sidecar",
                |s| format!("{}: {}", s.path, s.message),
            );
            out.push_str(&format!(
                "\nFound {} orphan sidecar(s)\n",
                self.orphan_sidecars.len()
            ));
            code = crate::exit::STRICT_FAILURE;
        }
        if !self.trailing_content.is_empty() {
            push_capped(
                &mut out,
                &self.trailing_content,
                max_per_node,
                "trailing-content finding",
                |t| format!("{}:{}: {}", t.path, t.line, t.message),
            );
            out.push_str(&format!(
                "\nFound {} annotation file(s) with trailing content\n",
                self.trailing_content.len()
            ));
            code = crate::exit::STRICT_FAILURE;
        }
        if !self.rule_violations.is_empty() {
            push_capped(
                &mut out,
                &self.rule_violations,
                max_per_node,
                "rule violation",
                |v| format!("rule: {}", v.message),
            );
            out.push_str(&format!(
                "\nFound {} rule violation(s)\n",
                self.rule_violations.len()
            ));
            code = crate::exit::STRICT_FAILURE;
        }
        (out, code)
    }

    pub fn to_json(&self) -> String {
        serde_json::to_string_pretty(self).expect("strict report serializes to JSON")
    }
}

/// `cap` is never `Some(0)`, so a non-empty list always shows at least one line.
fn push_capped<T>(
    out: &mut String,
    items: &[T],
    cap: Option<usize>,
    noun: &str,
    line: impl Fn(&T) -> String,
) {
    let shown = cap.map_or(items.len(), |c| items.len().min(c));
    for item in &items[..shown] {
        out.push_str(&line(item));
        out.push('\n');
    }
    let hidden = items.len() - shown;
    if hidden > 0 {
        out.push_str(&format!(
            "[+{hidden} more {noun}(s) — full list in --format json]\n"
        ));
    }
}

/// The graph build is skipped entirely when no rule is active.
pub(crate) fn check_structured(
    root: &Path,
    files: &[PathBuf],
    config: &Config,
    excludes: &GlobSet,
) -> StrictReport {
    let (violations, annotated_count, annotated_files) = check_annotations(root, files, config);
    let orphan_sidecars = orphan_sidecars(root, files);
    let trailing_content = trailing_contents(root, files, config);
    let mut rule_violations = Vec::new();
    if config.rules.is_active() {
        // UNCAPPED by depth: `-L` never shrinks what `--strict-check` evaluates.
        let graph = graph::build(
            &[root.to_path_buf()],
            crate::walk::WalkFilter::from_config(config, excludes),
            None,
        );
        let root_canon = root.canonicalize().unwrap_or_else(|_| root.to_path_buf());
        rule_violations = rules::evaluate(&graph.packages, &config.rules)
            .into_iter()
            .map(|v| RuleViolation {
                code: v.code,
                message: v.message,
                packages: v.packages,
                path: v
                    .dir
                    .map(|d| crate::util::to_unix_path(d.strip_prefix(&root_canon).unwrap_or(&d))),
            })
            .collect();
        // Modeled on `forbid_orphans`: a `[rules]` toggle yielding a fatal `RuleViolation`.
        if config.rules.require_package_charter {
            rule_violations.extend(package_charter_violations(
                &graph,
                &root_canon,
                &annotated_files,
                config,
            ));
            rule_violations.sort_by(|a, b| a.message.cmp(&b.message));
        }
    }
    StrictReport {
        passed: violations.is_empty()
            && orphan_sidecars.is_empty()
            && trailing_content.is_empty()
            && rule_violations.is_empty(),
        error_count: violations.len(),
        files_checked: files.len(),
        annotated_count,
        violations,
        orphan_sidecars,
        trailing_content,
        rule_violations,
    }
}

/// A lone file has no package neighbourhood, so directory-scale signals and the sidecar census
/// do not apply and no graph is built.
pub(crate) fn check_file(root: &Path, files: &[PathBuf], config: &Config) -> StrictReport {
    let (violations, annotated_count, _annotated_files) = check_annotations(root, files, config);
    StrictReport {
        passed: violations.is_empty(),
        error_count: violations.len(),
        files_checked: files.len(),
        annotated_count,
        violations,
        orphan_sidecars: Vec::new(),
        trailing_content: Vec::new(),
        rule_violations: Vec::new(),
    }
}

fn check_annotations(
    root: &Path,
    files: &[PathBuf],
    config: &Config,
) -> (Vec<AnnotationViolation>, usize, Vec<String>) {
    let mut violations: Vec<AnnotationViolation> = Vec::new();
    let mut annotated_count = 0usize;
    // `require_package_charter` only fires on a package whose files are actually annotated.
    let mut annotated_files: Vec<String> = Vec::new();
    for path in files {
        let rel = rel_of(root, path);
        let Some(lang) = config.language_for_path(path) else {
            if let Some(body) = sidecar::body(path) {
                annotated_files.push(rel.clone());
                if annotation::content_past_first_line(&body).is_some() {
                    continue;
                }
                match defect_parts(annotation::analyze_charter(
                    &body,
                    config.rules.max_annotation_length,
                )) {
                    Some(parts) => violations.push(bare_violation(
                        rel_of(root, &sidecar::path_for(path)),
                        SIDECAR_LANGUAGE,
                        &file_name(&rel),
                        parts,
                    )),
                    None => annotated_count += 1,
                }
            }
            continue;
        };
        let mk = marker(lang);

        let Some((line, category, defect, found, seed, detail)) = defect_parts(
            annotation::analyze_file(path, lang, config.rules.max_annotation_length),
        ) else {
            annotated_count += 1;
            annotated_files.push(rel);
            continue;
        };

        if !matches!(category, Category::MissingAnnotation) {
            annotated_files.push(rel.clone());
        }
        let seed = seed.as_deref().filter(|s| !s.is_empty());
        let suggestion = (!matches!(category, Category::AnnotationTooLong))
            .then(|| tailored_suggestion(&mk, &rel, seed));
        violations.push(AnnotationViolation {
            path: rel,
            line,
            language: lang.name.clone(),
            category,
            marker: mk,
            example: lang.example(),
            defect,
            expected: EXPECTED,
            suggestion,
            found,
            detail,
        });
    }

    violations.extend(charter_violations(root, files, config));

    violations.sort_by(|a, b| (&a.path, a.line).cmp(&(&b.path, b.line)));
    (violations, annotated_count, annotated_files)
}

/// `line`, [`Category`], `defect`, `found` text, a concern `seed`, and `detail`.
type DefectParts = (
    usize,
    Category,
    Defect,
    Option<String>,
    Option<String>,
    Option<String>,
);

/// `None` for `Ok`. Shared by the per-file lint AND the `.annotation` charter check.
fn defect_parts(outcome: annotation::Outcome) -> Option<DefectParts> {
    use annotation::Outcome;
    match outcome {
        Outcome::Ok => None,
        Outcome::Missing { line, raw } => Some((
            line,
            Category::MissingAnnotation,
            Defect {
                missing: vec![
                    annotation::PART_CONCERN,
                    annotation::PART_NON_CONCERN,
                    annotation::PART_IO,
                ],
                length: None,
                max: None,
            },
            raw.map(|r| r.trim().to_string()),
            None,
            None,
        )),
        Outcome::Malformed {
            line,
            actual,
            missing,
            detail,
        } => {
            let seed = annotation::concern_seed(&actual).to_string();
            Some((
                line,
                Category::MalformedAnnotation,
                Defect {
                    missing,
                    length: None,
                    max: None,
                },
                Some(actual),
                Some(seed),
                detail,
            ))
        }
        Outcome::TooLong {
            line,
            actual,
            length,
            max,
        } => {
            let detail = too_long_detail(length, max);
            Some((
                line,
                Category::AnnotationTooLong,
                Defect {
                    missing: Vec::new(),
                    length: Some(length),
                    max: Some(max),
                },
                Some(actual),
                None,
                Some(detail),
            ))
        }
    }
}

/// Rendered once here so the JSON `detail` and the TEXT message carry byte-identical prose.
fn too_long_detail(length: usize, max: usize) -> String {
    format!(
        "the annotation is {}, over the {max} limit",
        annotation::counted(length, "character")
    )
}

/// Opting in means doing it right: a malformed breadcrumb is fatal, not a silent no-op.
fn charter_violations(root: &Path, files: &[PathBuf], config: &Config) -> Vec<AnnotationViolation> {
    let mut out = Vec::new();
    for dir in tree_dirs(root, files) {
        let Some(content) = charter::read_charter_file(&dir) else {
            continue;
        };
        if annotation::content_past_first_line(&content).is_some() {
            continue;
        }
        let Some(parts) = defect_parts(annotation::analyze_charter(
            &content,
            config.rules.max_annotation_length,
        )) else {
            continue;
        };
        let path = charter_rel(root, &dir);
        let dir_name = dir
            .file_name()
            .map(|n| n.to_string_lossy().into_owned())
            .unwrap_or_default();
        out.push(bare_violation(path, CHARTER_LANGUAGE, &dir_name, parts));
    }
    out
}

/// ONE builder for both a directory charter and a file sidecar, so the two scales cannot drift.
fn bare_violation(
    path: String,
    language: &str,
    subject: &str,
    (line, category, defect, found, seed, detail): DefectParts,
) -> AnnotationViolation {
    let seed = seed.as_deref().filter(|s| !s.is_empty());
    let suggestion = (!matches!(category, Category::AnnotationTooLong)).then(|| {
        found
            .as_deref()
            .and_then(annotation::unwrapped_bare_line)
            .map(|wrapped| wrapped.bare)
            .unwrap_or_else(|| {
                tailored_suggestion("", subject, seed)
                    .trim_start()
                    .to_string()
            })
    });
    AnnotationViolation {
        path,
        line,
        language: language.to_string(),
        category,
        marker: String::new(),
        example: charter::EXAMPLE.to_string(),
        defect,
        expected: EXPECTED,
        suggestion,
        found,
        detail,
    }
}

/// A dangling sidecar is precisely the one no listed file points at.
fn orphan_sidecars(root: &Path, files: &[PathBuf]) -> Vec<OrphanSidecar> {
    let mut out = Vec::new();
    for dir in tree_dirs(root, files) {
        for path in sidecar::candidates_in(&dir) {
            let Some(target) = sidecar::named_target(&path) else {
                continue;
            };
            if target.is_file() {
                continue;
            }
            let name = file_name(&crate::util::to_unix_path(&target));
            out.push(OrphanSidecar {
                path: rel_of(root, &path),
                target: rel_of(root, &target),
                message: format!(
                    "annotates no file — '{name}' does not exist beside it. Remove the sidecar, \
                     or create the file it names"
                ),
            });
        }
    }
    out.sort_by(|a, b| a.path.cmp(&b.path));
    out
}

fn sidecar_trailing_contents(
    root: &Path,
    files: &[PathBuf],
    config: &Config,
) -> Vec<TrailingContent> {
    let mut out = Vec::new();
    for file in files {
        if config.language_for_path(file).is_some() {
            continue;
        }
        let Some(body) = sidecar::body(file) else {
            continue;
        };
        if let Some(line) = annotation::content_past_first_line(&body) {
            out.push(trailing_content(
                rel_of(root, &sidecar::path_for(file)),
                line,
            ));
        }
    }
    out
}

fn trailing_contents(root: &Path, files: &[PathBuf], config: &Config) -> Vec<TrailingContent> {
    let mut out = sidecar_trailing_contents(root, files, config);
    for dir in tree_dirs(root, files) {
        let Some(body) = charter::read_charter_file(&dir) else {
            continue;
        };
        if let Some(line) = annotation::content_past_first_line(&body) {
            out.push(trailing_content(charter_rel(root, &dir), line));
        }
    }
    out.sort_by(|a, b| a.path.cmp(&b.path));
    out
}

fn trailing_content(path: String, line: usize) -> TrailingContent {
    TrailingContent {
        message: "holds more than the one line an `.annotation` file is — a trailing newline and \
                  trailing blank lines are fine, prose is not. Delete everything from this line \
                  down"
            .to_string(),
        path,
        line,
    }
}

fn charter_rel(root: &Path, dir: &Path) -> String {
    let dir_rel = rel_of(root, dir);
    if dir_rel.is_empty() {
        CHARTER_FILE.to_string()
    } else {
        format!("{dir_rel}/{CHARTER_FILE}")
    }
}

fn rel_of(root: &Path, path: &Path) -> String {
    crate::util::to_unix_path(path.strip_prefix(root).unwrap_or(path))
}

fn file_name(rel: &str) -> String {
    rel.rsplit('/').next().unwrap_or(rel).to_string()
}

fn tree_dirs(root: &Path, files: &[PathBuf]) -> BTreeSet<PathBuf> {
    let mut dirs = BTreeSet::new();
    for file in files {
        for ancestor in file.ancestors().skip(1) {
            dirs.insert(ancestor.to_path_buf());
            if ancestor == root {
                break;
            }
        }
    }
    dirs
}

fn rel_dir(dir: &Path, root_canon: &Path) -> String {
    crate::util::to_unix_path(dir.strip_prefix(root_canon).unwrap_or(dir))
}

fn owning_dir<'a>(file_rel: &str, pkg_dirs: &'a [String]) -> Option<&'a str> {
    pkg_dirs
        .iter()
        .map(String::as_str)
        .filter(|dir| dir_contains(dir, file_rel))
        .max_by_key(|dir| dir.len())
}

/// An empty `dir_rel` is the root package, which contains every file under the tree.
fn dir_contains(dir_rel: &str, file_rel: &str) -> bool {
    dir_rel.is_empty()
        || file_rel
            .strip_prefix(dir_rel)
            .is_some_and(|rest| rest.starts_with('/'))
}

fn packages_owning_annotations(
    graph: &graph::Graph,
    root_canon: &Path,
    annotated_files: &[String],
) -> std::collections::HashSet<String> {
    let pkg_dirs: Vec<String> = graph
        .packages
        .iter()
        .map(|p| rel_dir(&p.dir, root_canon))
        .collect();
    annotated_files
        .iter()
        .filter_map(|f| owning_dir(f, &pkg_dirs))
        .map(str::to_string)
        .collect()
}

/// A manifest-bearing package that OWNS an annotated file but resolves NO concern charter.
fn package_charter_violations(
    graph: &graph::Graph,
    root_canon: &Path,
    annotated_files: &[String],
    config: &Config,
) -> Vec<RuleViolation> {
    let owned = packages_owning_annotations(graph, root_canon, annotated_files);
    graph
        .packages
        .iter()
        .filter_map(|pkg| {
            let dir_rel = rel_dir(&pkg.dir, root_canon);
            if !owned.contains(&dir_rel) {
                return None;
            }
            if charter::resolve_from_fs(&pkg.dir, config).is_some() {
                return None;
            }
            Some(RuleViolation {
                code: rules::RuleCode::MissingPackageCharter,
                message: format!(
                    "package '{}' carries annotated files but resolves no concern charter — add a \
                     `.annotation` breadcrumb to its directory, or annotate its code entry file \
                     (src/lib.rs, __init__.py, index.ts, mod.rs, doc.go)",
                    pkg.name
                ),
                packages: vec![pkg.name.clone()],
                path: Some(dir_rel),
            })
        })
        .collect()
}

/// The judgment fields are scaffolded as `<…>` slots — not finished, they're what an agent writes.
fn tailored_suggestion(marker: &str, path: &str, seed: Option<&str>) -> String {
    let concern = match seed {
        Some(s) => s.to_string(),
        None => format!("<what {} does>", file_stem(path)),
    };
    format!(
        "{marker} Concern: {concern} | Non-concern: <a concern an agent would expect here but this file does NOT own> | IO: (<inputs>) -> <outputs>"
    )
}

fn file_stem(path: &str) -> &str {
    let name = path.rsplit('/').next().unwrap_or(path);
    name.rsplit_once('.').map_or(name, |(stem, _)| stem)
}

/// Falls back to `#` for a language with no delimiter configured.
fn marker(lang: &crate::config::Language) -> String {
    lang.line
        .clone()
        .or_else(|| lang.block.as_ref().map(|(open, _)| open.clone()))
        .or_else(|| lang.docstring.first().cloned())
        .unwrap_or_else(|| "#".to_string())
}
