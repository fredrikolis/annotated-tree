// Concern: declares the `[rules]` table and evaluates its dependency half — deny, cycles, orphans | Non-concern: computing edges, or enforcing the bound | IO: (packages, Rules) -> Vec<Violation>

use std::collections::{HashMap, HashSet};
use std::path::PathBuf;

use serde::Serialize;

use crate::graph::PackageEdges;
use crate::manifest::Ecosystem;

/// Empty/false means "enforce nothing".
#[derive(Debug, Default, Clone)]
pub struct Rules {
    pub deny: Vec<(String, String)>,
    pub forbid_cycles: bool,
    pub forbid_orphans: bool,
    pub require_package_charter: bool,
    /// `None` — what a configured `0` normalizes to — raises no length issue at any length.
    pub max_annotation_length: Option<usize>,
}

impl Rules {
    /// When false the strict path skips the whole graph build. `max_annotation_length` is
    /// excluded: it's a per-file property that must not pull in a graph build.
    pub fn is_active(&self) -> bool {
        !self.deny.is_empty()
            || self.forbid_cycles
            || self.forbid_orphans
            || self.require_package_charter
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum RuleCode {
    DeniedDependency,
    DependencyCycle,
    OrphanPackage,
    UnknownDenyPackage,
    /// Evaluated in `strict` (needs a filesystem charter resolve), not produced by [`evaluate`].
    MissingPackageCharter,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Violation {
    pub code: RuleCode,
    pub message: String,
    /// `[from, to]` for a denied dependency, the ordered node path for a cycle, the single
    /// package for an orphan / unknown deny.
    pub packages: Vec<String>,
    /// `None` for a cycle spanning many packages, or an absent deny package.
    pub dir: Option<PathBuf>,
}

/// Sorted and de-duplicated so the report is deterministic regardless of walk order.
pub fn evaluate(packages: &[PackageEdges], rules: &Rules) -> Vec<Violation> {
    let mut out = Vec::new();

    unknown_deny_packages(packages, &rules.deny, &mut out);
    deny_violations(packages, &rules.deny, &mut out);
    if rules.forbid_cycles {
        cycle_violations(packages, &mut out);
    }
    if rules.forbid_orphans {
        orphan_violations(packages, &mut out);
    }

    out.sort_by(|a, b| a.message.cmp(&b.message));
    out.dedup_by(|a, b| a.message == b.message);
    out
}

fn unknown_deny_packages(
    packages: &[PackageEdges],
    deny: &[(String, String)],
    out: &mut Vec<Violation>,
) {
    let known: HashSet<&str> = packages.iter().map(|p| p.name.as_str()).collect();
    for (from, to) in deny {
        for name in [from, to] {
            if !known.contains(name.as_str()) {
                out.push(Violation {
                    code: RuleCode::UnknownDenyPackage,
                    message: format!(
                        "deny rule names unknown package '{name}': matches no package in the scanned tree"
                    ),
                    packages: vec![name.clone()],
                    dir: None,
                });
            }
        }
    }
}

fn deny_violations(packages: &[PackageEdges], deny: &[(String, String)], out: &mut Vec<Violation>) {
    for pkg in packages {
        for dep in pkg.internal.iter().filter(|d| d.resolved) {
            for (from, to) in deny {
                if &pkg.name == from && &dep.name == to {
                    out.push(Violation {
                        code: RuleCode::DeniedDependency,
                        message: format!("denied dependency: {from} must not depend on {to}"),
                        packages: vec![from.clone(), to.clone()],
                        dir: Some(pkg.dir.clone()),
                    });
                }
            }
        }
    }
}

/// Keyed by `(ecosystem, name)`: a name shared across ecosystems is not the same node.
pub fn orphan_packages(packages: &[PackageEdges]) -> Vec<&PackageEdges> {
    let mut depended_on: HashSet<(Ecosystem, &str)> = HashSet::new();
    for pkg in packages {
        for dep in pkg.internal.iter().filter(|d| d.resolved) {
            depended_on.insert((pkg.ecosystem, dep.name.as_str()));
        }
    }
    packages
        .iter()
        .filter(|pkg| {
            let has_out = pkg.internal.iter().any(|d| d.resolved);
            let has_in = depended_on.contains(&(pkg.ecosystem, pkg.name.as_str()));
            !has_out && !has_in
        })
        .collect()
}

fn orphan_violations(packages: &[PackageEdges], out: &mut Vec<Violation>) {
    for pkg in orphan_packages(packages) {
        out.push(Violation {
            code: RuleCode::OrphanPackage,
            message: format!(
                "orphan package: {} has no internal dependencies in or out",
                pkg.name
            ),
            packages: vec![pkg.name.clone()],
            dir: Some(pkg.dir.clone()),
        });
    }
}

fn cycle_violations(packages: &[PackageEdges], out: &mut Vec<Violation>) {
    let mut index: HashMap<(Ecosystem, &str), usize> = HashMap::new();
    for (i, pkg) in packages.iter().enumerate() {
        index.insert((pkg.ecosystem, pkg.name.as_str()), i);
    }
    let adjacency: Vec<Vec<usize>> = packages
        .iter()
        .map(|pkg| {
            pkg.internal
                .iter()
                .filter(|d| d.resolved)
                .filter_map(|d| index.get(&(pkg.ecosystem, d.name.as_str())).copied())
                .collect()
        })
        .collect();

    for cycle in find_cycles(&adjacency) {
        let mut names: Vec<&str> = cycle.iter().map(|&i| packages[i].name.as_str()).collect();
        let path: Vec<String> = names.iter().map(|s| s.to_string()).collect();
        names.push(names[0]); // close the loop for a readable A -> B -> C -> A message
        out.push(Violation {
            code: RuleCode::DependencyCycle,
            message: format!("dependency cycle: {}", names.join(" -> ")),
            packages: path,
            dir: None,
        });
    }
}

/// Find cycles in a directed graph via DFS with white/gray/black colouring: a back
/// edge to a gray (on-stack) node closes a cycle, reconstructed from the DFS stack.
/// Each distinct node set is reported once.
fn find_cycles(adjacency: &[Vec<usize>]) -> Vec<Vec<usize>> {
    let mut color = vec![0u8; adjacency.len()]; // 0 white, 1 gray, 2 black
    let mut stack: Vec<usize> = Vec::new();
    let mut cycles: Vec<Vec<usize>> = Vec::new();
    let mut seen: HashSet<Vec<usize>> = HashSet::new();
    for start in 0..adjacency.len() {
        if color[start] == 0 {
            dfs(
                start,
                adjacency,
                &mut color,
                &mut stack,
                &mut cycles,
                &mut seen,
            );
        }
    }
    cycles
}

fn dfs(
    u: usize,
    adjacency: &[Vec<usize>],
    color: &mut [u8],
    stack: &mut Vec<usize>,
    cycles: &mut Vec<Vec<usize>>,
    seen: &mut HashSet<Vec<usize>>,
) {
    color[u] = 1;
    stack.push(u);
    for &v in &adjacency[u] {
        if color[v] == 1 {
            let pos = stack.iter().position(|&x| x == v).unwrap();
            let cycle = stack[pos..].to_vec();
            let mut key = cycle.clone();
            key.sort_unstable();
            if seen.insert(key) {
                cycles.push(cycle);
            }
        } else if color[v] == 0 {
            dfs(v, adjacency, color, stack, cycles, seen);
        }
    }
    stack.pop();
    color[u] = 2;
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::graph::InternalDep;

    fn pkg(name: &str, deps: &[&str]) -> PackageEdges {
        PackageEdges {
            name: name.to_string(),
            ecosystem: Ecosystem::Cargo,
            internal: deps
                .iter()
                .map(|d| InternalDep {
                    name: d.to_string(),
                    resolved: true,
                })
                .collect(),
            dir: std::path::PathBuf::new(),
        }
    }

    fn messages(packages: &[PackageEdges], rules: &Rules) -> Vec<String> {
        evaluate(packages, rules)
            .into_iter()
            .map(|v| v.message)
            .collect()
    }

    #[test]
    fn reports_a_three_node_cycle() {
        let packages = [pkg("a", &["b"]), pkg("b", &["c"]), pkg("c", &["a"])];
        let rules = Rules {
            forbid_cycles: true,
            ..Default::default()
        };
        // The ` -> `-joined render is a presentation detail, deliberately not frozen here.
        let msgs = messages(&packages, &rules);
        assert_eq!(msgs.len(), 1, "one cycle reported once: {msgs:?}");
        assert!(
            ["a", "b", "c"].iter().all(|n| msgs[0].contains(n)),
            "the finding names every node in the cycle: {}",
            msgs[0]
        );
    }

    #[test]
    fn reports_a_two_node_cycle_once() {
        let packages = [pkg("a", &["b"]), pkg("b", &["a"])];
        let rules = Rules {
            forbid_cycles: true,
            ..Default::default()
        };
        // A 2-cycle is reachable from both nodes and must still be reported once.
        let msgs = messages(&packages, &rules);
        assert_eq!(
            msgs.len(),
            1,
            "the 2-cycle is reported exactly once: {msgs:?}"
        );
        assert!(
            msgs[0].contains("a") && msgs[0].contains("b"),
            "names both nodes: {}",
            msgs[0]
        );
    }

    #[test]
    fn reports_a_self_loop() {
        let packages = [pkg("a", &["a"])];
        let rules = Rules {
            forbid_cycles: true,
            ..Default::default()
        };
        // Self-loop edge case: a single node depending on itself is one cycle.
        let msgs = messages(&packages, &rules);
        assert_eq!(msgs.len(), 1, "a self-loop is one cycle: {msgs:?}");
        assert!(
            msgs[0].contains("a"),
            "names the self-looping node: {}",
            msgs[0]
        );
    }

    #[test]
    fn acyclic_graph_has_no_cycle_violation() {
        let packages = [pkg("a", &["b", "c"]), pkg("b", &["c"]), pkg("c", &[])];
        let rules = Rules {
            forbid_cycles: true,
            ..Default::default()
        };
        assert!(messages(&packages, &rules).is_empty());
    }

    #[test]
    fn deny_matches_a_forbidden_edge() {
        let packages = [pkg("web", &["core"]), pkg("core", &[])];
        let rules = Rules {
            deny: vec![("web".to_string(), "core".to_string())],
            ..Default::default()
        };
        let msgs = messages(&packages, &rules);
        assert_eq!(msgs.len(), 1, "a matched deny rule fires once: {msgs:?}");
        assert!(
            msgs[0].contains("web") && msgs[0].contains("core"),
            "the finding names the participating packages: {}",
            msgs[0]
        );
    }

    #[test]
    fn deny_ignores_a_non_matching_edge() {
        // web depends on util, not core — the web->core rule must stay silent.
        let packages = [pkg("web", &["util"]), pkg("core", &[]), pkg("util", &[])];
        let rules = Rules {
            deny: vec![("web".to_string(), "core".to_string())],
            ..Default::default()
        };
        assert!(messages(&packages, &rules).is_empty());
    }

    #[test]
    fn unknown_deny_package_is_flagged() {
        let packages = [pkg("web", &["core"]), pkg("core", &[])];
        let rules = Rules {
            deny: vec![("web".to_string(), "ghost".to_string())],
            ..Default::default()
        };
        // Behaviour only, as in the sibling tests: the prose is frozen once at the e2e level.
        let msgs = messages(&packages, &rules);
        assert_eq!(msgs.len(), 1, "unknown-package deny fires once: {msgs:?}");
        assert!(
            msgs[0].contains("ghost"),
            "the finding names the unknown package: {}",
            msgs[0]
        );
    }

    #[test]
    fn orphan_packages_is_the_shared_definition() {
        // The shared definition `forbid_orphans` builds on, asserted directly rather than through the rule.
        let packages = [pkg("web", &["core"]), pkg("core", &[]), pkg("lonely", &[])];
        let orphans: Vec<&str> = orphan_packages(&packages)
            .iter()
            .map(|p| p.name.as_str())
            .collect();
        assert_eq!(
            orphans,
            vec!["lonely"],
            "only the disconnected package is an orphan"
        );
    }

    #[test]
    fn orphan_package_is_flagged() {
        let packages = [pkg("web", &["core"]), pkg("core", &[]), pkg("lonely", &[])];
        let rules = Rules {
            forbid_orphans: true,
            ..Default::default()
        };
        // Behaviour only, as above.
        let msgs = messages(&packages, &rules);
        assert_eq!(msgs.len(), 1, "only the orphan is flagged: {msgs:?}");
        assert!(
            msgs[0].contains("lonely"),
            "the finding names the orphan package: {}",
            msgs[0]
        );
    }
}
