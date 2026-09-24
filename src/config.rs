// Concern: resolves the layered configuration, and answers which language a path is | Non-concern: walking or rendering | IO: (config paths, CLI overrides, shebang heads) -> Config, Language

use std::collections::HashMap;
use std::path::{Path, PathBuf};

use anyhow::{Context, Result};
use clap::ValueEnum;
use regex::Regex;
use serde::Deserialize;

use crate::rules::Rules;

const DEFAULT_CONFIG: &str = include_str!("default_config.toml");

/// `[display] format` can win over an absent `--format`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, ValueEnum, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Format {
    #[default]
    Text,
    Json,
    Md,
}

#[derive(Debug, Default, Deserialize)]
#[serde(deny_unknown_fields)]
struct RawConfig {
    display: Option<RawDisplay>,
    limits: Option<RawLimits>,
    rules: Option<RawRules>,
    #[serde(default)]
    languages: HashMap<String, RawLanguage>,
}

#[derive(Debug, Default, Clone, Deserialize)]
#[serde(deny_unknown_fields)]
struct RawRules {
    deny: Option<Vec<[String; 2]>>,
    forbid_cycles: Option<bool>,
    forbid_orphans: Option<bool>,
    require_package_charter: Option<bool>,
    max_annotation_length: Option<usize>,
}

#[derive(Debug, Default, Clone, Deserialize)]
#[serde(deny_unknown_fields)]
struct RawLimits {
    max_files: Option<usize>,
    max_depth: Option<usize>,
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
    format: Option<Format>,
    ignore: Option<Vec<String>>,
    ignore_parsing_errors: Option<bool>,
    no_guide: Option<bool>,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(deny_unknown_fields)]
struct RawLanguage {
    extensions: Vec<String>,
    interpreters: Option<Vec<String>>,
    comment: Option<String>,
    block: Option<[String; 2]>,
    docstring: Option<Vec<String>>,
    pattern: Option<String>,
}

/// `None` means "not specified; keep the merged value".
#[derive(Debug, Default, Clone)]
pub struct CliOverrides {
    pub show_age: Option<bool>,
    pub ascii: Option<bool>,
    pub gitignore: Option<bool>,
    pub hidden: Option<bool>,
    pub include_tests: Option<bool>,
    /// ADDITIVE to any config `[display] include`, not a replacement.
    pub include: Vec<String>,
    /// ADDITIVE, like `include`.
    pub ignore: Vec<String>,
    pub max_depth: Option<usize>,
    /// `Some` only when `--format` actually appeared.
    pub format: Option<Format>,
    pub ignore_parsing_errors: Option<bool>,
    pub no_guide: Option<bool>,
    pub config_file: Option<PathBuf>,
    /// `None` = fall through to env/config/default; `Some(None)` = `--no-limit`/`--force`;
    /// `Some(Some(n))` = `--max-files n`.
    pub max_files: Option<Option<usize>>,
    /// Same `Option<Option<usize>>` shape as `max_files`; `Some(None)` = `--full`.
    pub max_per_node: Option<Option<usize>>,
    /// `0` normalizes to "no bound", same as `--max-per-node 0`.
    pub max_annotation_length: Option<usize>,
}

#[derive(Debug, Clone)]
pub struct Display {
    pub show_age: bool,
    pub ascii: bool,
    pub gitignore: bool,
    /// Independent of `gitignore`; `.git` is walked under neither.
    pub hidden: bool,
    pub include_tests: bool,
    pub max_per_node: Option<usize>,
    /// A file is listed when its extension maps to a known language OR matches one of these.
    pub include: Vec<String>,
    /// A matching path is pruned from the walk.
    pub ignore: Vec<String>,
    pub format: Format,
    pub ignore_parsing_errors: bool,
    pub no_guide: bool,
}

#[derive(Debug, Clone)]
pub struct Limits {
    pub max_files: Option<usize>,
    pub max_depth: Option<usize>,
}

/// Configures only HOW to find the first comment (markers / an escape-hatch `pattern`); the
/// annotation FORMAT itself is invariant, validated in [`crate::annotation`].
#[derive(Debug, Clone)]
pub struct Language {
    pub name: String,
    /// The only way an EXTENSIONLESS file resolves — a git hook or `bin/` script.
    pub interpreters: Vec<String>,
    /// True where line 1 is spoken for by a metadata block to scan past; false where `---`
    /// is the file's own content.
    pub frontmatter_prefix: bool,
    pub line: Option<String>,
    pub block: Option<(String, String)>,
    pub docstring: Vec<String>,
    pub pattern: Option<Regex>,
}

