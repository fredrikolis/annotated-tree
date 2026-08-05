// Concern: resolves the layered configuration into a language table, display settings, and lint rules | Non-concern: walking or rendering | IO: (paths, CLI overrides) -> Config

use std::collections::HashMap;
use std::path::{Path, PathBuf};

use anyhow::{Context, Result};
use regex::Regex;
use serde::Deserialize;

use crate::rules::Rules;

const DEFAULT_CONFIG: &str = include_str!("default_config.toml");

/// The raw, all-optional shape parsed from a TOML layer. Every layer omits most
/// fields; merging overlays later layers onto earlier ones.
#[derive(Debug, Default, Deserialize)]
#[serde(deny_unknown_fields)]
struct RawConfig {
    display: Option<RawDisplay>,
    limits: Option<RawLimits>,
    rules: Option<RawRules>,
    #[serde(default)]
    languages: HashMap<String, RawLanguage>,
}

/// Lint rules parsed from a `[rules]` table. Declarative and regex-free: `deny` names package
/// pairs, the flags toggle structural checks, `max_annotation_length` bounds the whole
/// annotation line.
#[derive(Debug, Default, Clone, Deserialize)]
#[serde(deny_unknown_fields)]
struct RawRules {
    deny: Option<Vec<[String; 2]>>,
    forbid_cycles: Option<bool>,
    forbid_orphans: Option<bool>,
    require_package_charter: Option<bool>,
    max_annotation_length: Option<usize>,
}

/// Walk-scope limits parsed from a `[limits]` table. Deliberately separate from
/// `[display]`: a runaway-scope cap bounds the walk, it is not a rendering choice.
#[derive(Debug, Default, Clone, Deserialize)]
#[serde(deny_unknown_fields)]
struct RawLimits {
    max_files: Option<usize>,
}

#[derive(Debug, Default, Clone, Deserialize)]
#[serde(deny_unknown_fields)]
struct RawDisplay {
    show_age: Option<bool>,
    ascii: Option<bool>,
    gitignore: Option<bool>,
    hidden: Option<bool>,
    include_tests: Option<bool>,
    max_per_node: Option<usize>,
    include: Option<Vec<String>>,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(deny_unknown_fields)]
struct RawLanguage {
    extensions: Vec<String>,
    comment: Option<String>,
    block: Option<[String; 2]>,
    docstring: Option<Vec<String>>,
    pattern: Option<String>,
}

/// CLI-supplied overrides. `None` means "not specified; keep the merged value".
#[derive(Debug, Default)]
pub struct CliOverrides {
    pub show_age: Option<bool>,
    pub ascii: Option<bool>,
    pub gitignore: Option<bool>,
    pub hidden: Option<bool>,
    pub include_tests: Option<bool>,
    /// Additional `--include` glob selectors from the CLI (each may pipe-bundle several,
    /// tree-style). ADDITIVE to any config `[display] include`: the resolved selector set is
    /// the config list followed by these. Empty means the CLI added no selectors.
    pub include: Vec<String>,
    pub config_file: Option<PathBuf>,
    /// Runaway-scope cap override, modelled as an `Option<Option<usize>>`:
    /// `None` = the CLI said nothing (fall through to env/config/default);
    /// `Some(None)` = `--no-limit`/`--force` (cap disabled);
    /// `Some(Some(n))` = `--max-files n`.
    pub max_files: Option<Option<usize>>,
    /// Per-directory display cap override, same `Option<Option<usize>>` shape as
    /// `max_files`: `None` = CLI silent (use config/default); `Some(None)` =
    /// `--full` (cap disabled); `Some(Some(n))` = `--max-per-node n`.
    pub max_per_node: Option<Option<usize>>,
    /// `--max-length <N>`: the whole-annotation length bound. A plain `Option<usize>` —
    /// `None` is "the CLI said nothing; fall through to the config layers". There is no
    /// `--full`-style sentinel; `--max-length 0` is how you turn the shipped bound off,
    /// since 0 normalizes to "no bound" (the same normalization `--max-per-node 0` uses).
    pub max_annotation_length: Option<usize>,
}

