// Graph: Cross-references every manifest into per-directory dependency edges (internal, external, reverse "used by"). NOT concerned with parsing manifest syntax or rendering. | I/O: (roots) -> map<dir, DirDeps>

use std::collections::{HashMap, HashSet};
use std::path::{Path, PathBuf};

use globset::GlobSet;
use serde::Serialize;

use crate::manifest::{canonicalize, Ecosystem, ManifestParser};

/// An internal (same-tree) dependency. `resolved` is false when the dep was
/// *declared* internal (npm `workspace:*`, Cargo `path=`) but no package with
/// that name exists in the scanned tree — a dangling workspace/path edge.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Serialize)]
pub struct InternalDep {
    pub name: String,
    pub resolved: bool,
}

/// The dependency facts shown next to a directory that holds a package manifest.
#[derive(Debug, Clone, Default, Serialize)]
pub struct DirDeps {
    pub used_by: Vec<String>,
    pub internal: Vec<InternalDep>,
    pub external: Vec<String>,
}

/// The package-level internal-edge list, keyed by canonical name + ecosystem. This
/// is the graph `build` already computes for directory keying; exposing it lets
/// policy evaluation (`rules`) reason over edges, and blast-radius (`--since`) map
/// files to their owning package, without re-walking or re-parsing. `dir` is the
/// package's canonical directory, matching the `dir_deps` keys.
#[derive(Debug, Clone)]
pub struct PackageEdges {
    pub name: String,
    pub ecosystem: Ecosystem,
    pub internal: Vec<InternalDep>,
    pub dir: PathBuf,
}

/// The resolved dependency graph: per-directory facts for rendering, plus the
/// package edge list for policy checks, plus parse warnings. One struct so callers
/// take exactly the view they need (Minimal API).
pub struct Graph {
    pub dir_deps: HashMap<PathBuf, DirDeps>,
    pub packages: Vec<PackageEdges>,
    pub warnings: Vec<String>,
}

struct Package {
    ecosystem: Ecosystem,
    name: String,
    dir: PathBuf,
    internal: Vec<InternalDep>,
    external: Vec<String>,
}

/// Scan `roots` for every known manifest, then resolve the graph. Directories are
/// keyed by canonicalized absolute path. The manifest walk applies the SAME filter as
/// the code-file walk (gitignore, hidden, `tests`, `-I` excludes) so that "what's
/// graphed" equals "what's shown"; a multi-root run drives that filter from the
/// PRIMARY (first) root's ignore settings, matching how the primary root's config
/// already governs the shared render/rules choices.
pub fn build(roots: &[PathBuf], gitignore: bool, include_tests: bool, excludes: &GlobSet) -> Graph {
    let parsers = crate::manifest::parsers();
    let mut raw: Vec<(Ecosystem, PathBuf, crate::manifest::ParsedManifest)> = Vec::new();
    let mut warnings: Vec<String> = Vec::new();

    for root in roots {
        collect_manifests(
            root,
            gitignore,
            include_tests,
            excludes,
            &parsers,
            &mut raw,
            &mut warnings,
        );
    }

    // Known package names per ecosystem, canonicalized, for internal detection.
    let mut names_by_eco: HashMap<Ecosystem, HashSet<String>> = HashMap::new();
    for (eco, _, m) in &raw {
        if let Some(name) = &m.name {
            names_by_eco
                .entry(*eco)
                .or_default()
                .insert(canonicalize(*eco, name));
        }
    }

    let mut packages = Vec::new();
    for (eco, dir, m) in raw {
        let Some(name) = m.name else { continue };
        let known = names_by_eco.get(&eco);
        let mut internal = Vec::new();
        let mut external = Vec::new();
        for dep in m.deps {
            let canon = canonicalize(eco, &dep.name);
            let resolved = known.is_some_and(|k| k.contains(&canon));
            let is_internal = dep.local || resolved;
            if is_internal {
                internal.push(InternalDep {
                    name: canon,
                    resolved,
                });
            } else {
                external.push(dep.name);
            }
        }
        internal.sort();
        internal.dedup();
        external.sort();
        external.dedup();
        packages.push(Package {
            ecosystem: eco,
            name: canonicalize(eco, &name),
            dir,
            internal,
            external,
        });
    }

    // Reverse edges: for each *resolved* internal dep D of P, D is "used by" P.
    // Unresolved deps have no target package in the tree, so no reverse edge.
    let mut used_by: HashMap<(Ecosystem, String), Vec<String>> = HashMap::new();
    for pkg in &packages {
        for dep in pkg.internal.iter().filter(|d| d.resolved) {
            used_by
                .entry((pkg.ecosystem, dep.name.clone()))
                .or_default()
                .push(pkg.name.clone());
        }
    }
    for names in used_by.values_mut() {
        names.sort();
        names.dedup();
    }

    // The package edge list is derived from the same resolved packages the
    // directory map is built from — computed once, no re-derivation in `rules`.
    let package_edges = packages
        .iter()
        .map(|pkg| PackageEdges {
            name: pkg.name.clone(),
            ecosystem: pkg.ecosystem,
            internal: pkg.internal.clone(),
            dir: canon_dir(&pkg.dir),
        })
        .collect();

    let mut out = HashMap::new();
    for pkg in packages {
        let used = used_by
            .get(&(pkg.ecosystem, pkg.name.clone()))
            .cloned()
            .unwrap_or_default();
        out.insert(
            canon_dir(&pkg.dir),
            DirDeps {
                used_by: used,
                internal: pkg.internal,
                external: pkg.external,
            },
        );
    }
    Graph {
        dir_deps: out,
        packages: package_edges,
        warnings,
    }
}

