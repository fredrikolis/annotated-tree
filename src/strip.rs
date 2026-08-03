// Concern: removes conforming first-line annotations from named files and trees, in place | Non-concern: whether a file should carry one | IO: (paths, flags) -> report + edits

use std::io::Write;
use std::path::{Path, PathBuf};

use crate::annotation;
use crate::cli;
use crate::config::Config;
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
    /// The stable key an agent branches on, or `None` where the annotation went.
    const fn code(self) -> Option<&'static str> {
        match self {
            Self::Stripped | Self::AlreadyGone => None,
            Self::Excluded => Some("excluded"),
            Self::Symlink => Some("symlink"),
            Self::Unreadable => Some("unreadable"),
            Self::Unwritable => Some("unwritable"),
        }
    }

    /// A skip is reported but is not a failure, so it must not colour the exit code.
    const fn is_failure(self) -> bool {
        matches!(self, Self::Unreadable | Self::Unwritable)
    }
}

/// A file to consider, carrying the spelling the caller used. The canonical form dedups `./x`
/// against `x`; the given form is what the report echoes, so a caller can match a row to its input.
struct Target {
    given: PathBuf,
    canonical: PathBuf,
    /// Cached: link-ness is part of the dedup key, and cannot change mid-run.
    is_symlink: bool,
    /// Which resolved config governs this file. Pooling every target under one config would let
    /// argument order decide whether a language is recognized, and so whether a file is edited.
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

/// Strip conforming annotations from each named file, and from each named directory under `-R`.
/// Nothing is written without `-y`: the default reports and stops. There is no prompt, because a
/// Report that depended on a terminal would not be determined by its inputs.
pub fn dispatch(
    args: &cli::Strip,
    cli: &cli::Cli,
    out: &mut impl Write,
    err: &mut impl Write,
) -> i32 {
    let mut targets: Vec<Target> = Vec::new();
    let mut configs: Vec<Config> = Vec::new();
    // `-I` is a property of the invocation, not of a target, so it is compiled once.
    let excludes = match crate::util::build_globset(&cli.ignore) {
        Ok(g) => g,
        Err(e) => return precondition(out, err, cli, &e),
    };

    for path in &args.paths {
        // Each target resolves its OWN config, so a multi-target run never applies one tree's languages or ignore settings to another — the contract `--strict-check` already holds.
        let base = if path.is_dir() {
            path.clone()
        } else {
            path.parent()
                .filter(|p| !p.as_os_str().is_empty())
                .unwrap_or(Path::new("."))
                .to_path_buf()
        };
        let resolved = match Config::load(&base, &cli.overrides()) {
            Ok(c) => c,
            Err(e) => return precondition(out, err, cli, &e),
        };
        let include = match crate::util::build_globset(&resolved.display.include) {
            Ok(g) => g,
            Err(e) => return precondition(out, err, cli, &e),
        };
        let idx = configs.len();

        if path.is_dir() {
            if !args.recursive {
                let m = format!("{}: is a directory (use -R)", path.display());
                return fail(out, err, cli, exit::code::USAGE, exit::USAGE, &m);
            }
            match walk::collect_code_files(path, &resolved, &excludes, &include) {
                Ok(files) => targets.extend(files.into_iter().map(|f| Target::new(f, idx))),
                Err(_) => {
                    let m = format!("{}: exceeds the file cap", path.display());
                    return fail(
                        out,
                        err,
                        cli,
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
            return fail(out, err, cli, exit::code::NOT_FOUND, exit::PRECONDITION, &m);
        }
        configs.push(resolved);
    }

    // Keyed on link-ness too: a symlink and its target share a canonical path, so collapsing them would drop whichever the caller named second and skip the file it points at.
    targets.sort_by(|a, b| (&a.canonical, a.is_symlink).cmp(&(&b.canonical, b.is_symlink)));
    targets.dedup_by(|a, b| a.canonical == b.canonical && a.is_symlink == b.is_symlink);
    run(args, cli, &targets, &configs, &excludes, out, err)
}

/// Report only after the write pass, so a run that half-succeeded says so on the surface an agent
/// parses. Reporting the intended list up front would hand a JSON caller a success-shaped payload
/// whose exit code contradicts it.
fn run(
    args: &cli::Strip,
    cli: &cli::Cli,
    targets: &[Target],
    configs: &[Config],
    excludes: &globset::GlobSet,
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
        // `-I` narrows a named file too: honouring it only on the `-R` walk leaves the flag that exists to narrow a destructive verb inert on half its input.
        if target.is_symlink {
            results.push((target, Outcome::Symlink));
            continue;
        }
        if excluded(excludes, &target.given) {
            results.push((target, Outcome::Excluded));
            continue;
        }
        let config = &configs[target.config];
        match sole_annotation_line(&target.canonical, config) {
            Err(()) => results.push((target, Outcome::Unreadable)),
            Ok(None) => {}
            Ok(Some(_)) if !args.yes => results.push((target, Outcome::Stripped)),
            Ok(Some(_)) => results.push((target, strip_file(&target.canonical, config))),
        }
    }

    if artifacts > 0 {
        let _ = writeln!(
            err,
            "strip: delete those {artifacts} `.annotation` files to remove them"
        );
    }
    report(&results, args.yes, targets.len(), cli, out, err);
    if results.iter().any(|(_, o)| o.is_failure()) {
        exit::PRECONDITION
    } else {
        exit::SUCCESS
    }
}

/// Dual-rendered from one list: the JSON envelope every other surface emits, or prose.
fn report(
    results: &[(&Target, Outcome)],
    applied: bool,
    scanned: usize,
    cli: &cli::Cli,
    out: &mut impl Write,
    err: &mut impl Write,
) {
    if cli.format == cli::Format::Json {
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

/// One failure, rendered for whichever surface asked, so a JSON caller never reads emptiness as
/// success. The caller pairs the code with its exit integer; `exit.rs` owns that taxonomy.
fn fail(
    out: &mut impl Write,
    err: &mut impl Write,
    cli: &cli::Cli,
    code: &'static str,
    exit_code: i32,
    message: &str,
) -> i32 {
    if cli.format == cli::Format::Json {
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

/// `-I` matched against the file name and the path as written. `walk::keep_entry` matches the
/// ROOT-RELATIVE path where this sees the root-prefixed one, so a rooted glob can exclude here and
/// not there. That direction is the safe one for a destructive verb: it over-skips, never over-strips.
fn excluded(excludes: &globset::GlobSet, path: &Path) -> bool {
    !excludes.is_empty()
        && (path
            .file_name()
            .is_some_and(|n| excludes.is_match(Path::new(n)))
            || excludes.is_match(path))
}

/// The one shape every setup failure takes.
fn precondition(
    out: &mut impl Write,
    err: &mut impl Write,
    cli: &cli::Cli,
    e: &anyhow::Error,
) -> i32 {
    fail(
        out,
        err,
        cli,
        exit::code::PRECONDITION,
        exit::PRECONDITION,
        &format!("{e:#}"),
    )
}

/// A `.annotation` charter or `<name>.annotation` sidecar holds nothing but its annotation, so
/// removing it means deleting the file — a different act than editing one, left to the caller.
fn is_annotation_artifact(path: &Path) -> bool {
    path.file_name().is_some_and(|n| n == ".annotation")
        || path.extension().is_some_and(|e| e == "annotation")
}

/// The line to remove, read fresh. Every caller re-asks rather than trusting an earlier answer:
/// the same path can reach here twice, and the first removal invalidates the second's verdict. A
/// file whose comment grammar is unknown is never edited — knowing where a comment ends is the
/// whole safety argument.
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
    // Take one blank line with it, so the body does not open on the whitespace that separated it.
    if lines.get(at + 1).is_some_and(|l| l.trim().is_empty()) {
        lines.remove(at + 1);
    }
    lines.remove(at);
    write_atomically(path, &lines.join("\n"))
}

/// Written beside the file and renamed over it. A truncate-then-write interrupted midway leaves a
/// source file half-erased, which is worse than never having run; a rename either happens or does
/// not. The temp file is removed on either failure, so a partial write leaves nothing behind.
fn write_atomically(path: &Path, body: &str) -> Outcome {
    let tmp = path.with_file_name(format!(
        ".{}.strip-tmp",
        path.file_name().unwrap_or_default().to_string_lossy()
    ));
    if std::fs::write(&tmp, body).is_err() {
        let _ = std::fs::remove_file(&tmp);
        return Outcome::Unwritable;
    }
    // The rename carries the temp file's mode, so without this an executable script comes back unexecutable — a fresh file gets the process umask.
    if let Ok(meta) = std::fs::metadata(path) {
        let _ = std::fs::set_permissions(&tmp, meta.permissions());
    }
    if std::fs::rename(&tmp, path).is_err() {
        let _ = std::fs::remove_file(&tmp);
        return Outcome::Unwritable;
    }
    Outcome::Stripped
}