#[derive(Debug, Clone)]
pub struct Display {
    pub show_age: bool,
    pub ascii: bool,
    pub gitignore: bool,
    /// Descend into dot-directories and list dot-files. Independent of `gitignore` — a hidden path
    /// `.gitignore` also names needs both switched to appear — and `.git` is walked under neither.
    pub hidden: bool,
    pub include_tests: bool,
    /// Show at most this many subdirectories AND files per directory, replacing the overflow with a
    /// `[+N folders and F files]` marker; `None` means no cap. A display concern, so it lives here
    /// rather than in `Limits` — it truncates the rendered tree, it does not bound the walk.
    pub max_per_node: Option<usize>,
    /// Glob selectors that ADD files of any type beyond the recognized-language set. A file is
    /// listed when its extension maps to a known language OR matches one of these; an unrecognized
    /// match shows its annotation via marker-agnostic extraction. Compiled to a `GlobSet` at the
    /// walk call site, so a bad pattern surfaces there, next to `-I`'s.
    pub include: Vec<String>,
}

/// Walk-scope limits. `max_files: None` means "no cap". Kept out of `Display`
/// (SoC): these bound the walk, not the rendered output.
#[derive(Debug, Clone)]
pub struct Limits {
    pub max_files: Option<usize>,
}

/// How a single language's first-line annotation is located. The annotation FORMAT is
/// invariant (the three-field `Concern: … | Non-concern: … | IO: …` shape, validated in
/// [`crate::annotation`]) — not configurable — so a language only configures HOW to find
/// its first comment (markers / an escape-hatch `pattern`), never what shape to require.
#[derive(Debug, Clone)]
pub struct Language {
    pub name: String,
    pub line: Option<String>,
    pub block: Option<(String, String)>,
    pub docstring: Vec<String>,
    pub pattern: Option<Regex>,
}

/// The canonical, marker-free annotation body — one concrete, self-conforming instance of the fixed
/// three-field format. The FORMAT is invariant, so a per-language example is DERIVED from this body
/// plus the language's marker, never configured. Distinct from [`crate::strict::EXPECTED`]'s
/// abstract template: this is a filled, valid line, that is the fill-in contract.
const EXAMPLE_BODY: &str =
    "Concern: runs the core loop | Non-concern: transport | IO: (Job) -> Result";

impl Language {
    /// A full, conformant annotation line for this language — [`EXAMPLE_BODY`] wrapped in the
    /// language's comment marker — shown verbatim in `--help` and `--strict-check` diagnostics.
    /// Derived rather than configured because the format is invariant, and a tested invariant
    /// guarantees it round-trips through the extractor and validator as `Outcome::Ok`.
    pub fn example(&self) -> String {
        if let Some(line) = &self.line {
            format!("{line} {EXAMPLE_BODY}")
        } else if let Some((open, close)) = &self.block {
            format!("{open} {EXAMPLE_BODY} {close}")
        } else if let Some(delim) = self.docstring.first() {
            format!("{delim}{EXAMPLE_BODY}{delim}")
        } else {
            EXAMPLE_BODY.to_string()
        }
    }
}

/// Fully resolved configuration. Extensions are indexed for O(1) lookup.
#[derive(Debug, Clone)]
pub struct Config {
    pub display: Display,
    pub limits: Limits,
    // Architectural `[rules]` are a strict-check concern the internal crate consumes; kept crate-private so making `Config` a public type does not leak the internal `Rules` shape into the library API (the low-level walk/annotation consumer never needs it).
    pub(crate) rules: Rules,
    languages: Vec<Language>,
    ext_to_lang: HashMap<String, usize>,
}

impl Config {
    /// Load defaults, overlay the user file, overlay the nearest repo file found
    /// by walking up from `root`, then apply CLI overrides.
    pub fn load(root: &Path, cli: &CliOverrides) -> Result<Config> {
        let mut raw: RawConfig =
            toml::from_str(DEFAULT_CONFIG).context("built-in default config is invalid")?;

        if let Some(user) = user_config_path() {
            merge(&mut raw, read_layer(&user)?);
        }

        let repo_path = match &cli.config_file {
            Some(explicit) => Some(explicit.clone()),
            None => find_repo_config(root),
        };
        if let Some(path) = repo_path {
            merge(&mut raw, read_layer(&path)?);
        }

        resolve(raw, cli)
    }

