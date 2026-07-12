// Concern: lint mode — validates every code file's annotation against the one three-field format and reports offenders as `path:LINE: message` (language, marker, real line, offending content, a conformant example) | Non-concern: the tree view | IO: (files, Config) -> (report, exit_code)

//! # Strict-check JSON schema (`--strict-check --format json` and MCP `strict_check`)
//!
//! The structured verdict is a machine-consumable contract (the counterpart to the
//! default human TEXT report), so its shape is documented here — mirroring the schema
//! note in `render/json.rs`. Both the CLI's `--format json` and the MCP `strict_check`
//! tool serialize the SAME [`StrictReport`], so they are byte-for-byte identical. The
//! exact same text is exposed at runtime via `--schema` and defined ONCE in
//! [`SCHEMA_DOC`] (an embedded file), so this rustdoc and the `--schema` output can never
//! drift apart:
//!
#![doc = concat!("```text\n", include_str!("strict_schema.txt"), "```")]
//!
//! `category` maps `Outcome::Missing` -> `missing_annotation`,
//! `Outcome::Malformed` -> `malformed_annotation` (a comment that is not the three-field
//! shape), and `Outcome::Vacuous` -> `annotation_vacuous` (the shape is present but a
//! required slot is box-filled — a FATAL violation). `found` carries the raw landing line
//! even for ordinary code (so no misleading "unrecognized token" category is needed). A
//! separate, NON-FATAL `warnings` array carries advisories that do not fail the check —
//! today only `annotation_on_orphan` (an annotated file in an orphaned package).

use std::collections::BTreeSet;
use std::path::{Path, PathBuf};

use globset::GlobSet;
use serde::Serialize;

use crate::annotation;
use crate::charter;
use crate::config::Config;
use crate::graph;
use crate::rules;
use crate::walk::CHARTER_FILE;

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
    /// A comment is present but is not the three-field `Concern: … | Non-concern: … |
    /// IO: …` shape — a keyed field is absent or the ` | ` structure is broken
    /// (`Outcome::Malformed`).
    MalformedAnnotation,
    /// The three-field shape is present but a required slot (Concern, Non-concern, or an
    /// IO operand) is empty, a filler token, or an unfilled placeholder — a copied
    /// box-filling stub (`Outcome::Vacuous`). Distinct from `MalformedAnnotation` so an
    /// agent can tell "not the format" from "the format, but hollow", and so a
    /// thoughtlessly-filled suggestion is a FAILING state, not merely discouraged.
    AnnotationVacuous,
}

/// The canonical annotation shape an agent should converge on — the fill-in `template`
/// plus which named parts are ENFORCED (`required`) vs ADVISED (`recommended`). Every
/// violation carries this so an agent reads the contract off the finding instead of
/// reverse-engineering it. All three fields are `required`; `recommended` is empty (the
/// old advisory boundary is now a required field). Part tokens come from
/// [`crate::annotation`], the grader that produces them, so the contract and the delta
/// name the SAME parts and cannot drift.
#[derive(Debug, Clone, Copy, Serialize)]
pub struct Expected {
    pub template: &'static str,
    pub required: &'static [&'static str],
    pub recommended: &'static [&'static str],
}

/// The one enforced contract, identical for every finding (declared once so it can't drift
/// from the grader). All three fields are required and substantive; `IO:` accepts the
/// blessed value `none`. `pub(crate)` so the embedded annotation guide ([`crate::guide`])
/// renders the SAME template the grader enforces.
pub(crate) const EXPECTED: Expected = Expected {
    template: "Concern: {what it does} | Non-concern: {what it isn't} | IO: (in) -> out  OR  none",
    required: &[
        crate::annotation::PART_CONCERN,
        crate::annotation::PART_NON_CONCERN,
        crate::annotation::PART_IO,
    ],
    recommended: &[],
};

/// The machine-coded delta between the required shape and what `found` carries: which
/// named parts are ABSENT (`missing`) vs PRESENT-BUT-HOLLOW (`vacuous`). An agent branches
/// on these stable part tokens (`concern` | `non_concern` | `io`), never on `message`
/// prose. Each list is omitted when empty, per the schema's absent-key convention.
#[derive(Debug, Clone, Serialize)]
pub struct Defect {
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub missing: Vec<&'static str>,
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub vacuous: Vec<&'static str>,
}