impl Graph {
    /// The transitive reverse-dependency closure of `pkg` within ecosystem `eco`:
    /// every package that (directly or indirectly) depends on `pkg`. This is the
    /// "blast radius" — who could break if `pkg` changes — walked over the resolved
    /// internal edges (the inverse of the `used_by` relation). The seed `pkg` itself
    /// is NOT included; only its dependents. Cycle-safe via a visited set, so a
    /// dependency cycle terminates instead of looping.
    pub fn reverse_closure(&self, pkg: &str, eco: Ecosystem) -> HashSet<String> {
        let mut result = HashSet::new();
        let mut seen: HashSet<String> = HashSet::from([pkg.to_string()]);
        let mut stack = vec![pkg.to_string()];
        while let Some(cur) = stack.pop() {
            for p in &self.packages {
                if p.ecosystem != eco {
                    continue;
                }
                let depends_on_cur = p.internal.iter().any(|d| d.resolved && d.name == cur);
                if depends_on_cur && seen.insert(p.name.clone()) {
                    result.insert(p.name.clone());
                    stack.push(p.name.clone());
                }
            }
        }
        result
    }

    /// The set of package directories in the blast radius of a change set: for each
    /// changed file, resolve its owning package (nearest ancestor package dir), take
    /// that package's reverse-dependency closure, and map those dependents back to
    /// their directories. The returned dirs are canonical, so a walked file is "in
    /// the blast radius" iff it `starts_with` one of them.
    pub fn blast_radius_dirs(&self, changed: &HashSet<PathBuf>) -> HashSet<PathBuf> {
        let mut affected: HashSet<(Ecosystem, String)> = HashSet::new();
        for file in changed {
            if let Some(owner) = self.owning_package(file) {
                for name in self.reverse_closure(&owner.name, owner.ecosystem) {
                    affected.insert((owner.ecosystem, name));
                }
            }
        }
        self.packages
            .iter()
            .filter(|p| affected.contains(&(p.ecosystem, p.name.clone())))
            .map(|p| p.dir.clone())
            .collect()
    }

    /// The package that owns `file`: the package whose directory is the *deepest*
    /// ancestor of the file (so a file in a nested package is attributed to the
    /// inner one). `None` if the file lives under no package.
    fn owning_package(&self, file: &Path) -> Option<&PackageEdges> {
        self.packages
            .iter()
            .filter(|p| file.starts_with(&p.dir))
            .max_by_key(|p| p.dir.components().count())
    }
}