    /// The language matching `path`'s extension, or `None` for an extensionless or
    /// unknown-extension file. Owns the dotted-lowercase key normalization in ONE
    /// place, so walk/model/strict never re-derive `format!(".{}", ext.to_lowercase())`.
    pub fn language_for_path(&self, path: &Path) -> Option<&Language> {
        let key = ext_key(path)?;
        self.language_for_extension(&key)
    }

    /// Whether `path`'s extension maps to a known language (the walk's file filter).
    pub fn known_for_path(&self, path: &Path) -> bool {
        ext_key(path).is_some_and(|key| self.is_known_extension(&key))
    }

    fn language_for_extension(&self, ext: &str) -> Option<&Language> {
        self.ext_to_lang.get(ext).map(|&i| &self.languages[i])
    }

    fn is_known_extension(&self, ext: &str) -> bool {
        self.ext_to_lang.contains_key(ext)
    }
}

/// The canonical extension lookup key for a path: the extension lowercased and
/// dotted (`Foo.PY` -> `.py`). `None` for a path with no extension.
fn ext_key(path: &Path) -> Option<String> {
    path.extension()
        .and_then(|e| e.to_str())
        .map(|e| format!(".{}", e.to_lowercase()))
}

fn read_layer(path: &Path) -> Result<RawConfig> {
    let text = std::fs::read_to_string(path)
        .with_context(|| format!("reading config {}", path.display()))?;
    toml::from_str(&text).with_context(|| format!("parsing config {}", path.display()))
}

/// Overlay `src` onto `dst`: any field set in `src` wins; languages merge by key.
fn merge(dst: &mut RawConfig, src: RawConfig) {
    if let Some(sd) = src.display {
        let dd = dst.display.get_or_insert_with(Default::default);
        dd.show_age = sd.show_age.or(dd.show_age);
        dd.ascii = sd.ascii.or(dd.ascii);
        dd.gitignore = sd.gitignore.or(dd.gitignore);
        dd.hidden = sd.hidden.or(dd.hidden);
        dd.include_tests = sd.include_tests.or(dd.include_tests);
        dd.max_per_node = sd.max_per_node.or(dd.max_per_node);
        // `include` is a whole list, so a layer that sets it REPLACES (not appends) — the same precedence `[rules] deny` uses, so a repo file can fully re-state the selectors rather than inherit a user file's. CLI selectors are folded in additively later, in `resolve`.
        dd.include = sd.include.or_else(|| dd.include.take());
    }
    if let Some(sl) = src.limits {
        let dl = dst.limits.get_or_insert_with(Default::default);
        dl.max_files = sl.max_files.or(dl.max_files);
    }
    if let Some(sr) = src.rules {
        let dr = dst.rules.get_or_insert_with(Default::default);
        // `deny` is a whole list, so a layer that sets it replaces (not appends); the flags overlay per the standard `.or()` precedence.
        dr.deny = sr.deny.or_else(|| dr.deny.take());
        dr.forbid_cycles = sr.forbid_cycles.or(dr.forbid_cycles);
        dr.forbid_orphans = sr.forbid_orphans.or(dr.forbid_orphans);
        dr.require_package_charter = sr.require_package_charter.or(dr.require_package_charter);
        dr.max_annotation_length = sr.max_annotation_length.or(dr.max_annotation_length);
    }
    for (name, lang) in src.languages {
        dst.languages.insert(name, lang);
    }
}