/// One structured annotation violation. The default TEXT report is one rendering over
/// these ([`AnnotationViolation::message`]); `--format json` and MCP serialize them.
#[derive(Debug, Clone, Serialize)]
pub struct AnnotationViolation {
    /// Path relative to the checked root, unix slashes.
    pub path: String,
    /// 1-based line the scan landed on (past a shebang / blank lines).
    pub line: usize,
    /// Resolved language name (e.g. `python`).
    pub language: String,
    /// Which class of failure (`missing_annotation` | `malformed_annotation` |
    /// `annotation_vacuous`).
    pub category: Category,
    /// The comment delimiter this language expects the annotation to open with.
    pub marker: String,
    /// A canonical, self-conforming annotation line for this language (the config's
    /// per-language exemplar) — a guaranteed-valid concrete instance, distinct from the
    /// abstract `expected.template` and the file-tailored `suggestion`.
    pub example: String,
    /// The machine-coded delta — which template parts are missing/vacuous. An agent acts
    /// on this, not on `message`.
    pub defect: Defect,
    /// The canonical annotation contract (template + required/recommended parts).
    pub expected: Expected,
    /// The offending line — the raw landing line (missing) or the extracted annotation
    /// (malformed / vacuous) — or `None` for an empty / unreadable head.
    pub found: Option<String>,
    /// A FILE-TAILORED candidate to adapt: whatever descriptive text the file already
    /// carries (or its stem) seeds the `Concern:` field, with the judgment fields scaffolded
    /// as VACUOUS placeholder slots (`<concern owned elsewhere>`, `(<inputs>) -> <outputs>`).
    /// Because the `annotation_vacuous` gate rejects those slots, the stub scaffolds the
    /// shape WITHOUT letting an agent submit it unthought — the placeholders must be replaced.
    pub suggestion: String,
    /// Why the annotation is vacuous — which slot is empty/filler/placeholder. Present
    /// only for `annotation_vacuous`; absent (not null) for the other categories, per the
    /// schema's key-presence convention.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub detail: Option<String>,
}

impl AnnotationViolation {
    /// The one human message line for this violation, keeping the machine-parseable
    /// `path:line:` prefix. The TEXT report is exactly these, one per line — so this is
    /// the single place the wording lives, shared by the CLI report and any renderer.
    fn message(&self) -> String {
        match self.category {
            Category::MissingAnnotation => {
                // Name the language and the exact marker to add, show a conformant
                // example, and — when a foreign/wrong-marker line was present — echo
                // it so the fix is unambiguous (e.g. "you used `;` not `--`").
                let mut msg = format!(
                    "{}:{}: missing annotation [{}] — add a `{}` comment. suggestion: {}",
                    self.path, self.line, self.language, self.marker, self.suggestion,
                );
                if let Some(found) = &self.found {
                    msg.push_str(&format!(" found: '{found}'"));
                }
                msg
            }
            Category::MalformedAnnotation => format!(
                "{}:{}: annotation is malformed [{}] — expected '{}'. found: '{}'. suggestion: {}",
                self.path,
                self.line,
                self.language,
                self.expected.template,
                self.found.as_deref().unwrap_or(""),
                self.suggestion,
            ),
            Category::AnnotationVacuous => format!(
                "{}:{}: annotation is vacuous [{}] — {}. Say what this file actually does and \
                 what it does NOT. found: '{}'. suggestion: {}",
                self.path,
                self.line,
                self.language,
                self.detail.as_deref().unwrap_or("a required slot is empty"),
                self.found.as_deref().unwrap_or(""),
                self.suggestion,
            ),
        }
    }
}

/// One NON-FATAL annotation advisory — guidance that does NOT fail `--strict-check`.
/// Carries a stable dispatch [`code`](crate::exit::code) + `path` + human `message`, the
/// same located-diagnostic shape as [`AnnotationViolation`] and [`crate::graph::Warning`],
/// so an agent branches on `code` and only humans read the prose. One kind today: the
/// per-package [`crate::exit::code::ANNOTATION_ON_ORPHAN`] — a package-level concern, so
/// `path` is the package directory and there is no single line or language.
#[derive(Debug, Clone, Serialize)]
pub struct AnnotationWarning {
    /// Stable dispatch code (`annotation_on_orphan`).
    pub code: &'static str,
    /// Path relative to the checked root, unix slashes — the package directory.
    pub path: String,
    /// The advisory body (no location prefix) — the dual-render kept next to the `code`,
    /// the same way [`RuleViolation::message`] carries its finding verbatim.
    pub message: String,
}