fn collect_manifests(
    root: &Path,
    gitignore: bool,
    include_tests: bool,
    excludes: &GlobSet,
    parsers: &[Box<dyn ManifestParser>],
    out: &mut Vec<(Ecosystem, PathBuf, crate::manifest::ParsedManifest)>,
    warnings: &mut Vec<String>,
) {
    // One traversal for every manifest kind: dispatch each entry to the parser
    // whose filename it matches. (Previously one full walk per parser.) Shares the
    // code-file walk's exact directory filter (`configured_walk`), so gitignored/
    // hidden/`tests`/`-I`-excluded manifests are skipped just like their files —
    // no spurious "could not parse manifest" warnings for invisible files, and no
    // package leaking into the name set from a dir the tree never shows.
    let walker = crate::walk::configured_walk(root, gitignore, include_tests, excludes).build();
    for entry in walker.flatten() {
        let fname = entry.file_name();
        let Some(parser) = parsers.iter().find(|p| fname == p.filename()) else {
            continue;
        };
        let path = entry.path();
        let Some(dir) = path.parent() else { continue };
        match parser.parse(path) {
            Ok(parsed) => out.push((parser.ecosystem(), dir.to_path_buf(), parsed)),
            Err(err) => warnings.push(format!("could not parse manifest: {err:#}")),
        }
    }
}

fn canon_dir(dir: &Path) -> PathBuf {
    dir.canonicalize().unwrap_or_else(|_| dir.to_path_buf())
}

impl DirDeps {
    /// Render the trailing `# ...` annotation for a package directory, or `None`
    /// when there is nothing worth showing.
    pub fn annotation(&self) -> Option<String> {
        let mut parts = Vec::new();
        if !self.used_by.is_empty() {
            parts.push(format!("used by: [{}]", self.used_by.join(", ")));
        }
        if !self.internal.is_empty() {
            let deps = self
                .internal
                .iter()
                .map(|d| {
                    if d.resolved {
                        d.name.clone()
                    } else {
                        format!("{} (unresolved)", d.name)
                    }
                })
                .collect::<Vec<_>>()
                .join(", ");
            parts.push(format!("<- depends on [{deps}]"));
        }
        if !parts.is_empty() {
            return Some(parts.join("; "));
        }
        if !self.external.is_empty() {
            const MAX: usize = 3;
            let shown = self.external.len().min(MAX);
            let mut preview = self.external[..shown].join(", ");
            if self.external.len() > MAX {
                preview.push_str(&format!(", +{} more", self.external.len() - MAX));
            }
            return Some(format!("ext: {preview}"));
        }
        None
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Build a `Graph` from a hand-written package edge list (name -> resolved
    /// internal deps). Directories/`dir_deps` are irrelevant to `reverse_closure`,
    /// so they stay empty — this isolates the pure closure algorithm.
    fn graph(edges: &[(&str, &[&str])]) -> Graph {
        let packages = edges
            .iter()
            .map(|(name, deps)| PackageEdges {
                name: name.to_string(),
                ecosystem: Ecosystem::Cargo,
                internal: deps
                    .iter()
                    .map(|d| InternalDep {
                        name: d.to_string(),
                        resolved: true,
                    })
                    .collect(),
                dir: PathBuf::new(),
            })
            .collect();
        Graph {
            dir_deps: HashMap::new(),
            packages,
            warnings: Vec::new(),
        }
    }

    fn closure(g: &Graph, pkg: &str) -> Vec<String> {
        let mut v: Vec<String> = g
            .reverse_closure(pkg, Ecosystem::Cargo)
            .into_iter()
            .collect();
        v.sort();
        v
    }

    #[test]
    fn transitive_closure_excludes_seed() {
        // core <- api <- gateway ; core <- worker
        let g = graph(&[
            ("core", &[]),
            ("api", &["core"]),
            ("worker", &["core"]),
            ("gateway", &["api"]),
        ]);
        // Editing core blasts everything that (transitively) depends on it.
        assert_eq!(closure(&g, "core"), vec!["api", "gateway", "worker"]);
        // A leaf that nothing depends on has an empty radius.
        assert!(closure(&g, "gateway").is_empty());
    }

    #[test]
    fn diamond_dedupes_shared_dependents() {
        // core <- {left, right} <- top (a diamond): top reached via two paths, once.
        let g = graph(&[
            ("core", &[]),
            ("left", &["core"]),
            ("right", &["core"]),
            ("top", &["left", "right"]),
        ]);
        assert_eq!(closure(&g, "core"), vec!["left", "right", "top"]);
    }

    #[test]
    fn cycle_terminates() {
        // a <-> b cycle, plus c depending on a. Must not loop forever.
        let g = graph(&[("a", &["b"]), ("b", &["a"]), ("c", &["a"])]);
        // Everything else in the cycle/dependents, minus the seed itself.
        assert_eq!(closure(&g, "a"), vec!["b", "c"]);
        assert_eq!(closure(&g, "b"), vec!["a", "c"]);
    }
}
