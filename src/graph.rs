// Concern: cross-references every manifest into per-directory dependency edges — internal, external and reverse | Non-concern: parsing manifest syntax or rendering | IO: (roots) -> map<dir, DirDeps>

use std::collections::{HashMap, HashSet};
use std::path::{Path, PathBuf};

use serde::Serialize;

use crate::manifest::{canonicalize, Ecosystem, ManifestParser};
use crate::walk::WalkFilter;

/// `resolved` is false when the dep was *declared* internal (npm `workspace:*`, Cargo `path=`)
/// but no package with that name exists in the scanned tree.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Serialize)]
pub struct InternalDep {
    pub name: String,
    pub resolved: bool,
}

#[derive(Debug, Clone, Default, Serialize)]
pub struct DirDeps {
    pub used_by: Vec<String>,
    pub internal: Vec<InternalDep>,
    pub external: Vec<String>,
}

/// Exposed so `rules` and `--since` can reason over edges without re-walking or re-parsing.
#[derive(Debug, Clone)]
pub struct PackageEdges {
    pub name: String,
    pub ecosystem: Ecosystem,
    pub internal: Vec<InternalDep>,
    pub dir: PathBuf,
}

/// A manifest a parser could not read or parse; the map is still produced, just missing that
/// package's edges.
#[derive(Debug, Clone, Serialize)]
pub struct Warning {
    pub code: &'static str,
    pub path: String,
    pub message: String,
}

pub struct Graph {
    pub dir_deps: HashMap<PathBuf, DirDeps>,
    pub packages: Vec<PackageEdges>,
    pub warnings: Vec<Warning>,
}

struct Package {
    ecosystem: Ecosystem,
    name: String,
    dir: PathBuf,
    internal: Vec<InternalDep>,
    external: Vec<String>,
}

/// `max_depth` bounds the manifest walk one level below the deepest displayed row, so "what's
/// graphed" is exactly "what's shown". `None` scans every depth.
pub fn build(roots: &[PathBuf], filter: WalkFilter<'_>, max_depth: Option<usize>) -> Graph {
    let parsers = crate::manifest::parsers();
    let mut raw: Vec<(Ecosystem, PathBuf, crate::manifest::ParsedManifest)> = Vec::new();
    let mut warnings: Vec<Warning> = Vec::new();

    let scope = WalkScope { filter, max_depth };
    for root in roots {
        collect_manifests(root, &scope, &parsers, &mut raw, &mut warnings);
    }

    let mut names_by_eco: HashMap<Ecosystem, HashSet<String>> = HashMap::new();
    for (eco, _, m) in &raw {
        if let Some(name) = &m.name {
            names_by_eco.entry(*eco).or_default().insert(canonicalize(*eco, name));
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
                internal.push(InternalDep { name: canon, resolved });
            } else {
                external.push(dep.name);
            }
        }
        internal.sort();
        internal.dedup();
        external.sort();
        external.dedup();
        packages.push(Package { ecosystem: eco, name: canonicalize(eco, &name), dir, internal, external });
    }

    // Unresolved deps have no target package in the tree, so no reverse edge.
    let mut used_by: HashMap<(Ecosystem, String), Vec<String>> = HashMap::new();
    for pkg in &packages {
        for dep in pkg.internal.iter().filter(|d| d.resolved) {
            used_by.entry((pkg.ecosystem, dep.name.clone())).or_default().push(pkg.name.clone());
        }
    }
    for names in used_by.values_mut() {
        names.sort();
        names.dedup();
    }

    let package_edges = packages.iter().map(|pkg| PackageEdges { name: pkg.name.clone(), ecosystem: pkg.ecosystem, internal: pkg.internal.clone(), dir: canon_dir(&pkg.dir) }).collect();

    let mut out = HashMap::new();
    for pkg in packages {
        let used = used_by.get(&(pkg.ecosystem, pkg.name.clone())).cloned().unwrap_or_default();
        out.insert(canon_dir(&pkg.dir), DirDeps { used_by: used, internal: pkg.internal, external: pkg.external });
    }
    Graph { dir_deps: out, packages: package_edges, warnings }
}

impl Graph {
    /// The seed itself is NOT included. Cycle-safe via a visited set.
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