impl AnnotationWarning {
    /// The one human TEXT line for this advisory, keeping the machine-parseable `path`
    /// prefix and flagging it as non-fatal — the single place its text rendering lives (not
    /// serialized; JSON carries the structured fields instead). A package-level advisory
    /// tags the bracket with the dispatch `code` (`path: warning [code] — …`).
    fn text_line(&self) -> String {
        format!("{}: warning [{}] — {}", self.path, self.code, self.message)
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

/// The whole structured `--strict-check` verdict for one root: annotation violations
/// PLUS architectural `[rules]` findings. This is the ONE producer every surface drives
/// — the CLI's TEXT report ([`StrictReport::to_text`]), `--format json`
/// ([`StrictReport::to_json`]), and the MCP `strict_check` tool — so no two surfaces
/// can drift.
#[derive(Debug, Clone, Serialize)]
pub struct StrictReport {
    /// True iff there are no annotation AND no rule violations. NON-FATAL `warnings`
    /// never flip this — guidance advises, it does not fail the check.
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
    pub rule_violations: Vec<RuleViolation>,
    /// NON-FATAL annotation advisories (`annotation_on_orphan`). Never truncated in
    /// JSON (an agent needs every finding); the TEXT report caps them via the
    /// `--max-per-node` overflow idiom. Omitted from JSON when empty, per the schema's
    /// absent-key convention, so a clean run's document is byte-for-byte unchanged.
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub warnings: Vec<AnnotationWarning>,
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
            rule_violations: Vec::new(),
            warnings: Vec::new(),
        }
    }

    /// Fold another root's verdict in (multi-root `--strict-check --format json`): sum
    /// the counts, concatenate the findings and advisories, AND together the pass flags.
    pub fn merge(&mut self, other: StrictReport) {
        self.passed = self.passed && other.passed;
        self.error_count += other.error_count;
        self.files_checked += other.files_checked;
        self.annotated_count += other.annotated_count;
        self.violations.extend(other.violations);
        self.rule_violations.extend(other.rule_violations);
        self.warnings.extend(other.warnings);
    }

