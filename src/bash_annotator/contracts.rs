// Concern: the per-process cache from a path to its annotation, or a directory to its charter | Non-concern: the annotation grammar, or reading a path out of a line | IO: (path) -> annotation text

use std::collections::HashMap;
use std::path::{Path, PathBuf};

use crate::annotation;
use crate::config::{CliOverrides, Config};

/// Delegates to `annotated-tree` rather than re-deriving the contract grammar.
pub struct Contracts {
    config: Option<Config>,
    files: HashMap<PathBuf, Option<String>>,
    dirs: HashMap<PathBuf, Option<String>>,
}

impl Contracts {
    pub fn new() -> Self {
        Contracts {
            config: Config::load(Path::new("."), &CliOverrides::default()).ok(),
            files: HashMap::new(),
            dirs: HashMap::new(),
        }
    }

    /// `None` rather than a placeholder, so a "(none)" doesn't bury the entries that do speak.
    pub fn describe(&mut self, path: &Path) -> Option<String> {
        if path.is_dir() {
            self.charter(path)
        } else {
            self.file(path)
        }
    }

    fn file(&mut self, path: &Path) -> Option<String> {
        if let Some(hit) = self.files.get(path) {
            return hit.clone();
        }
        let found = self
            .config
            .as_ref()
            .and_then(|c| c.language_for_path(path))
            .and_then(|lang| annotation::extract(path, lang))
            .or_else(|| annotation::extract_any(path));
        self.files.insert(path.to_path_buf(), found.clone());
        found
    }

    fn charter(&mut self, dir: &Path) -> Option<String> {
        if let Some(hit) = self.dirs.get(dir) {
            return hit.clone();
        }
        let found = self
            .config
            .as_ref()
            .and_then(|c| crate::charter::resolve_from_fs(dir, c))
            .map(|c| c.line());
        self.dirs.insert(dir.to_path_buf(), found.clone());
        found
    }
}
