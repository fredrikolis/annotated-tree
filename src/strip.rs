// Concern: removes conforming first-line annotations from named files and trees, in place | Non-concern: whether a file should carry one | IO: (paths, Config) -> report + edits

use std::io::Write;
use std::path::{Path, PathBuf};

use crate::annotation;
use crate::cli;
use crate::config::{CliOverrides, Config, Format};
use crate::exit;
use crate::walk;

/// One file's outcome, named so a no-op never reads as a failure.
#[derive(PartialEq, Eq, Clone, Copy)]
enum Outcome {
    Stripped,
    AlreadyGone,
    Excluded,
    Symlink,
    Unreadable,
    Unwritable,
}

impl Outcome {
    const fn code(self) -> Option<&'static str> {
        match self {
            Self::Stripped | Self::AlreadyGone => None,
            Self::Excluded => Some("excluded"),
            Self::Symlink => Some("symlink"),
            Self::Unreadable => Some("unreadable"),
            Self::Unwritable => Some("unwritable"),
        }
    }

    const fn is_failure(self) -> bool {
        matches!(self, Self::Unreadable | Self::Unwritable)
    }
}

/// The canonical form dedups `./x` against `x`; the given form is what the report echoes.
struct Target {
    given: PathBuf,
    canonical: PathBuf,
    is_symlink: bool,
    /// Each target keeps its own resolved config, so argument order never decides whether a
    /// language is recognized.
    config: usize,
}

impl Target {
    fn new(given: PathBuf, config: usize) -> Self {
        let canonical = given.canonicalize().unwrap_or_else(|_| given.clone());
        let is_symlink = given.is_symlink();
        Self {
            given,
            canonical,
            is_symlink,
            config,
        }
    }
}

/// A directory's own config governs it; a file's comes from its parent.
pub(crate) fn base_of(path: &Path) -> PathBuf {
    if path.is_dir() {
        path.to_path_buf()
    } else {
        path.parent()
            .filter(|p| !p.as_os_str().is_empty())
            .unwrap_or(Path::new("."))
            .to_path_buf()
    }
}

/// Nothing is written without `-y`: the default reports and stops. No prompt, since a report
/// that depended on a terminal would not be determined by its inputs.
pub fn dispatch(
    args: &cli::Strip,
    overrides: &CliOverrides,
    bootstrap_format: Format,
    out: &mut impl Write,
    err: &mut impl Write,
) -> i32 {
    let mut targets: Vec<Target> = Vec::new();
    let mut configs: Vec<Config> = Vec::new();
    let mut excludes: Vec<globset::GlobSet> = Vec::new();

    for path in &args.paths {
        let resolved = match Config::load(&base_of(path), overrides) {
            Ok(c) => c,
            Err(e) => {
                let format = configs
                    .first()
                    .map_or(bootstrap_format, |c| c.display.format);
                return precondition(out, err, format, &e);
            }
        };
        let format = configs
            .first()
            .map_or(resolved.display.format, |c| c.display.format);
        let path_excludes = match crate::util::build_globset(&resolved.display.ignore) {
            Ok(g) => g,
            Err(e) => return precondition(out, err, format, &e),
        };
        let include = match crate::util::build_globset(&resolved.display.include) {
            Ok(g) => g,
            Err(e) => return precondition(out, err, format, &e),
        };
        let idx = configs.len();

        if path.is_dir() {
            if !args.recursive {
                let m = format!("{}: is a directory (use -R)", path.display());
                return fail(out, err, format, exit::code::USAGE, exit::USAGE, &m);
            }
            match walk::collect_code_files(path, &resolved, &path_excludes, &include) {
                Ok(files) => targets.extend(files.into_iter().map(|f| Target::new(f, idx))),
                Err(_) => {
                    let m = format!("{}: exceeds the file cap", path.display());
                    return fail(
                        out,
                        err,
                        format,
                        exit::code::SCOPE_EXCEEDED,
                        exit::RUNAWAY_SCOPE,
                        &m,
                    );
                }
            }
        } else if path.is_file() || path.is_symlink() {
            targets.push(Target::new(path.clone(), idx));
        } else {
            let m = format!("{}: no such file or directory", path.display());
            return fail(
                out,
                err,
                format,
                exit::code::NOT_FOUND,
                exit::PRECONDITION,
                &m,
            );
        }
        configs.push(resolved);
        excludes.push(path_excludes);
    }

    targets.sort_by(|a, b| (&a.canonical, a.is_symlink).cmp(&(&b.canonical, b.is_symlink)));
    targets.dedup_by(|a, b| a.canonical == b.canonical && a.is_symlink == b.is_symlink);
    let format = configs[0].display.format;
    run(args, format, &targets, &configs, &excludes, out, err)
}

