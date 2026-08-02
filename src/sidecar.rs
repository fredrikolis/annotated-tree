// Concern: resolves `<name>.annotation` sidecars — which paths are sidecars, and what each annotates | Non-concern: the grammar or a directory's charter | IO: (path, Config) -> Option<path|body>

use std::path::{Path, PathBuf};

use crate::config::Config;
use crate::walk::CHARTER_FILE;

/// The sidecar path for `file`: `trials.csv` -> `trials.csv.annotation`. The suffix is the
/// SAME `.annotation` name a directory's charter uses — one metadata name at both scales — so
/// a reader who knows the folder breadcrumb already knows this one.
pub fn path_for(file: &Path) -> PathBuf {
    let mut name = file.file_name().unwrap_or_default().to_os_string();
    name.push(CHARTER_FILE);
    file.with_file_name(name)
}

/// The path a `<name>.annotation` file NAMES, whether or not that file exists. `None` when
/// `path` does not carry the suffix, and for the bare `.annotation` itself — that one is a
/// DIRECTORY charter (`crate::charter` owns it), not a sidecar for a file called "".
pub fn named_target(path: &Path) -> Option<PathBuf> {
    let name = path.file_name()?.to_str()?;
    let stem = name.strip_suffix(CHARTER_FILE)?;
    if stem.is_empty() {
        return None;
    }
    Some(path.with_file_name(stem))
}

/// The file `path` is a sidecar FOR, or `None` when it is not a sidecar at all.
///
/// A sidecar annotates only a file that maps to no comment marker. That is the whole reason
/// an Annotation's location stays determined by the path it annotates: a file that CAN hold a
/// first-line comment must, so there is never a second place to look and no precedence rule to
/// diagnose. `foo.rs.annotation` beside a `foo.rs` is therefore not a sidecar — it is an
/// ordinary file, and the tree lists it like any other. Neither is one whose named file is
/// absent: that is a dangling path, reported by `--strict-check`, not an annotation of
/// anything.
pub fn target_of(path: &Path, config: &Config) -> Option<PathBuf> {
    let target = named_target(path)?;
    let annotatable = target.is_file() && config.language_for_path(&target).is_none();
    annotatable.then_some(target)
}

/// Whether `file`'s contract lives in a sidecar beside it — it maps to no comment marker AND
/// a `<name>.annotation` file exists. This is what opts an otherwise-unlisted file (a CSV, a
/// dataset, a binary) into the tree: writing the sidecar IS the opt-in, so a reader needs no
/// `--include` to see the contract its author already wrote.
pub fn annotates(file: &Path, config: &Config) -> bool {
    config.language_for_path(file).is_none() && path_for(file).is_file()
}

/// The raw body of `file`'s sidecar, or `None` when there is no sidecar file. Read DIRECTLY,
/// never through the code-file walk, for the same reason [`crate::charter::read_charter_file`]
/// is: the walk deliberately hides this row. Returned untrimmed so a caller can tell an EMPTY
/// sidecar (an opt-in file with nothing in it — a defect `--strict-check` reports) from an
/// absent one.
pub fn body(file: &Path) -> Option<String> {
    std::fs::read_to_string(path_for(file)).ok()
}

/// Every `<name>.annotation` path directly inside `dir`, sorted. The lint pass needs the
/// sidecars the WALK does not yield — a dangling one names a file that does not exist, so
/// nothing in the file set points at it — and sorting keeps the report deterministic
/// regardless of directory order.
pub fn candidates_in(dir: &Path) -> Vec<PathBuf> {
    let Ok(entries) = std::fs::read_dir(dir) else {
        return Vec::new();
    };
    let mut out: Vec<PathBuf> = entries
        .flatten()
        // A DIRECTORY that happens to end in `.annotation` carries no annotation line, so it is not a candidate — and must not be reported as one that dangles.
        .filter(|e| e.file_type().is_ok_and(|t| t.is_file()))
        .map(|e| e.path())
        .filter(|p| named_target(p).is_some())
        .collect();
    out.sort();
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_bare_charter_file_is_not_a_sidecar() {
        // `.annotation` names the DIRECTORY it sits in, so it must never be read as a sidecar for a file called "" — the one case where the shared suffix could alias.
        assert_eq!(named_target(Path::new("a/.annotation")), None);
        assert_eq!(
            named_target(Path::new("a/trials.csv.annotation")),
            Some(PathBuf::from("a/trials.csv"))
        );
        assert_eq!(named_target(Path::new("a/trials.csv")), None);
    }

    #[test]
    fn path_for_appends_the_suffix_to_the_whole_name() {
        // The suffix is appended to the FULL file name, extension included, so `trials.csv` and `trials.json` cannot collide on one sidecar.
        assert_eq!(
            path_for(Path::new("a/trials.csv")),
            PathBuf::from("a/trials.csv.annotation")
        );
    }
}
