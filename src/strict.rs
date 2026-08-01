// Concern: reports each annotation absent, malformed or over the bound, and each dangling or overfull `.annotation` file | Non-concern: the tree view | IO: (files, Config) -> (report, exit_code)

//! # Strict-check JSON schema (`--strict-check --format json`)
//!
//! The structured verdict is a machine-consumable contract (the counterpart to the
//! default human TEXT report), so its shape is documented here — mirroring the schema
//! note in `render/json.rs`. It serializes the SAME [`StrictReport`] the TEXT report
//! renders, so the two can never disagree on a verdict. The
//! exact same text is exposed at runtime via `--schema` and defined ONCE in
//! [`SCHEMA_DOC`] (an embedded file), so this rustdoc and the `--schema` output can never
//! drift apart:
//!
#![doc = concat!("```text\n", include_str!("strict_schema.txt"), "```")]
//!
//! `category` maps `Outcome::Missing` -> `missing_annotation`, `Outcome::Malformed` ->
//! `malformed_annotation` (a keyed field is absent or empty, or the ` | ` structure is
//! broken), and `Outcome::TooLong` -> `annotation_too_long` (the annotation is longer than the
//! configured bound). `found` carries the raw landing line even for ordinary code (so no
//! misleading "unrecognized token" category is needed). Every category FAILS the check.

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

/// The `language` a bare `.annotation` body is reported under, by scale: a directory's charter
/// or a file's sidecar. Neither has a comment marker, so neither has a real language — these
/// name WHAT was checked, and an agent branches on them the way it branches on `python`.
const CHARTER_LANGUAGE: &str = "charter";
const SIDECAR_LANGUAGE: &str = "sidecar";

/// The human-readable strict-check report schema as text — the SAME string embedded in
/// this module's rustdoc above. The `--schema` flag prints it alongside the map schema so
/// an agent can fetch the whole wire contract; sourcing both surfaces from this one
/// embedded file keeps the advertised schema from drifting.
pub const SCHEMA_DOC: &str = include_str!("strict_schema.txt");

/// Which class of annotation failure a [`AnnotationViolation`] records. Serialized as
/// the snake_case tag consumers branch on. `Missing.raw` is set even for ordinary code
/// lines, so a separate "unrecognized token" label would mislabel — the raw line is
/// exposed via `found` instead.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum Category {
    /// No conforming annotation at all (`Outcome::Missing`).
    MissingAnnotation,
    /// A comment is present but does not carry three non-empty `Concern: … | Non-concern: …
    /// | IO: …` fields — a keyed field is absent, a field is empty after trimming, or the
    /// ` | ` structure is broken (`Outcome::Malformed`).
    MalformedAnnotation,
    /// All three fields are present and non-empty, but the annotation as a whole is longer
    /// than the configured `[rules] max_annotation_length` / `--max-length` bound
    /// (`Outcome::TooLong`). Distinct from `MalformedAnnotation` so an agent can tell "not
    /// the shape" from "the shape, but too wordy to ingest".
    AnnotationTooLong,
}

/// What `--max-length` / `[rules] max_annotation_length` measures. The flag's `--help` renders
/// this rather than restating it.
///
/// It is not the only statement of the bound, and claiming otherwise here was itself wrong: the
/// `note:` line carries the LIVE value (correct by construction), and the guide, config and
/// README point at the rule for their own readers. What this owns is the DEFINITION. Seven
/// copies existed when the bound changed from per-field to whole-annotation; five went stale,
/// including the user-facing help — the same no-drift reason [`EXPECTED`] feeds the guide.
pub const LENGTH_RULE: &str = "Fail --strict-check when an annotation is longer than N \
    characters, counted over the whole annotation rather than any one field, with the comment \
    marker excluded. 200 by default; 0 turns the bound off. Overrides \
    `[rules] max_annotation_length`.";