    /// Render the DEFAULT human report + exit code (0 pass / 1 any violation). Violation
    /// lines (or "All N files passed"), then any rule lines, then NON-FATAL advisory
    /// warnings — each list capped for humans at `max_per_node` via the `[+N more …]`
    /// overflow idiom (JSON is never capped; a summary count line is always present).
    /// `max_per_node` is `None` for "no cap" (`--full`). A run with zero violations and
    /// zero rule findings still exits 0 even when warnings are present.
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
            code = crate::exit::STRICT_FAILURE;
        }
        // Progress, not just a terminal error count: how far the tree is toward every
        // code file carrying an annotation. An agent watches this converge.
        out.push_str(&format!(
            "{} of {} files annotated\n",
            self.annotated_count, self.files_checked
        ));
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
        // NON-FATAL advisories last, clearly separated, and NEVER changing the exit code —
        // guidance nudges the author toward a bounded annotation without failing the gate.
        if !self.warnings.is_empty() {
            out.push('\n');
            push_capped(&mut out, &self.warnings, max_per_node, "warning", |w| {
                w.text_line()
            });
            out.push_str(&format!("Found {} warning(s)\n", self.warnings.len()));
        }
        (out, code)
    }

    /// Serialize to the structured JSON document (see the schema note above). Both the
    /// CLI's `--format json` and MCP `strict_check` call THIS, so they are byte-for-byte
    /// identical for the same inputs.
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
/// This is the single composition every surface drives — the CLI's TEXT/JSON strict
/// paths and the MCP `strict_check` tool — so a verdict is identical whichever asks.
/// Building the graph is skipped entirely when no rule is active (a repo with no
/// `[rules]` does zero extra work).
pub(crate) fn check_structured(
    root: &Path,
    files: &[PathBuf],
    config: &Config,
    excludes: &GlobSet,
) -> StrictReport {
    let (violations, mut warnings, annotated_count, annotated_files) =
        check_annotations(root, files, config);
    let mut rule_violations = Vec::new();
    // The dependency graph now feeds TWO signals: the architectural `[rules]` findings (only
    // when a rule is active) AND the always-on `annotation_on_orphan` advisory that connects
    // annotations to the graph. Build it when EITHER is wanted — a repo with no rules AND no
    // annotated files (nothing an orphan advisory could fire on) still does zero graph work.
    if config.rules.is_active() || !annotated_files.is_empty() {
        // Same filter as the file walk: the rules graph sees exactly the manifests the
        // tree would show (gitignore/hidden/`tests`/`-I` honored).
        let graph = graph::build(
            &[root.to_path_buf()],
            config.display.gitignore,
            config.display.include_tests,
            excludes,
        );
        // `PackageEdges::dir` is canonicalized/absolute; canonicalize the root once so
        // the location relativizes to the same unix path shape as annotation `path`s
        // (falling back to the full dir if it lies outside the root, mirroring
        // `check_annotations`' `strip_prefix(root).unwrap_or(path)`).
        let root_canon = root.canonicalize().unwrap_or_else(|_| root.to_path_buf());
        if config.rules.is_active() {
            rule_violations = rules::evaluate(&graph.packages, &config.rules)
                .into_iter()
                .map(|v| RuleViolation {
                    code: v.code,
                    message: v.message,
                    packages: v.packages,
                    path: v.dir.map(|d| {
                        crate::util::to_unix_path(d.strip_prefix(&root_canon).unwrap_or(&d))
                    }),
                })
                .collect();
        }
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
        // The always-on cross-file signal: annotated files sitting in an orphaned package.
        warnings.extend(orphan_annotation_warnings(
            &graph,
            &root_canon,
            &annotated_files,
        ));
        // Keep the merged advisories deterministic; package-level warnings are keyed by path.
        warnings.sort_by(|a, b| a.path.cmp(&b.path));
    }
    StrictReport {
        passed: violations.is_empty() && rule_violations.is_empty(),
        error_count: violations.len(),
        files_checked: files.len(),
        annotated_count,
        violations,
        rule_violations,
        warnings,
    }
}

/// The structured verdict for a SINGLE explicitly-named file — annotation linting ONLY. A
/// lone file has no package neighbourhood, so the directory-scale signals `check_structured`
/// derives from the dependency graph (`[rules]`, the charter gate, the `annotation_on_orphan`
/// advisory) do not apply and no graph is built. Reuses the ONE per-file analyzer
/// [`check_annotations`], so a file checked this way is graded byte-identically to the same
/// file checked inside its directory; only the composition (no graph) differs. `root` is the
/// file's parent, used solely to relativize the displayed path.
pub(crate) fn check_file(root: &Path, files: &[PathBuf], config: &Config) -> StrictReport {
    let (violations, warnings, annotated_count, _annotated_files) =
        check_annotations(root, files, config);
    StrictReport {
        passed: violations.is_empty(),
        error_count: violations.len(),
        files_checked: files.len(),
        annotated_count,
        violations,
        rule_violations: Vec::new(),
        warnings,
    }
}