fn resolve(raw: RawConfig, cli: &CliOverrides) -> Result<Config> {
    let disp = raw.display.unwrap_or_default();
    // Selectors are config-first, then CLI: a config `[display] include` sets a baseline and each `--include` on the command line ADDS to it, so a run can widen the tree beyond what the repo file already opts in without having to re-state it.
    let mut include = disp.include.clone().unwrap_or_default();
    include.extend(cli.include.iter().cloned());
    let display = Display {
        show_age: cli.show_age.or(disp.show_age).unwrap_or(false),
        ascii: cli.ascii.or(disp.ascii).unwrap_or(false),
        gitignore: cli.gitignore.or(disp.gitignore).unwrap_or(true),
        hidden: cli.hidden.or(disp.hidden).unwrap_or(false),
        include_tests: cli.include_tests.or(disp.include_tests).unwrap_or(false),
        max_per_node: resolve_max_per_node(cli, disp.max_per_node),
        include,
    };

    let limits = Limits {
        max_files: resolve_max_files(cli, raw.limits.unwrap_or_default())?,
    };

    let rules = resolve_rules(raw.rules.unwrap_or_default(), cli);

    let mut languages = Vec::new();
    let mut ext_to_lang = HashMap::new();
    // Deterministic order so diagnostics and any future listing are stable.
    let mut entries: Vec<(String, RawLanguage)> = raw.languages.into_iter().collect();
    entries.sort_by(|a, b| a.0.cmp(&b.0));

    for (name, lang) in entries {
        let idx = languages.len();
        for ext in &lang.extensions {
            let key = ext.strip_prefix('.').unwrap_or(ext).to_lowercase();
            ext_to_lang.insert(format!(".{key}"), idx);
        }
        languages.push(to_language(name, lang)?);
    }

    Ok(Config {
        display,
        limits,
        rules,
        languages,
        ext_to_lang,
    })
}

/// One `[languages.*]` entry resolved into a [`Language`]. Its own function so the raw shape is
/// turned into the resolved one in ONE place, whether the entry arrives from a merged layer through
/// [`resolve`] or straight from the built-in table through [`builtin_markdown`].
fn to_language(name: String, raw: RawLanguage) -> Result<Language> {
    let pattern = match &raw.pattern {
        Some(p) => Some(
            Regex::new(p)
                .with_context(|| format!("language '{name}': invalid extraction pattern `{p}`"))?,
        ),
        None => None,
    };
    Ok(Language {
        name,
        line: raw.comment,
        block: raw.block.map(|[open, close]| (open, close)),
        docstring: raw.docstring.unwrap_or_default(),
        pattern,
    })
}

/// The Markdown [`Language`] as the BUILT-IN table defines it, for reading a document compiled into
/// the binary. Derived from [`DEFAULT_CONFIG`] rather than restated, so it cannot drift from the
/// built-in grammar the gate starts from before a user's file layers over it.
/// Reads nothing: the table is compiled in, as [`builtin_example`] is.
pub(crate) fn builtin_markdown() -> Language {
    let mut raw: RawConfig =
        toml::from_str(DEFAULT_CONFIG).expect("built-in default config is valid TOML");
    let markdown = raw
        .languages
        .remove("markdown")
        .expect("built-in default config defines a markdown language");
    to_language("markdown".to_string(), markdown)
        .expect("built-in markdown language needs no pattern to compile")
}

/// A representative conformant annotation line for `--help`'s ANNOTATION FORMAT block: the
/// canonical [`EXAMPLE_BODY`] with the default `//` marker, the help text separately noting how the
/// marker varies. Derived from the same body every language's [`Language::example`] wraps, so
/// `--help` and `--strict-check` cannot advertise different exemplars.
pub fn builtin_example() -> String {
    format!("// {EXAMPLE_BODY}")
}

/// Resolve the `[rules]` table. Takes the CLI overrides because `max_annotation_length` has a
/// `--max-length` flag, which must win over the merged layers like every other CLI value. A
/// resolved bound of `0` normalizes to `None`, as [`resolve_max_per_node`] does: an empty field is
/// already fatal, so a literal 0 could only mean "fail everything" — and it is how you switch it off.
fn resolve_rules(raw: RawRules, cli: &CliOverrides) -> Rules {
    Rules {
        deny: raw
            .deny
            .unwrap_or_default()
            .into_iter()
            .map(|[from, to]| (from, to))
            .collect(),
        forbid_cycles: raw.forbid_cycles.unwrap_or(false),
        forbid_orphans: raw.forbid_orphans.unwrap_or(false),
        require_package_charter: raw.require_package_charter.unwrap_or(false),
        max_annotation_length: cli
            .max_annotation_length
            .or(raw.max_annotation_length)
            .filter(|&n| n > 0),
    }
}