/// A concrete, self-conforming annotation body. Per-language examples DERIVE from this
/// plus the language's marker, never configured.
const EXAMPLE_BODY: &str = "Concern: runs the core loop | Non-concern: transport | IO: (Job) -> Result";

impl Language {
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

#[derive(Debug, Clone)]
pub struct Config {
    pub display: Display,
    pub limits: Limits,
    // crate-private so `Config` being public doesn't leak `Rules` into the library API.
    pub(crate) rules: Rules,
    languages: Vec<Language>,
    ext_to_lang: HashMap<String, usize>,
    interp_to_lang: HashMap<String, usize>,
}

impl Config {
    pub fn load(root: &Path, cli: &CliOverrides) -> Result<Config> {
        let mut raw: RawConfig = toml::from_str(DEFAULT_CONFIG).context("built-in default config is invalid")?;

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

    pub fn language_for_path(&self, path: &Path) -> Option<&Language> {
        match ext_key(path) {
            Some(key) => self.language_for_extension(&key),
            // An extension always wins; the shebang probe fires only when there's none to consult.
            None => self.language_for_shebang(path),
        }
    }

    pub fn known_for_path(&self, path: &Path) -> bool {
        self.language_for_path(path).is_some()
    }

    fn language_for_extension(&self, ext: &str) -> Option<&Language> {
        self.ext_to_lang.get(ext).map(|&i| &self.languages[i])
    }

    fn language_for_shebang(&self, path: &Path) -> Option<&Language> {
        let word = interpreter_word(&read_shebang(path)?)?;
        self.interp_to_lang.get(&interp_key(&word)).map(|&i| &self.languages[i])
    }
}

const SHEBANG_BYTES: usize = 256;

/// Offset 0 only — a `#!` further down is a comment, not a shebang.
fn read_shebang(path: &Path) -> Option<String> {
    use std::io::Read;
    let mut buf = [0u8; SHEBANG_BYTES];
    let read = std::fs::File::open(path).ok()?.read(&mut buf).ok()?;
    let head = String::from_utf8_lossy(&buf[..read]);
    let line = head.strip_prefix("#!")?.lines().next()?;
    Some(line.to_string())
}

/// `env`'s own word is skipped: `#!/usr/bin/env python3` resolves as `#!/usr/bin/python3` does.
fn interpreter_word(shebang: &str) -> Option<String> {
    let mut words = shebang.split_whitespace();
    let first = words.next()?;
    if interp_key(first) != "env" {
        return Some(first.to_string());
    }
    words.next().map(str::to_string)
}

fn interp_key(word: &str) -> String {
    word.rsplit_once('/').map_or(word, |(_, base)| base).to_lowercase()
}

fn ext_key(path: &Path) -> Option<String> {
    path.extension().and_then(|e| e.to_str()).map(|e| format!(".{}", e.to_lowercase()))
}

fn ext_table_key(ext: &str) -> String {
    format!(".{}", ext.strip_prefix('.').unwrap_or(ext).to_lowercase())
}

fn read_layer(path: &Path) -> Result<RawConfig> {
    let text = std::fs::read_to_string(path).with_context(|| format!("reading config {}", path.display()))?;
    toml::from_str(&text).with_context(|| format!("parsing config {}", path.display()))
}

fn merge(dst: &mut RawConfig, src: RawConfig) {
    if let Some(sd) = src.display {
        let dd = dst.display.get_or_insert_with(Default::default);
        dd.show_age = sd.show_age.or(dd.show_age);
        dd.ascii = sd.ascii.or(dd.ascii);
        dd.gitignore = sd.gitignore.or(dd.gitignore);
        dd.hidden = sd.hidden.or(dd.hidden);
        dd.include_tests = sd.include_tests.or(dd.include_tests);
        dd.max_per_node = sd.max_per_node.or(dd.max_per_node);
        // A whole list REPLACES, not appends; CLI selectors are folded in additively later.
        dd.include = sd.include.or_else(|| dd.include.take());
        dd.ignore = sd.ignore.or_else(|| dd.ignore.take());
        dd.format = sd.format.or(dd.format);
        dd.ignore_parsing_errors = sd.ignore_parsing_errors.or(dd.ignore_parsing_errors);
        dd.no_guide = sd.no_guide.or(dd.no_guide);
    }
    if let Some(sl) = src.limits {
        let dl = dst.limits.get_or_insert_with(Default::default);
        dl.max_files = sl.max_files.or(dl.max_files);
    }
    if let Some(sr) = src.rules {
        let dr = dst.rules.get_or_insert_with(Default::default);
        // A whole list replaces, not appends.
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
    // Config-first, then CLI additively.
    let mut include = disp.include.clone().unwrap_or_default();
    include.extend(cli.include.iter().cloned());
    let mut ignore = disp.ignore.clone().unwrap_or_default();
    ignore.extend(cli.ignore.iter().cloned());
    let display = Display { show_age: cli.show_age.or(disp.show_age).unwrap_or(false), ascii: cli.ascii.or(disp.ascii).unwrap_or(false), gitignore: cli.gitignore.or(disp.gitignore).unwrap_or(true), hidden: cli.hidden.or(disp.hidden).unwrap_or(false), include_tests: cli.include_tests.or(disp.include_tests).unwrap_or(false), max_per_node: resolve_max_per_node(cli, disp.max_per_node), include, ignore, format: cli.format.or(disp.format).unwrap_or(Format::Text), ignore_parsing_errors: cli.ignore_parsing_errors.or(disp.ignore_parsing_errors).unwrap_or(false), no_guide: cli.no_guide.or(disp.no_guide).unwrap_or(false) };

    let raw_limits = raw.limits.unwrap_or_default();
    let limits = Limits { max_files: resolve_max_files(cli, raw_limits.clone())?, max_depth: resolve_max_depth(cli, raw_limits.max_depth) };

    let rules = resolve_rules(raw.rules.unwrap_or_default(), cli);

    let mut languages = Vec::new();
    let mut ext_to_lang = HashMap::new();
    let mut interp_to_lang = HashMap::new();
    let mut entries: Vec<(String, RawLanguage)> = raw.languages.into_iter().collect();
    entries.sort_by(|a, b| a.0.cmp(&b.0));

    for (name, lang) in entries {
        let idx = languages.len();
        for ext in &lang.extensions {
            ext_to_lang.insert(ext_table_key(ext), idx);
        }
        let language = to_language(name, lang)?;
        for interp in &language.interpreters {
            interp_to_lang.insert(interp_key(interp), idx);
        }
        languages.push(language);
    }

    Ok(Config { display, limits, rules, languages, ext_to_lang, interp_to_lang })
}

/// In YAML, `---` starts a DOCUMENT, not a metadata block to skip — else such a file would
/// certify as annotated off a comment below it.
const DOCUMENT_SEPARATOR_EXTENSIONS: &[&str] = &[".yml", ".yaml"];

fn to_language(name: String, raw: RawLanguage) -> Result<Language> {
    let pattern = match &raw.pattern {
        Some(p) => Some(Regex::new(p).with_context(|| format!("language '{name}': invalid extraction pattern `{p}`"))?),
        None => None,
    };
    let frontmatter_prefix = !raw.extensions.iter().any(|ext| DOCUMENT_SEPARATOR_EXTENSIONS.contains(&ext_table_key(ext).as_str()));
    Ok(Language { frontmatter_prefix, name, interpreters: raw.interpreters.unwrap_or_default(), line: raw.comment, block: raw.block.map(|[open, close]| (open, close)), docstring: raw.docstring.unwrap_or_default(), pattern })
}

/// Derived from [`DEFAULT_CONFIG`] so it cannot drift from the built-in grammar.
pub(crate) fn builtin_markdown() -> Language {
    let mut raw: RawConfig = toml::from_str(DEFAULT_CONFIG).expect("built-in default config is valid TOML");
    let markdown = raw.languages.remove("markdown").expect("built-in default config defines a markdown language");
    to_language("markdown".to_string(), markdown).expect("built-in markdown language needs no pattern to compile")
}

/// Derived from the same body every language's [`Language::example`] wraps, so `--help` and
/// `--strict-check` cannot advertise different exemplars.
pub fn builtin_example() -> String {
    format!("// {EXAMPLE_BODY}")
}

fn resolve_rules(raw: RawRules, cli: &CliOverrides) -> Rules {
    Rules { deny: raw.deny.unwrap_or_default().into_iter().map(|[from, to]| (from, to)).collect(), forbid_cycles: raw.forbid_cycles.unwrap_or(false), forbid_orphans: raw.forbid_orphans.unwrap_or(false), require_package_charter: raw.require_package_charter.unwrap_or(false), max_annotation_length: cli.max_annotation_length.or(raw.max_annotation_length).filter(|&n| n > 0) }
}

/// Precedence: CLI, then env `ANNOTATED_TREE_MAX_FILES`, then config, then built-in default.
fn resolve_max_files(cli: &CliOverrides, config_limits: RawLimits) -> Result<Option<usize>> {
    if let Some(cli_choice) = cli.max_files {
        return Ok(cli_choice);
    }
    if let Some(raw) = std::env::var_os("ANNOTATED_TREE_MAX_FILES") {
        let text = raw.to_string_lossy();
        let n: usize = text.trim().parse().with_context(|| format!("ANNOTATED_TREE_MAX_FILES is not a valid count: `{text}`"))?;
        return Ok(Some(n));
    }
    Ok(config_limits.max_files)
}

/// `0` normalizes to `None` (unlimited), so `--max-per-node 0` disables the cap like `--full` does.
fn resolve_max_per_node(cli: &CliOverrides, config_value: Option<usize>) -> Option<usize> {
    let resolved = match cli.max_per_node {
        Some(cli_choice) => cli_choice,
        None => config_value,
    };
    resolved.filter(|&n| n > 0)
}

/// `0` normalizes to `None` (uncapped) from either layer — `-L 0` and `[limits] max_depth = 0`
/// now agree; CLI still wins outright when given.
fn resolve_max_depth(cli: &CliOverrides, config_value: Option<usize>) -> Option<usize> {
    cli.max_depth.or(config_value).filter(|&n| n > 0)
}

fn user_config_path() -> Option<PathBuf> {
    let env_dir = |key: &str| std::env::var_os(key).map(PathBuf::from).filter(|p| !p.as_os_str().is_empty());
    // XDG (explicit) > ~/.config (unix) > %APPDATA% (windows).
    let base = env_dir("XDG_CONFIG_HOME").or_else(|| env_dir("HOME").map(|h| h.join(".config"))).or_else(|| env_dir("APPDATA"))?;
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

    #[test]
    fn builtin_examples_are_self_conforming() {
        let raw: RawConfig = toml::from_str(DEFAULT_CONFIG).expect("default config parses");
        let config = resolve(raw, &CliOverrides::default()).expect("default config resolves");
        for lang in &config.languages {
            let example = lang.example();
            assert_eq!(crate::annotation::analyze(&example, lang, None), crate::annotation::Outcome::Ok, "language '{}' example is not self-conforming: {:?}", lang.name, example,);
        }
    }

    #[test]
    fn a_shebang_resolves_only_an_extensionless_file() {
        let raw: RawConfig = toml::from_str(DEFAULT_CONFIG).expect("default config parses");
        let config = resolve(raw, &CliOverrides::default()).expect("default config resolves");
        let dir = std::env::temp_dir().join(format!("at-shebang-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).expect("mkdir fixture");

        let cases: &[(&str, &str, Option<&str>)] = &[("env-bash", "#!/usr/bin/env bash\n", Some("shell")), ("bin-sh", "#!/bin/sh\n", Some("shell")), ("env-python", "#!/usr/bin/env python3\n", Some("python")), ("env-node", "#!/usr/bin/env node\n", None), ("plain", "just text\n", None), ("late", "text\n#!/bin/sh\n", None), ("script.rs", "#!/bin/sh\nfn main() {}\n", Some("rust"))];
        for (name, body, expected) in cases {
            let path = dir.join(name);
            std::fs::write(&path, body).expect("write fixture");
            assert_eq!(config.language_for_path(&path).map(|l| l.name.as_str()), *expected, "{name}",);
        }
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn builtin_example_matches_rust_derived() {
        let raw: RawConfig = toml::from_str(DEFAULT_CONFIG).unwrap();
        let config = resolve(raw, &CliOverrides::default()).unwrap();
        let rust = config.languages.iter().find(|l| l.name == "rust").expect("rust language present");
        assert_eq!(builtin_example(), rust.example());
    }
}