/// Analyze every code file's annotation and produce the sorted structured violations
/// PLUS the non-fatal advisory warnings. Both lists are sorted by the machine-parseable
/// `path:line` key so the report is deterministic regardless of walk order.
fn check_annotations(
    root: &Path,
    files: &[PathBuf],
    config: &Config,
) -> (
    Vec<AnnotationViolation>,
    Vec<AnnotationWarning>,
    usize,
    Vec<String>,
) {
    let mut violations: Vec<AnnotationViolation> = Vec::new();
    let mut warnings: Vec<AnnotationWarning> = Vec::new();
    let mut annotated_count = 0usize;
    // Root-relative unix paths of the files that CARRY an annotation (any comment, even a
    // non-conforming or vacuous one) — the input to the `annotation_on_orphan` advisory,
    // which only fires on a package whose files are actually annotated.
    let mut annotated_files: Vec<String> = Vec::new();
    for path in files {
        let Some(lang) = config.language_for_path(path) else {
            continue;
        };
        let rel = crate::util::to_unix_path(path.strip_prefix(root).unwrap_or(path));
        let mk = marker(lang);

        // Per-branch facts, assembled ONCE below (shared `expected`, marker, hint + the
        // tailored suggestion). A conforming annotation is counted and skipped; every other
        // outcome maps to a violation via the shared `defect_parts`.
        let Some((line, category, defect, found, seed, detail)) =
            defect_parts(annotation::analyze_file(path, lang))
        else {
            annotated_count += 1;
            annotated_files.push(rel);
            continue;
        };

        // A non-conforming or vacuous outcome still CARRIES a comment (only `Missing` does
        // not), so the file is annotated for the orphan advisory's purpose — a misleading
        // annotation on a dead package is exactly what it warns about.
        if !matches!(category, Category::MissingAnnotation) {
            annotated_files.push(rel.clone());
        }
        let seed = seed.as_deref().filter(|s| !s.is_empty());
        let suggestion = tailored_suggestion(&mk, &rel, seed);
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
    violations.extend(charter_violations(root, files));

    violations.sort_by(|a, b| (&a.path, a.line).cmp(&(&b.path, b.line)));
    warnings.sort_by(|a, b| a.path.cmp(&b.path));
    (violations, warnings, annotated_count, annotated_files)
}

/// The per-violation facts extracted from one non-`Ok` annotation outcome: the real `line`, the
/// [`Category`], the machine `defect`, the offending `found` text, a concern `seed` to tailor the
/// suggestion from, and the vacuous `detail`. A named tuple so both the per-file lint and the
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
                vacuous: Vec::new(),
            },
            raw.map(|r| r.trim().to_string()),
            None,
            None,
        )),
        // A comment exists but is not the three-field shape: reuse its text as the concern
        // seed; `missing` names which keyed fields are absent.
        Outcome::Malformed {
            line,
            actual,
            missing,
        } => {
            let seed = annotation::concern_seed(&actual).to_string();
            Some((
                line,
                Category::MalformedAnnotation,
                Defect {
                    missing,
                    vacuous: Vec::new(),
                },
                Some(actual),
                Some(seed),
                None,
            ))
        }
        // FATAL: a box-filling stub — the shape is present but `slot` is hollow. Reuse the real
        // concern portion as the seed; `reason` names the slot.
        Outcome::Vacuous {
            line,
            actual,
            slot,
            reason,
        } => {
            let seed = annotation::concern_seed(&actual).to_string();
            Some((
                line,
                Category::AnnotationVacuous,
                Defect {
                    missing: Vec::new(),
                    vacuous: vec![slot],
                },
                Some(actual),
                Some(seed),
                Some(reason),
            ))
        }
    }
}