/// The canonical annotation shape an agent should converge on — the fill-in `template`
/// plus which named parts are ENFORCED (`required`) vs ADVISED (`recommended`). Every
/// violation carries this so an agent reads the contract off the finding instead of
/// reverse-engineering it. All three fields are `required`; `recommended` is empty (the
/// old advisory boundary is now a required field). Part tokens come from
/// [`crate::annotation`], the checker that produces them, so the contract and the delta
/// name the SAME parts and cannot drift.
#[derive(Debug, Clone, Copy, Serialize)]
pub struct Expected {
    pub template: &'static str,
    pub required: &'static [&'static str],
    pub recommended: &'static [&'static str],
}

/// The one enforced contract, identical for every finding (declared once so it can't drift
/// from the checker). All three fields are required and must be non-empty. `pub(crate)` so
/// the embedded annotation guide ([`crate::guide`]) renders the SAME template the checker
/// enforces.
pub(crate) const EXPECTED: Expected = Expected {
    template: "Concern: {what it does} | Non-concern: {what it isn't} | IO: (in) -> out  OR  none",
    required: &[
        crate::annotation::PART_CONCERN,
        crate::annotation::PART_NON_CONCERN,
        crate::annotation::PART_IO,
    ],
    recommended: &[],
};

/// The machine-coded delta between the required shape and what `found` carries: which named
/// parts are ABSENT-OR-EMPTY (`missing`), and how long the annotation is when it exceeds the
/// configured bound (`length`, with the bound itself in `max`). An agent branches on the stable
/// part tokens (`concern` | `non_concern` | `io`) and on these numbers, never on `message` prose.
/// The list is omitted when empty and each number when absent, per the schema's absent-key
/// convention.
#[derive(Debug, Clone, Serialize)]
pub struct Defect {
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub missing: Vec<&'static str>,
    /// The annotation's length in characters — present only on `annotation_too_long`, where the
    /// annotation as a whole breached the bound. It is not a per-field measurement: the bound is
    /// on the contract an agent ingests, with the comment marker excluded.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub length: Option<usize>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub max: Option<usize>,
}

/// One structured annotation violation. The default TEXT report is one rendering over
/// these ([`AnnotationViolation::message`]); `--format json` serializes them.
#[derive(Debug, Clone, Serialize)]
pub struct AnnotationViolation {
    /// Path relative to the checked root, unix slashes.
    pub path: String,
    /// 1-based line the scan landed on (past a shebang / blank lines).
    pub line: usize,
    /// Resolved language name (e.g. `python`).
    pub language: String,
    /// Which class of failure (`missing_annotation` | `malformed_annotation` |
    /// `annotation_too_long`).
    pub category: Category,
    /// The comment delimiter this language expects the annotation to open with.
    pub marker: String,
    /// A canonical, self-conforming annotation line for this language (the config's
    /// per-language exemplar) — a guaranteed-valid concrete instance, distinct from the
    /// abstract `expected.template` and the file-tailored `suggestion`.
    pub example: String,
    /// The machine-coded delta — which template parts are absent/empty, and how long the
    /// annotation is when it is over the length bound. An agent acts on this, not on `message`.
    pub defect: Defect,
    /// The canonical annotation contract (template + required/recommended parts).
    pub expected: Expected,
    /// The offending line — the raw landing line (missing) or the extracted annotation
    /// (malformed / too long) — or `None` for an empty / unreadable head.
    pub found: Option<String>,
    /// A FILE-TAILORED candidate to adapt: whatever descriptive text the file already
    /// carries (or its stem) seeds the `Concern:` field, with the judgment fields scaffolded
    /// as `<…>` placeholder slots (`<concern owned elsewhere>`, `(<inputs>) -> <outputs>`).
    /// The check is form-only, so the stub itself is well-formed and applying it does not
    /// stack a second failure — but a configured length bound still applies to it, and the
    /// `<…>` slots are unfilled judgments an agent has to write out. Absent for
    /// `annotation_too_long`: the only text available to seed a stub from is the over-length
    /// annotation itself, so a stub would restate the defect.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub suggestion: Option<String>,
    /// Human prose for the cases where the machine fields alone would mislead: which fields are
    /// present but empty, that the ` | ` structure is broken, or how far the annotation is over
    /// the bound. Absent (not null) when the machine fields suffice, per the schema's
    /// key-presence convention.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub detail: Option<String>,
}

