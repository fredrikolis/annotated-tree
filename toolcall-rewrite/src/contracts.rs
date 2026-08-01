// Concern: the per-process cache from a path to its annotation, or a directory to its charter | Non-concern: the annotation grammar, or reading a path out of a line | IO: (path) -> annotation text

use std::collections::HashMap;
use std::path::{Path, PathBuf};

use annotated_tree::annotation;
use annotated_tree::config::{CliOverrides, Config};

/// Path-keyed contracts and charters, memoised for one process.
///
/// Both lookups delegate to the `annotated-tree` library rather than re-deriving what a contract
/// looks like: the grammar and the language table are the product's to define, and a wrapper that
/// disagreed with the tool about a file's contract would be worse than one that showed none.
pub struct Contracts {
    config: Option<Config>,
    files: HashMap<PathBuf, Option<String>>,
    dirs: HashMap<PathBuf, Option<String>>,
}

impl Contracts {
    pub fn new() -> Self {
        Contracts {
            // A config failure degrades to "no contracts" rather than aborting: the wrapped tool's
            // own output is still worth delivering, and swallowing it would lose the user's answer.
            config: Config::load(Path::new("."), &CliOverrides::default()).ok(),
            files: HashMap::new(),
            dirs: HashMap::new(),
        }
    }

    /// What a path declares about itself: a directory's charter, a file's first line, or `None`
    /// when it declares nothing.
    ///
    /// `None` rather than a placeholder is deliberate. Output that enumerates a directory carries
    /// every lockfile and build artifact, and a "(none)" beside each would bury the entries that
    /// do speak.
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
        // Marker-based first, then marker-agnostic, so an extensionless or unrecognised file
        // still shows the line it declared.
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
            .and_then(|c| annotated_tree::resolve_charter(dir, c))
            .map(|c| c.line());
        self.dirs.insert(dir.to_path_buf(), found.clone());
        found
    }
}
