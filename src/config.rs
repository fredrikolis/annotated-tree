// Config: Resolves layered configuration (built-in < user < repo < CLI) into a language table and display settings. NOT concerned with walking or rendering. | I/O: (paths, CLI overrides) -> Config

use std::collections::HashMap;
use std::path::{Path, PathBuf};

use anyhow::{Context, Result};
use regex::Regex;
use serde::Deserialize;

use crate::rules::Rules;

const DEFAULT_CONFIG: &str = include_str!("../default_config.toml");

/// The raw, all-optional shape parsed from a TOML layer. Every layer omits most
/// fields; merging overlays later layers onto earlier ones.
#[derive(Debug, Default, Deserialize)]
#[serde(deny_unknown_fields)]
struct RawConfig {
    convention: Option<RawConvention>,
    display: Option<RawDisplay>,
    limits: Option<RawLimits>,
    rules: Option<RawRules>,
    #[serde(default)]
    languages: HashMap<String, RawLanguage>,
}

/// Architectural dependency rules parsed from a `[rules]` table. Declarative and
/// regex-free: `deny` names package pairs, the flags toggle structural checks.
#[derive(Debug, Default, Clone, Deserialize)]
#[serde(deny_unknown_fields)]
struct RawRules {
    deny: Option<Vec<[String; 2]>>,
    forbid_cycles: Option<bool>,
    forbid_orphans: Option<bool>,
}

/// Walk-scope limits parsed from a `[limits]` table. Deliberately separate from
/// `[display]`: a runaway-scope cap bounds the walk, it is not a rendering choice.
#[derive(Debug, Default, Clone, Deserialize)]
#[serde(deny_unknown_fields)]
struct RawLimits {
    max_files: Option<usize>,
}

#[derive(Debug, Default, Deserialize)]
#[serde(deny_unknown_fields)]
struct RawConvention {
    require: Option<String>,
    hint: Option<String>,
}

#[derive(Debug, Default, Clone, Deserialize)]
#[serde(deny_unknown_fields)]
struct RawDisplay {
    show_age: Option<bool>,
    show_tokens: Option<bool>,
    show_symbols: Option<bool>,
    ascii: Option<bool>,
    gitignore: Option<bool>,
    include_tests: Option<bool>,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(deny_unknown_fields)]
struct RawLanguage {
    extensions: Vec<String>,
    comment: Option<String>,
    block: Option<[String; 2]>,
    docstring: Option<Vec<String>>,
    pattern: Option<String>,
    require: Option<String>,
    hint: Option<String>,
}

/// CLI-supplied overrides. `None` means "not specified; keep the merged value".
#[derive(Debug, Default)]
pub struct CliOverrides {
    pub show_age: Option<bool>,
    pub show_tokens: Option<bool>,
    pub show_symbols: Option<bool>,
    pub ascii: Option<bool>,
    pub gitignore: Option<bool>,
    pub include_tests: Option<bool>,
    pub config_file: Option<PathBuf>,
    /// Runaway-scope cap override, modelled as an `Option<Option<usize>>`:
    /// `None` = the CLI said nothing (fall through to env/config/default);
    /// `Some(None)` = `--no-limit`/`--force` (cap disabled);
    /// `Some(Some(n))` = `--max-files n`.
    pub max_files: Option<Option<usize>>,
}

#[derive(Debug, Clone)]
pub struct Display {
    pub show_age: bool,
    pub show_tokens: bool,
    pub show_symbols: bool,
    pub ascii: bool,
    pub gitignore: bool,
    pub include_tests: bool,
}

/// Walk-scope limits. `max_files: None` means "no cap". Kept out of `Display`
/// (SoC): these bound the walk, not the rendered output.
#[derive(Debug, Clone)]
pub struct Limits {
    pub max_files: Option<usize>,
}

/// How a single language's first-line annotation is located and validated.
#[derive(Debug, Clone)]
pub struct Language {
    pub name: String,
    pub line: Option<String>,
    pub block: Option<(String, String)>,
    pub docstring: Vec<String>,
    pub pattern: Option<Regex>,
    pub require: Regex,
    pub hint: String,
}