impl AnnotationViolation {
    /// The one human message line for this violation, keeping the machine-parseable
    /// `path:line:` prefix. The TEXT report is exactly these, one per line, shared by the CLI
    /// report and any renderer. The per-category FRAME lives here; the `detail` clause it
    /// interpolates is authored where the defect is diagnosed ([`annotation`]'s
    /// broken-structure and empty-field prose, and [`too_long_detail`]) — so a message is one
    /// frame plus one carried clause, never two competing renderings.
    fn message(&self) -> String {
        match self.category {
            Category::MissingAnnotation => {
                // Name the language and the exact marker to add, show a conformant
                // example, and — when a foreign/wrong-marker line was present — echo
                // it so the fix is unambiguous (e.g. "you used `;` not `--`").
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
                // `detail` is interpolated right after the language marker (the same slot the
                // too-long message uses), so it reads as the diagnosis rather than as a
                // comment on the trailing suggestion. A plainly-absent key carries no
                // `detail`, and then this renders exactly as it always has.
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

/// One architectural `[rules]` finding, its own shape (distinct from annotation
/// violations), so consumers can tell a dependency-rule breach from a missing comment.
/// Carries a stable dispatch [`code`](rules::RuleCode) and located facts — the same
/// located-diagnostic contract as [`AnnotationViolation`], so an agent acts on
/// structure and only humans read `message`.
#[derive(Debug, Clone, Serialize)]
pub struct RuleViolation {
    /// Stable dispatch code — an agent branches on this, not `message` prose.
    pub code: rules::RuleCode,
    /// The finding, verbatim (the same text the report's `rule: …` line carries).
    pub message: String,
    /// The participating package name(s): `[from, to]` for a denied dependency, the
    /// ordered node path for a cycle, the single package for an orphan / unknown deny.
    pub packages: Vec<String>,
    /// The offending package's directory relative to the checked root (unix slashes),
    /// absent when no single location applies (a cycle, or an absent deny package).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub path: Option<String>,
}

/// One `<name>.annotation` sidecar that annotates nothing: the file it names is not there.
/// Its own list, not a [`AnnotationViolation`], because it is not an issue about an Annotation
/// — a dangling path claims a target that does not exist, which says nothing about any
/// Annotation's parts — and not a [`RuleViolation`], because no `[rules]` table configures it
/// and no package participates. Located and dual-rendered like both: `path` is what an agent
/// acts on, `message` is what a human reads.
#[derive(Debug, Clone, Serialize)]
pub struct OrphanSidecar {
    /// The sidecar, relative to the checked root (unix slashes).
    pub path: String,
    /// The sibling file it names, which does not exist. Same coordinate space as `path`.
    pub target: String,
    /// The finding as one human line (the TEXT report emits `path: message`).
    pub message: String,
}

/// One `.annotation` artifact — a directory charter or a `<name>.annotation` sidecar — with
/// content below its first line. Its own list, not an [`AnnotationViolation`], because it is not
/// an issue about an Annotation: every part may be present and non-empty, and the whole
/// annotation inside the bound, and the defect is still that the FILE holds more than the one
/// line it IS. The same non-Annotation class as [`OrphanSidecar`], located and dual-rendered the
/// same way.
#[derive(Debug, Clone, Serialize)]
pub struct TrailingContent {
    /// The `.annotation` file, relative to the checked root (unix slashes).
    pub path: String,
    /// 1-based line of the first content past line 1 — where to start deleting.
    pub line: usize,
    /// The finding as one human line (the TEXT report emits `path:line: message`).
    pub message: String,
}

/// The whole structured `--strict-check` verdict for one root: annotation violations,
/// dangling sidecars, `.annotation` files with trailing content, PLUS architectural `[rules]`
/// findings. This is the ONE producer every
/// surface drives — the CLI's TEXT report ([`StrictReport::to_text`]) and `--format json`
/// ([`StrictReport::to_json`]) — so no two surfaces can drift.
#[derive(Debug, Clone, Serialize)]
pub struct StrictReport {
    /// True iff there are no annotation violations, no dangling sidecars, no `.annotation` file
    /// with content past its one line, and no rule violations.
    pub passed: bool,
    /// Number of annotation violations (matches the TEXT "Found N error(s)").
    pub error_count: usize,
    /// Number of code files examined.
    pub files_checked: usize,
    /// How many of `files_checked` already carry a conforming annotation. The convergence
    /// numerator behind the "N of M files annotated" footer — an agent watches this climb
    /// toward `files_checked` instead of reading only the terminal error count.
    pub annotated_count: usize,
    pub violations: Vec<AnnotationViolation>,
    pub orphan_sidecars: Vec<OrphanSidecar>,
    pub trailing_content: Vec<TrailingContent>,
    pub rule_violations: Vec<RuleViolation>,
}

impl StrictReport {
    /// An empty passing report — the identity for [`merge`](Self::merge) so a multi-root
    /// CLI run can fold each root's verdict into one document.
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

    /// Fold another root's verdict in (multi-root `--strict-check --format json`): sum
    /// the counts, concatenate the findings, AND together the pass flags.
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

    /// Render the DEFAULT human report + exit code (0 pass / 1 any violation). Violation
    /// lines (or "All N files passed"), then any rule lines — each list capped for humans at
    /// `max_per_node` via the `[+N more …]` overflow idiom (JSON is never capped; a summary
    /// count line is always present). `max_per_node` is `None` for "no cap" (`--full`).
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
            // The length bound ships ON, so an adopter can hit it having configured nothing:
            // name the escape in the same output that failed them. Once per report, not per
            // violation — the remedy is identical for every over-length annotation, and repeating
            // it on each line would drown the findings.
            if let Some(max) = self.violations.iter().find_map(|v| v.defect.max) {
                out.push_str(&format!(
                    "note: the annotation length bound is {max} — change it with `[rules] max_annotation_length = <N>` or `--max-length <N>`, or disable it with `--max-length 0`\n"
                ));
            }
            code = crate::exit::STRICT_FAILURE;
        }
        // Progress, not just a terminal error count: how far the tree is toward every
        // code file carrying an annotation. An agent watches this converge.
        out.push_str(&format!(
            "{} of {} files annotated\n",
            self.annotated_count, self.files_checked
        ));
        // A dangling sidecar is a path problem, not an annotation problem, so it gets its own
        // `path: message` lines rather than being folded into the violation list an agent
        // reads as "fix this annotation".
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
        // Content past line 1 is a defect of the FILE, not of any Annotation part, so it gets its
        // own located lines beside the dangling sidecars rather than being folded into the
        // violation list an agent reads as "fix this annotation".
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
        // Architectural rule findings append as `rule: <message>` lines — line-per-finding,
        // nonzero exit when any exist.
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

    /// Serialize to the structured JSON document (see the schema note above). Every
    /// caller that emits the machine-readable verdict calls THIS, so the document is
    /// byte-for-byte identical for the same inputs.
    pub fn to_json(&self) -> String {
        // Plain owned data with derived `Serialize` — serialization cannot fail (DbC).
        serde_json::to_string_pretty(self).expect("strict report serializes to JSON")
    }
}

/// Append at most `cap` rendered lines from `items`, then — when any were withheld — one
/// overflow marker naming the count and pointing at the uncapped JSON. This is the
/// strict-report counterpart of the renderer's `--max-per-node` truncation: humans get a
/// bounded, scannable report; `--format json` serializes every finding. `cap` is never
/// `Some(0)` (config normalizes 0 to `None` = no cap), so at least one line always shows
/// when the list is non-empty.
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

/// The structured verdict for one root: annotation linting AND (when the root's config
/// configures any `[rules]`) architectural dependency rules, folded into ONE report.
/// This is the single composition every surface drives — the CLI's TEXT and JSON strict
/// paths — so a verdict is identical whichever asks.
/// Building the graph is skipped entirely when no rule is active (a repo with no
/// `[rules]` does zero extra work).
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
    // The dependency graph feeds ONE signal: the architectural `[rules]` findings. No rule
    // configured, no graph build — a repo with no `[rules]` does zero extra work.
    if config.rules.is_active() {
        // Same filter as the file walk: the rules graph sees exactly the manifests the
        // tree would show (gitignore/hidden/`tests`/`-I` honored). UNCAPPED by depth
        // (`None`), like the file walk feeding this check: `--strict-check` is a gate over
        // the whole tree, not a rendered view, so `-L` never shrinks what it evaluates.
        let graph = graph::build(
            &[root.to_path_buf()],
            config.display.gitignore,
            config.display.include_tests,
            excludes,
            None,
        );
        // `PackageEdges::dir` is canonicalized/absolute; canonicalize the root once so
        // the location relativizes to the same unix path shape as annotation `path`s
        // (falling back to the full dir if it lies outside the root, mirroring
        // `check_annotations`' `strip_prefix(root).unwrap_or(path)`).
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
        // Opt-in gate: a manifest-bearing package that owns annotated files but resolves no
        // concern charter FAILS the check. Modeled on `forbid_orphans` (a `[rules]` toggle →
        // a fatal `RuleViolation`), so it rides the existing rule-violation surface. Off by
        // default; the charter census is always available, this turns it into a gate.
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

/// The structured verdict for a SINGLE explicitly-named file — annotation linting ONLY. A
/// lone file has no package neighbourhood, so the directory-scale signals `check_structured`
/// derives from the dependency graph (`[rules]`, the charter gate) and the directory-wide
/// sidecar census do not apply and no graph is built. Reuses the ONE per-file analyzer
/// [`check_annotations`], so a file checked this way is checked byte-identically to the same
/// file checked inside its directory; only the composition (no graph) differs. `root` is the
/// file's parent, used solely to relativize the displayed path.
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

/// Analyze every code file's annotation and produce the sorted structured violations, the
/// count of conforming files, and the root-relative paths of the annotated ones. Violations
/// are sorted by the machine-parseable `path:line` key so the report is deterministic
/// regardless of walk order.
fn check_annotations(
    root: &Path,
    files: &[PathBuf],
    config: &Config,
) -> (Vec<AnnotationViolation>, usize, Vec<String>) {
    let mut violations: Vec<AnnotationViolation> = Vec::new();
    let mut annotated_count = 0usize;
    // Root-relative unix paths of the files that CARRY an annotation (any comment, even a
    // non-conforming one) — the input to the `require_package_charter` rule, which only fires
    // on a package whose files are actually annotated.
    let mut annotated_files: Vec<String> = Vec::new();
    for path in files {
        let rel = rel_of(root, path);
        let Some(lang) = config.language_for_path(path) else {
            // No comment marker, so this file's contract lives in a `<name>.annotation`
            // sidecar when it has one — checked by the same three-field grammar as a
            // directory charter, and reported at the SIDECAR's path, which is the file an
            // author edits to fix it. With no sidecar there is nothing to lint: the file is
            // in the set only because `--include` opted it in, and its grammar is unknown.
            if let Some(body) = sidecar::body(path) {
                annotated_files.push(rel.clone());
                // Same precedence as a directory charter — reported once, by `trailing_contents`.
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

        // Per-branch facts, assembled ONCE below (shared `expected`, marker, hint + the
        // tailored suggestion). A conforming annotation is counted and skipped; every other
        // outcome maps to a violation via the shared `defect_parts`.
        let Some((line, category, defect, found, seed, detail)) = defect_parts(
            annotation::analyze_file(path, lang, config.rules.max_annotation_length),
        ) else {
            annotated_count += 1;
            annotated_files.push(rel);
            continue;
        };

        // A malformed or over-length outcome still CARRIES a comment (only `Missing` does
        // not), so the file counts as annotated for the charter rule's purpose — a package
        // whose files carry annotations is a package that owes a charter.
        if !matches!(category, Category::MissingAnnotation) {
            annotated_files.push(rel.clone());
        }
        // No stub for an over-length annotation — see `defect_parts`.
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

    // A present `.annotation` breadcrumb is an OPT-IN charter, so its shape is enforced by the
    // very same grammar — a malformed one is a violation, never a silent no-op.
    violations.extend(charter_violations(root, files, config));

    violations.sort_by(|a, b| (&a.path, a.line).cmp(&(&b.path, b.line)));
    (violations, annotated_count, annotated_files)
}

/// The per-violation facts extracted from one non-`Ok` annotation outcome: the real `line`, the
/// [`Category`], the machine `defect`, the offending `found` text, a concern `seed` to tailor the
/// suggestion from, and the human `detail`. A named tuple so both the per-file lint and the
/// `.annotation` charter check share the one extraction ([`defect_parts`]).
type DefectParts = (
    usize,
    Category,
    Defect,
    Option<String>,
    Option<String>,
    Option<String>,
);

/// Map a non-`Ok` annotation [`Outcome`](annotation::Outcome) to the per-violation facts the
/// report assembles ([`DefectParts`]). Returns `None` for `Ok` (the caller counts it as
/// annotated). Shared by the per-file lint AND the `.annotation` charter check, so both diagnose
/// against the ONE grammar identically.
fn defect_parts(outcome: annotation::Outcome) -> Option<DefectParts> {
    use annotation::Outcome;
    match outcome {
        Outcome::Ok => None,
        // Echo the offending non-comment / wrong-marker line (trimmed); `None` for an empty /
        // unreadable head. Nothing usable to seed a concern from (stem fallback downstream).
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
        // A comment exists but does not carry three non-empty fields: reuse its text as the
        // concern seed; `missing` names which keyed fields are absent or empty, and `detail`
        // (when the checker set one) says which of those two it was.
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
        // The shape is right but the annotation as a whole is over the bound. No concern seed,
        // hence no suggestion: the only text to seed one from is the over-length annotation
        // itself — a stub would restate the defect and replace conforming fields with
        // placeholders. The remedy is to shorten the line.
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

/// The human `detail` for an over-length annotation — `"the annotation is 240 characters, over
/// the 200 limit"`.
/// Rendered once here, at construction, so the JSON `detail` and the TEXT message carry
/// byte-identical prose. The numbers themselves live structurally in `defect.length` /
/// `defect.max`, which is what an agent branches on; this is only their human rendering, and it
/// borrows [`annotation`]'s count pluralization so `1 character` never renders as `1 characters`.
fn too_long_detail(length: usize, max: usize) -> String {
    format!(
        "the annotation is {}, over the {max} limit",
        annotation::counted(length, "character")
    )
}

/// Enforce every `.annotation` charter breadcrumb in the tree's directories against the ONE
/// three-field grammar (via [`annotation::analyze_charter`]) — opting in means doing it right,
/// so a malformed breadcrumb is a fatal violation, not a silent no-op. The directories checked
/// are exactly those the tree shows (every ancestor of a code file), so render and enforcement
/// agree on scope.
fn charter_violations(root: &Path, files: &[PathBuf], config: &Config) -> Vec<AnnotationViolation> {
    let mut out = Vec::new();
    for dir in tree_dirs(root, files) {
        let Some(content) = charter::read_charter_file(&dir) else {
            continue;
        };
        // Content past line 1 is reported ONCE, by `trailing_contents`, and suppresses the part
        // diagnosis: `found:` and `suggestion:` would otherwise carry the stray line into a
        // report whose contract is one finding per line.
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

/// One violation for a BARE (marker-less) `.annotation` body — a directory's charter or a
/// file's sidecar. Both hold the same three-field line with no comment marker, so both report
/// identically: at the path of the file that HOLDS the line, with `marker` empty, the bare
/// exemplar as `example`, and a marker-less suggestion (the leading space a marker would add is
/// trimmed). `subject` names the thing being annotated — a directory or a file — and seeds the
/// stub when the body carried no reusable text. ONE builder, so the two scales cannot drift.
fn bare_violation(
    path: String,
    language: &str,
    subject: &str,
    (line, category, defect, found, seed, detail): DefectParts,
) -> AnnotationViolation {
    let seed = seed.as_deref().filter(|s| !s.is_empty());
    // An over-length body gets no stub, for the same reason a file annotation does not — see
    // `defect_parts`. A body someone WRAPPED in a comment marker gets the line from inside the
    // wrapper verbatim: seeding a stub from the wrapped text would embed the marker, which is
    // the one thing a suggestion must not do when the marker IS the defect.
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

/// Every `<name>.annotation` sidecar in the tree that annotates nothing — the file it names is
/// not beside it. Scanned per directory (the same directory set `charter_violations` walks)
/// rather than over the walked file set, because a dangling sidecar is precisely the one no
/// listed file points at. A sidecar whose named file EXISTS is never reported here: it either
/// carries that file's contract (checked with the file, above) or, for a file that can hold its
/// own first line, is an ordinary file the tree lists like any other.
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

/// Every `<name>.annotation` sidecar of a checked file that carries content past its one line.
/// Driven off the FILE set, not a directory scan, so `check_file` covers exactly the named file's
/// sidecar. A file that maps to a comment marker owns its own first line and takes no sidecar
/// ([`sidecar::target_of`]'s rule), so it is skipped here for the same reason.
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

/// Every `.annotation` artifact in the tree carrying content past its one line — file sidecars
/// plus each directory's charter, over the SAME directory census `charter_violations` enforces,
/// so render and enforcement agree on scope. Deterministic (sorted by path).
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

/// The finding for an `.annotation` artifact with content past line 1. ONE builder for both
/// scales, so a charter and a sidecar cannot drift on the remedy they state.
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

/// A directory's `.annotation` path, relative to the checked root (unix slashes) — the root's own
/// charter is the bare name, with no leading slash.
fn charter_rel(root: &Path, dir: &Path) -> String {
    let dir_rel = rel_of(root, dir);
    if dir_rel.is_empty() {
        CHARTER_FILE.to_string()
    } else {
        format!("{dir_rel}/{CHARTER_FILE}")
    }
}

/// A path relative to the checked root, unix slashes — the one coordinate space every finding
/// in this report is located in. Falls back to the full path when it lies outside the root.
fn rel_of(root: &Path, path: &Path) -> String {
    crate::util::to_unix_path(path.strip_prefix(root).unwrap_or(path))
}

/// The base name of a unix-slashed relative path (`a/b/trials.csv` -> `trials.csv`).
fn file_name(rel: &str) -> String {
    rel.rsplit('/').next().unwrap_or(rel).to_string()
}

/// Every directory the tree renders: the root and every ancestor directory of a listed code
/// file (a `BTreeSet` for deterministic order). A charter breadcrumb only renders in such a
/// directory, so this is exactly the set `--strict-check` enforces `.annotation` shape over.
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

/// A package directory relative to the checked root, unix slashes — the same relativization
/// rule-violation `path`s use, so package dirs and annotation `path`s live in one coordinate
/// space. Falls back to the full canonical dir if it lies outside the root.
fn rel_dir(dir: &Path, root_canon: &Path) -> String {
    crate::util::to_unix_path(dir.strip_prefix(root_canon).unwrap_or(dir))
}

/// The deepest package directory (from `pkg_dirs`) that contains `file_rel` — the file's
/// owning package, mirroring `graph`'s deepest-ancestor attribution so a file in a nested
/// package is attributed to the inner one. `None` when the file lives under no package.
fn owning_dir<'a>(file_rel: &str, pkg_dirs: &'a [String]) -> Option<&'a str> {
    pkg_dirs
        .iter()
        .map(String::as_str)
        .filter(|dir| dir_contains(dir, file_rel))
        .max_by_key(|dir| dir.len())
}

/// Whether `dir_rel` is `file_rel`'s directory or an ancestor of it, comparing on the
/// root-relative unix path components. An empty `dir_rel` is the root package, which
/// contains every file under the tree.
fn dir_contains(dir_rel: &str, file_rel: &str) -> bool {
    dir_rel.is_empty()
        || file_rel
            .strip_prefix(dir_rel)
            .is_some_and(|rest| rest.starts_with('/'))
}

/// The root-relative dirs of packages that OWN at least one annotated file (deepest-ancestor
/// attribution) — the input to the `require_package_charter` rule, so "which package owns
/// this annotation" is defined in one place rather than at each consumer.
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

/// The opt-in `require_package_charter` gate: every manifest-bearing package that OWNS an
/// annotated file but resolves NO concern charter (a `.annotation` breadcrumb, else a promoted
/// entry-file annotation — [`charter::resolve_from_fs`]) is a fatal `RuleViolation`. Off by
/// default (checked by the caller); a package may honestly omit a charter, but enabling the
/// rule promotes the always-available census into a gate. Deterministic (sorted by message).
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

/// Build a FILE-TAILORED suggestion: whatever descriptive text the file already carries
/// (`seed`, from [`annotation::concern_seed`]) or its stem seeds the `Concern:` field, then
/// the judgment fields are scaffolded as `<…>` placeholder slots. The check is form-only, so
/// the stub is well-formed: applying it fixes the form defect instead of stacking a second one.
/// It is not a finished annotation — the `<…>` slots are the two judgments (the boundary and
/// the contract) an agent still has to write out, and a configured `max_annotation_length`
/// applies to the stub like any other line (`<concern owned elsewhere>` alone is 25
/// characters, and a reused `seed` carries whatever length the file already had).
fn tailored_suggestion(marker: &str, path: &str, seed: Option<&str>) -> String {
    let concern = match seed {
        Some(s) => s.to_string(),
        None => format!("<what {} does>", file_stem(path)),
    };
    format!(
        "{marker} Concern: {concern} | Non-concern: <concern owned elsewhere> | IO: (<inputs>) -> <outputs>"
    )
}

/// The file's base name without its extension (`a/b/utils.py` -> `utils`), the stem the
/// suggestion falls back to when a file carries no reusable descriptive text.
fn file_stem(path: &str) -> &str {
    let name = path.rsplit('/').next().unwrap_or(path);
    name.rsplit_once('.').map_or(name, |(stem, _)| stem)
}

/// The comment delimiter a file of this language should open its annotation with,
/// for the "add a `MARKER` comment" hint: the line token, else the block open, else
/// the first docstring delimiter. Falls back to `#` for a language with no delimiter
/// configured (only reachable via a hand-rolled `pattern`-only entry).
fn marker(lang: &crate::config::Language) -> String {
    lang.line
        .clone()
        .or_else(|| lang.block.as_ref().map(|(open, _)| open.clone()))
        .or_else(|| lang.docstring.first().cloned())
        .unwrap_or_else(|| "#".to_string())
}
