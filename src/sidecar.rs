// Concern: resolves `<name>.annotation` sidecars — which paths are sidecars, and what each annotates | Non-concern: the grammar or a directory's charter | IO: (path, Config) -> Option<path|body>

use std::path::{Path, PathBuf};

use crate::config::Config;
use crate::walk::CHARTER_FILE;

pub fn path_for(file: &Path) -> PathBuf {
    let mut name = file.file_name().unwrap_or_default().to_os_string();
    name.push(CHARTER_FILE);
    file.with_file_name(name)
}

/// `None` for the bare `.annotation` itself — that's a DIRECTORY charter, not a sidecar for a
/// file called "".
pub fn named_target(path: &Path) -> Option<PathBuf> {
    let name = path.file_name()?.to_str()?;
    let stem = name.strip_suffix(CHARTER_FILE)?;
    if stem.is_empty() {
        return None;
    }
    Some(path.with_file_name(stem))
}

pub fn target_of(path: &Path, config: &Config) -> Option<PathBuf> {
    let target = named_target(path)?;
    let annotatable = target.is_file() && config.language_for_path(&target).is_none();
    annotatable.then_some(target)
}

pub fn annotates(file: &Path, config: &Config) -> bool {
    config.language_for_path(file).is_none() && path_for(file).is_file()
}

/// Untrimmed, so a caller can tell an EMPTY sidecar (a defect) from an absent one.
pub fn body(file: &Path) -> Option<String> {
    std::fs::read_to_string(path_for(file)).ok()
}

pub fn candidates_in(dir: &Path) -> Vec<PathBuf> {
    let Ok(entries) = std::fs::read_dir(dir) else {
        return Vec::new();
    };
    let mut out: Vec<PathBuf> = entries
        .flatten()
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
        assert_eq!(named_target(Path::new("a/.annotation")), None);
        assert_eq!(
            named_target(Path::new("a/trials.csv.annotation")),
            Some(PathBuf::from("a/trials.csv"))
        );
        assert_eq!(named_target(Path::new("a/trials.csv")), None);
    }

    #[test]
    fn path_for_appends_the_suffix_to_the_whole_name() {
        assert_eq!(
            path_for(Path::new("a/trials.csv")),
            PathBuf::from("a/trials.csv.annotation")
        );
    }
}