    /// The returned dirs are canonical, so a walked file is in the blast radius iff it
    /// `starts_with` one of them.
    pub fn blast_radius_dirs(&self, changed: &HashSet<PathBuf>) -> HashSet<PathBuf> {
        let mut affected: HashSet<(Ecosystem, String)> = HashSet::new();
        for file in changed {
            if let Some(owner) = self.owning_package(file) {
                for name in self.reverse_closure(&owner.name, owner.ecosystem) {
                    affected.insert((owner.ecosystem, name));
                }
            }
        }
        self.packages.iter().filter(|p| affected.contains(&(p.ecosystem, p.name.clone()))).map(|p| p.dir.clone()).collect()
    }

    /// The *deepest* ancestor package wins, so a nested package is attributed to itself.
    fn owning_package(&self, file: &Path) -> Option<&PackageEdges> {
        self.packages.iter().filter(|p| file.starts_with(&p.dir)).max_by_key(|p| p.dir.components().count())
    }
}

/// Bounds one level DEEPER than the rows — see [`crate::walk::cap_manifest_depth`].
struct WalkScope<'a> {
    filter: WalkFilter<'a>,
    max_depth: Option<usize>,
}

fn collect_manifests(root: &Path, scope: &WalkScope<'_>, parsers: &[Box<dyn ManifestParser>], out: &mut Vec<(Ecosystem, PathBuf, crate::manifest::ParsedManifest)>, warnings: &mut Vec<Warning>) {
    let mut builder = crate::walk::configured_walk(root, scope.filter);
    crate::walk::cap_manifest_depth(&mut builder, scope.max_depth);
    let walker = builder.build();
    for entry in walker.flatten() {
        let fname = entry.file_name();
        let Some(parser) = parsers.iter().find(|p| fname == p.filename()) else {
            continue;
        };
        let path = entry.path();
        let Some(dir) = path.parent() else { continue };
        match parser.parse(path) {
            Ok(parsed) => out.push((parser.ecosystem(), dir.to_path_buf(), parsed)),
            // `path` is also a structured field, for the JSON `warnings` array to dispatch on.
            Err(err) => warnings.push(Warning { code: crate::exit::code::MANIFEST_PARSE_ERROR, path: path.display().to_string(), message: format!("could not parse manifest: {err:#}") }),
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
            let deps = self.internal.iter().map(|d| if d.resolved { d.name.clone() } else { format!("{} (unresolved)", d.name) }).collect::<Vec<_>>().join(", ");
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

    /// Directories/`dir_deps` stay empty — irrelevant to `reverse_closure`.
    fn graph(edges: &[(&str, &[&str])]) -> Graph {
        let packages = edges.iter().map(|(name, deps)| PackageEdges { name: name.to_string(), ecosystem: Ecosystem::Cargo, internal: deps.iter().map(|d| InternalDep { name: d.to_string(), resolved: true }).collect(), dir: PathBuf::new() }).collect();
        Graph { dir_deps: HashMap::new(), packages, warnings: Vec::new() }
    }

    fn closure(g: &Graph, pkg: &str) -> Vec<String> {
        let mut v: Vec<String> = g.reverse_closure(pkg, Ecosystem::Cargo).into_iter().collect();
        v.sort();
        v
    }

    #[test]
    fn transitive_closure_excludes_seed() {
        // core <- api <- gateway ; core <- worker
        let g = graph(&[("core", &[]), ("api", &["core"]), ("worker", &["core"]), ("gateway", &["api"])]);
        assert_eq!(closure(&g, "core"), vec!["api", "gateway", "worker"]);
        assert!(closure(&g, "gateway").is_empty(), "a leaf nothing depends on has an empty radius");
    }

    #[test]
    fn diamond_dedupes_shared_dependents() {
        // core <- {left, right} <- top (a diamond): top reached via two paths, once.
        let g = graph(&[("core", &[]), ("left", &["core"]), ("right", &["core"]), ("top", &["left", "right"])]);
        assert_eq!(closure(&g, "core"), vec!["left", "right", "top"]);
    }

    #[test]
    fn cycle_terminates() {
        let g = graph(&[("a", &["b"]), ("b", &["a"]), ("c", &["a"])]);
        assert_eq!(closure(&g, "a"), vec!["b", "c"]);
        assert_eq!(closure(&g, "b"), vec!["a", "c"]);
    }
}