/// Report only after the write pass, so a run that half-succeeded says so on the surface parsed.
fn run(
    args: &cli::Strip,
    format: Format,
    targets: &[Target],
    configs: &[Config],
    excludes: &[globset::GlobSet],
    out: &mut impl Write,
    err: &mut impl Write,
) -> i32 {
    let mut results: Vec<(&Target, Outcome)> = Vec::new();
    let mut artifacts = 0usize;

    for target in targets {
        if is_annotation_artifact(&target.canonical) {
            artifacts += 1;
            let _ = writeln!(
                err,
                "strip: {}: whole file is its annotation",
                target.given.display()
            );
            continue;
        }
        if target.is_symlink {
            results.push((target, Outcome::Symlink));
            continue;
        }
        if excluded(&excludes[target.config], &target.given) {
            results.push((target, Outcome::Excluded));
            continue;
        }
        let target_config = &configs[target.config];
        match sole_annotation_line(&target.canonical, target_config) {
            Err(()) => results.push((target, Outcome::Unreadable)),
            Ok(None) => {}
            Ok(Some(_)) if !args.yes => results.push((target, Outcome::Stripped)),
            Ok(Some(_)) => results.push((target, strip_file(&target.canonical, target_config))),
        }
    }

    if artifacts > 0 {
        let _ = writeln!(
            err,
            "strip: delete those {artifacts} `.annotation` files to remove them"
        );
    }
    report(&results, args.yes, targets.len(), format, out, err);
    if results.iter().any(|(_, o)| o.is_failure()) {
        exit::PRECONDITION
    } else {
        exit::SUCCESS
    }
}

fn report(
    results: &[(&Target, Outcome)],
    applied: bool,
    scanned: usize,
    format: Format,
    out: &mut impl Write,
    err: &mut impl Write,
) {
    if format == Format::Json {
        let files: Vec<_> = results
            .iter()
            .map(|(t, o)| {
                serde_json::json!({ "path": t.given.display().to_string(), "code": o.code() })
            })
            .collect();
        let doc = serde_json::json!({
            "schema": 1,
            "strip": { "applied": applied, "scanned": scanned, "files": files },
        });
        let _ = writeln!(out, "{doc}");
        return;
    }
    for (target, outcome) in results {
        match outcome.code() {
            None => {
                let _ = writeln!(out, "{}", target.given.display());
            }
            Some(code) => {
                let _ = writeln!(err, "strip: {}: {code}", target.given.display());
            }
        }
    }
    let changed = results.iter().filter(|(_, o)| o.code().is_none()).count();
    let verb = if applied { "stripped" } else { "would strip" };
    let _ = writeln!(err, "strip: {verb} {changed} of {scanned} files scanned");
}

/// The caller pairs the code with its exit integer; `exit.rs` owns that taxonomy.
fn fail(
    out: &mut impl Write,
    err: &mut impl Write,
    format: Format,
    code: &'static str,
    exit_code: i32,
    message: &str,
) -> i32 {
    if format == Format::Json {
        let _ = writeln!(
            out,
            "{}",
            crate::render::json::render_error(code, message, None)
        );
    } else {
        let _ = writeln!(err, "strip: {message}");
    }
    exit_code
}

/// Matched against the file name AND the path as written, unlike `walk::keep_entry`'s
/// root-relative match — this direction over-skips rather than over-strips.
fn excluded(excludes: &globset::GlobSet, path: &Path) -> bool {
    !excludes.is_empty()
        && (path
            .file_name()
            .is_some_and(|n| excludes.is_match(Path::new(n)))
            || excludes.is_match(path))
}

fn precondition(
    out: &mut impl Write,
    err: &mut impl Write,
    format: Format,
    e: &anyhow::Error,
) -> i32 {
    fail(
        out,
        err,
        format,
        exit::code::PRECONDITION,
        exit::PRECONDITION,
        &format!("{e:#}"),
    )
}

/// Removing it means deleting the file — a different act than editing one, left to the caller.
fn is_annotation_artifact(path: &Path) -> bool {
    path.file_name().is_some_and(|n| n == ".annotation")
        || path.extension().is_some_and(|e| e == "annotation")
}

/// Read fresh every call: the same path can reach here twice, and the first removal
/// invalidates the second's verdict.
fn sole_annotation_line(path: &Path, config: &Config) -> Result<Option<(usize, String)>, ()> {
    let Some(lang) = config.language_for_path(path) else {
        return Ok(None);
    };
    let text = std::fs::read_to_string(path).map_err(|_| ())?;
    Ok(annotation::sole_annotation_line(&text, lang).map(|at| (at, text)))
}

fn strip_file(path: &Path, config: &Config) -> Outcome {
    let Ok(found) = sole_annotation_line(path, config) else {
        return Outcome::Unreadable;
    };
    let Some((at, text)) = found else {
        return Outcome::AlreadyGone;
    };
    let mut lines: Vec<&str> = text.split('\n').collect();
    // Take one blank line with it.
    if lines.get(at + 1).is_some_and(|l| l.trim().is_empty()) {
        lines.remove(at + 1);
    }
    lines.remove(at);
    write_atomically(path, &lines.join("\n"))
}

/// Written beside the file and renamed over it, so an interrupted write cannot leave the
/// source file half-erased.
fn write_atomically(path: &Path, body: &str) -> Outcome {
    let tmp = path.with_file_name(format!(
        ".{}.strip-tmp",
        path.file_name().unwrap_or_default().to_string_lossy()
    ));
    if std::fs::write(&tmp, body).is_err() {
        let _ = std::fs::remove_file(&tmp);
        return Outcome::Unwritable;
    }
    // Without this, an executable script comes back unexecutable — a fresh file gets the umask.
    if let Ok(meta) = std::fs::metadata(path) {
        let _ = std::fs::set_permissions(&tmp, meta.permissions());
    }
    if std::fs::rename(&tmp, path).is_err() {
        let _ = std::fs::remove_file(&tmp);
        return Outcome::Unwritable;
    }
    Outcome::Stripped
}