/// Fully resolved configuration. Extensions are indexed for O(1) lookup.
#[derive(Debug, Clone)]
pub struct Config {
    pub display: Display,
    pub limits: Limits,
    pub rules: Rules,
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
    if let Some(sc) = src.convention {
        let dc = dst.convention.get_or_insert_with(Default::default);
        if sc.require.is_some() {
            dc.require = sc.require;
        }
        if sc.hint.is_some() {
            dc.hint = sc.hint;
        }
    }
    if let Some(sd) = src.display {
        let dd = dst.display.get_or_insert_with(Default::default);
        dd.show_age = sd.show_age.or(dd.show_age);
        dd.show_tokens = sd.show_tokens.or(dd.show_tokens);
        dd.show_symbols = sd.show_symbols.or(dd.show_symbols);
        dd.ascii = sd.ascii.or(dd.ascii);
        dd.gitignore = sd.gitignore.or(dd.gitignore);
        dd.include_tests = sd.include_tests.or(dd.include_tests);
    }
    if let Some(sl) = src.limits {
        let dl = dst.limits.get_or_insert_with(Default::default);
        dl.max_files = sl.max_files.or(dl.max_files);
    }
    if let Some(sr) = src.rules {
        let dr = dst.rules.get_or_insert_with(Default::default);
        // `deny` is a whole list, so a layer that sets it replaces (not appends);
        // the flags overlay per the standard `.or()` precedence.
        dr.deny = sr.deny.or_else(|| dr.deny.take());
        dr.forbid_cycles = sr.forbid_cycles.or(dr.forbid_cycles);
        dr.forbid_orphans = sr.forbid_orphans.or(dr.forbid_orphans);
    }
    for (name, lang) in src.languages {
        dst.languages.insert(name, lang);
    }
}

fn resolve(raw: RawConfig, cli: &CliOverrides) -> Result<Config> {
    let convention = raw.convention.unwrap_or_default();
    let default_require_src = convention
        .require
        .context("resolved config is missing [convention].require")?;
    let default_hint = convention
        .hint
        .unwrap_or_else(|| default_require_src.clone());

    let disp = raw.display.unwrap_or_default();
    let display = Display {
        show_age: cli.show_age.or(disp.show_age).unwrap_or(false),
        show_tokens: cli.show_tokens.or(disp.show_tokens).unwrap_or(false),
        show_symbols: cli.show_symbols.or(disp.show_symbols).unwrap_or(false),
        ascii: cli.ascii.or(disp.ascii).unwrap_or(false),
        gitignore: cli.gitignore.or(disp.gitignore).unwrap_or(true),
        include_tests: cli.include_tests.or(disp.include_tests).unwrap_or(false),
    };

    let limits = Limits {
        max_files: resolve_max_files(cli, raw.limits.unwrap_or_default())?,
    };

    let rules = resolve_rules(raw.rules.unwrap_or_default());

    let mut languages = Vec::new();
    let mut ext_to_lang = HashMap::new();
    // Deterministic order so diagnostics and any future listing are stable.
    let mut entries: Vec<(String, RawLanguage)> = raw.languages.into_iter().collect();
    entries.sort_by(|a, b| a.0.cmp(&b.0));

    for (name, lang) in entries {
        let require_src = lang.require.as_deref().unwrap_or(&default_require_src);
        let require = Regex::new(require_src)
            .with_context(|| format!("language '{name}': invalid require regex `{require_src}`"))?;
        let hint = lang.hint.clone().unwrap_or_else(|| default_hint.clone());
        let pattern =
            match &lang.pattern {
                Some(p) => Some(Regex::new(p).with_context(|| {
                    format!("language '{name}': invalid extraction pattern `{p}`")
                })?),
                None => None,
            };
        let block = lang.block.map(|[open, close]| (open, close));

        let idx = languages.len();
        for ext in &lang.extensions {
            let key = ext.strip_prefix('.').unwrap_or(ext).to_lowercase();
            ext_to_lang.insert(format!(".{key}"), idx);
        }
        languages.push(Language {
            name,
            line: lang.comment,
            block,
            docstring: lang.docstring.unwrap_or_default(),
            pattern,
            require,
            hint,
        });
    }

    Ok(Config {
        display,
        limits,
        rules,
        languages,
        ext_to_lang,
    })
}

fn resolve_rules(raw: RawRules) -> Rules {
    Rules {
        deny: raw
            .deny
            .unwrap_or_default()
            .into_iter()
            .map(|[from, to]| (from, to))
            .collect(),
        forbid_cycles: raw.forbid_cycles.unwrap_or(false),
        forbid_orphans: raw.forbid_orphans.unwrap_or(false),
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