/// Enforce every `.annotation` charter breadcrumb in the tree's directories against the ONE
/// three-field grammar (via [`annotation::analyze_charter`]) — opting in means doing it right,
/// so a malformed breadcrumb is a fatal violation, not a silent no-op. The directories checked
/// are exactly those the tree shows (every ancestor of a code file), so render and enforcement
/// agree on scope. Reuses the same [`AnnotationViolation`] machinery as the per-file lint; a
/// charter has no comment marker, so `marker` is empty and `example` is the bare exemplar.
fn charter_violations(root: &Path, files: &[PathBuf]) -> Vec<AnnotationViolation> {
    let mut out = Vec::new();
    for dir in tree_dirs(root, files) {
        let Some(content) = charter::read_charter_file(&dir) else {
            continue;
        };
        let Some((line, category, defect, found, seed, detail)) =
            defect_parts(annotation::analyze_charter(&content))
        else {
            continue;
        };
        let dir_rel = crate::util::to_unix_path(dir.strip_prefix(root).unwrap_or(&dir));
        let path = if dir_rel.is_empty() {
            CHARTER_FILE.to_string()
        } else {
            format!("{dir_rel}/{CHARTER_FILE}")
        };
        // A charter is a bare line, so the suggestion is marker-less (the leading space a
        // marker would add is trimmed); its concern seed comes from whatever the breadcrumb
        // already carried, falling back to the directory name.
        let seed = seed.as_deref().filter(|s| !s.is_empty());
        let dir_name = dir
            .file_name()
            .map(|n| n.to_string_lossy().into_owned())
            .unwrap_or_default();
        let suggestion = tailored_suggestion("", &dir_name, seed)
            .trim_start()
            .to_string();
        out.push(AnnotationViolation {
            path,
            line,
            language: "charter".to_string(),
            category,
            marker: String::new(),
            example: charter::EXAMPLE.to_string(),
            defect,
            expected: EXPECTED,
            suggestion,
            found,
            detail,
        });
    }
    out
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

/// The always-on cross-file advisory that connects the tool's two halves — annotations and
/// the dependency graph. For every package the graph shows ORPHANED (reusing the ONE
/// [`rules::orphan_packages`] definition, never a fork) that also carries annotated files,
/// emit one NON-FATAL warning: annotating a dead package misleads agents into treating it as
/// live infrastructure, so the nudge points at the SoC "should we just delete it?" lens.
///
/// Three guards keep this from firing on legitimate orphans (the reason `forbid_orphans` is
/// opt-in): (1) only orphans whose ECOSYSTEM has real internal structure — at least one
/// resolved internal edge among its packages — are surfaced, so a lone entry-point binary or
/// a single-package repo (whose sole package is trivially edgeless) stays silent; (2) only a
/// package that actually OWNS an annotated file warns (a file is attributed to its deepest
/// containing package, so a file in a nested live package never counts toward an outer
/// orphan); (3) a package whose directory IS the scan root is a top-level deliverable (a
/// scan-root package / distribution wrapper) that is depended-on-by-nothing BY DESIGN — the
/// "charter" case — and is never flagged. Never fails the check or changes the exit code.
fn orphan_annotation_warnings(
    graph: &graph::Graph,
    root_canon: &Path,
    annotated_files: &[String],
) -> Vec<AnnotationWarning> {
    use std::collections::HashSet;

    use crate::manifest::Ecosystem;

    if annotated_files.is_empty() {
        return Vec::new();
    }
    // Guard 1: ecosystems that actually have internal dependency structure. Only inside one
    // of these is an orphan anomalous rather than simply "the only package here".
    let structured: HashSet<Ecosystem> = graph
        .packages
        .iter()
        .filter(|p| p.internal.iter().any(|d| d.resolved))
        .map(|p| p.ecosystem)
        .collect();
    if structured.is_empty() {
        return Vec::new();
    }
    let orphans: Vec<&graph::PackageEdges> = rules::orphan_packages(&graph.packages)
        .into_iter()
        .filter(|p| structured.contains(&p.ecosystem))
        .collect();
    if orphans.is_empty() {
        return Vec::new();
    }

    // Guard 2: keep only orphan packages that actually OWN an annotated file (deepest-ancestor
    // attribution). The owned-package set is the shared input the require_package_charter rule
    // also consumes, so "which package owns this annotation" is defined once.
    let owned = packages_owning_annotations(graph, root_canon, annotated_files);

    let mut warnings: Vec<AnnotationWarning> = orphans
        .iter()
        .filter_map(|o| {
            let dir_rel = rel_dir(&o.dir, root_canon);
            // Guard 3 (charter carve-out): a package whose directory IS the scan root is a
            // TOP-LEVEL deliverable — a scan-root package or distribution wrapper — that is
            // depended-on-by-nothing BY DESIGN, not by accident (the "charter" case). An
            // empty root-relative dir means `o.dir == root_canon`, so never flag it as an
            // orphan; only genuinely disconnected INNER packages are anomalous here.
            if dir_rel.is_empty() {
                return None;
            }
            if !owned.contains(dir_rel.as_str()) {
                return None;
            }
            Some(AnnotationWarning {
                code: crate::exit::code::ANNOTATION_ON_ORPHAN,
                path: dir_rel,
                message: format!(
                    "package '{}' appears orphaned — nothing in the dependency graph imports \
                     it and it imports nothing internal — yet its files carry annotations. \
                     Annotating a dead package misleads agents into treating it as live \
                     infrastructure; weigh whether it should be deleted rather than annotated \
                     (SoC: should we just delete it?). Advisory; does not fail the check.",
                    o.name
                ),
            })
        })
        .collect();
    warnings.sort_by(|a, b| a.path.cmp(&b.path));
    warnings
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
/// attribution), the shared input to BOTH the `annotation_on_orphan` advisory and the
/// `require_package_charter` rule — so "which package owns this annotation" is defined once.
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
/// the judgment fields are scaffolded as VACUOUS placeholder slots. Because the
/// `annotation_vacuous` gate rejects `<…>` placeholders (and placeholder IO operands), the
/// returned stub is a *failing* annotation until an agent replaces the slots — it scaffolds
/// the shape without letting the stub be submitted unthought.
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