/// Resolve the runaway-scope cap. Precedence: CLI, then env
/// `ANNOTATED_TREE_MAX_FILES`, then config file, then built-in default. `None`
/// means "no cap" (only reachable via `--no-limit`, since the built-in default
/// always supplies a value).
fn resolve_max_files(cli: &CliOverrides, config_limits: RawLimits) -> Result<Option<usize>> {
    if let Some(cli_choice) = cli.max_files {
        return Ok(cli_choice);
    }
    if let Some(raw) = std::env::var_os("ANNOTATED_TREE_MAX_FILES") {
        let text = raw.to_string_lossy();
        let n: usize = text
            .trim()
            .parse()
            .with_context(|| format!("ANNOTATED_TREE_MAX_FILES is not a valid count: `{text}`"))?;
        return Ok(Some(n));
    }
    Ok(config_limits.max_files)
}

/// Resolve the per-directory display cap. Precedence: CLI, then config file, then
/// built-in default. No env var (a display setting, unlike `max_files`). `0` is
/// normalized to `None` (unlimited) so `--max-per-node 0` disables the cap the same
/// way `--full` does; `None` otherwise only arises via `--full`.
fn resolve_max_per_node(cli: &CliOverrides, config_value: Option<usize>) -> Option<usize> {
    let resolved = match cli.max_per_node {
        Some(cli_choice) => cli_choice,
        None => config_value,
    };
    resolved.filter(|&n| n > 0)
}

fn user_config_path() -> Option<PathBuf> {
    let env_dir = |key: &str| {
        std::env::var_os(key)
            .map(PathBuf::from)
            .filter(|p| !p.as_os_str().is_empty())
    };
    // XDG (explicit) > ~/.config (unix) > %APPDATA% (windows).
    let base = env_dir("XDG_CONFIG_HOME")
        .or_else(|| env_dir("HOME").map(|h| h.join(".config")))
        .or_else(|| env_dir("APPDATA"))?;
    let path = base.join("annotated-tree").join("config.toml");
    path.is_file().then_some(path)
}

/// Walk up from `start` looking for `.annotated-tree.toml`, git-style.
fn find_repo_config(start: &Path) -> Option<PathBuf> {
    let start = start.canonicalize().unwrap_or_else(|_| start.to_path_buf());
    for dir in start.ancestors() {
        let candidate = dir.join(".annotated-tree.toml");
        if candidate.is_file() {
            return Some(candidate);
        }
    }
    None
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The advertised annotation format must provably pass the lint it advertises:
    /// every built-in language's `example` (shown in `--help` and every strict-check
    /// diagnostic) round-trips through the real extractor+validator as `Outcome::Ok`.
    /// This is the DbC guarantee against advertise-vs-enforce drift.
    #[test]
    fn builtin_examples_are_self_conforming() {
        let raw: RawConfig = toml::from_str(DEFAULT_CONFIG).expect("default config parses");
        let config = resolve(raw, &CliOverrides::default()).expect("default config resolves");
        for lang in &config.languages {
            let example = lang.example();
            assert_eq!(
                crate::annotation::analyze(&example, lang, None),
                crate::annotation::Outcome::Ok,
                "language '{}' example is not self-conforming: {:?}",
                lang.name,
                example,
            );
        }
    }

    #[test]
    fn builtin_example_matches_rust_derived() {
        // `--help` sources its exemplar from `builtin_example()`; it must equal the `//` (Rust/Go/TS) language's DERIVED example, so help and the per-file diagnostic advertise the same body from the one `EXAMPLE_BODY` source.
        let raw: RawConfig = toml::from_str(DEFAULT_CONFIG).unwrap();
        let config = resolve(raw, &CliOverrides::default()).unwrap();
        let rust = config
            .languages
            .iter()
            .find(|l| l.name == "rust")
            .expect("rust language present");
        assert_eq!(builtin_example(), rust.example());
    }
}
