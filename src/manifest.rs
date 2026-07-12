// Concern: parses one package manifest per ecosystem (Python/npm/Cargo/Go) into a name + dependency list | Non-concern: cross-referencing or rendering | IO: (manifest path) -> Result<ParsedManifest>

use std::path::Path;

use anyhow::{Context, Result};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Ecosystem {
    Python,
    Npm,
    Cargo,
    Go,
}

/// A dependency edge as declared by a manifest, before internal/external is known.
#[derive(Debug, Clone)]
pub struct Dep {
    pub name: String,
    /// Declared as a local/path/workspace dependency — internal regardless of name match.
    pub local: bool,
}

#[derive(Debug, Clone)]
pub struct ParsedManifest {
    pub name: Option<String>,
    pub deps: Vec<Dep>,
}

impl ParsedManifest {
    /// A well-formed manifest that declares no package (e.g. a `pyproject.toml`
    /// with only tool config). Not an error — just nothing to graph.
    fn empty() -> Self {
        ParsedManifest {
            name: None,
            deps: Vec::new(),
        }
    }
}

fn read_file(path: &Path) -> Result<String> {
    std::fs::read_to_string(path).with_context(|| format!("reading {}", path.display()))
}

fn read_toml(path: &Path) -> Result<toml::Value> {
    toml::from_str(&read_file(path)?).with_context(|| format!("parsing {}", path.display()))
}

/// One implementation per ecosystem. Adding a language's dependency graph means
/// adding a parser here and registering it in [`parsers`] — open for extension.
///
/// `parse` returns `Err` only when the file is unreadable or syntactically corrupt
/// (worth warning about); a well-formed file that simply declares no package is
/// `Ok(ParsedManifest { name: None, .. })` and skipped quietly downstream.
pub trait ManifestParser {
    fn filename(&self) -> &'static str;
    fn ecosystem(&self) -> Ecosystem;
    fn parse(&self, path: &Path) -> Result<ParsedManifest>;
}

pub fn parsers() -> Vec<Box<dyn ManifestParser>> {
    vec![
        Box::new(Python),
        Box::new(Npm),
        Box::new(Cargo),
        Box::new(Go),
    ]
}

/// Normalize a Python distribution name to its PEP 503 canonical form so that
/// `acme_core`, `Acme-Core`, and `acme-core` compare equal.
pub fn canonicalize(eco: Ecosystem, name: &str) -> String {
    match eco {
        Ecosystem::Python => name.to_lowercase().replace('_', "-"),
        _ => name.to_string(),
    }
}

struct Python;
impl ManifestParser for Python {
    fn filename(&self) -> &'static str {
        "pyproject.toml"
    }
    fn ecosystem(&self) -> Ecosystem {
        Ecosystem::Python
    }
    fn parse(&self, path: &Path) -> Result<ParsedManifest> {
        let value: toml::Value = read_toml(path)?;
        let Some(project) = value.get("project") else {
            return Ok(ParsedManifest::empty());
        };
        let name = project
            .get("name")
            .and_then(|v| v.as_str())
            .map(str::to_string);
        let deps = project
            .get("dependencies")
            .and_then(|v| v.as_array())
            .into_iter()
            .flatten()
            .filter_map(|v| v.as_str())
            .filter_map(pep508_name)
            .map(|name| Dep { name, local: false })
            .collect();
        Ok(ParsedManifest { name, deps })
    }
}

/// Extract the distribution name from a PEP 508 requirement like
/// `acme-core[extra] >= 1.0`.
fn pep508_name(req: &str) -> Option<String> {
    let end = req
        .find(|c: char| "[<>=!~; (".contains(c))
        .unwrap_or(req.len());
    let name = req[..end].trim();
    (!name.is_empty()).then(|| name.to_string())
}

struct Npm;
impl ManifestParser for Npm {
    fn filename(&self) -> &'static str {
        "package.json"
    }
    fn ecosystem(&self) -> Ecosystem {
        Ecosystem::Npm
    }
    fn parse(&self, path: &Path) -> Result<ParsedManifest> {
        let text = read_file(path)?;
        let value: serde_json::Value =
            serde_json::from_str(&text).with_context(|| format!("parsing {}", path.display()))?;
        let name = value
            .get("name")
            .and_then(|v| v.as_str())
            .map(str::to_string);
        let mut deps = Vec::new();
        for table in ["dependencies", "devDependencies"] {
            if let Some(obj) = value.get(table).and_then(|v| v.as_object()) {
                for (dep_name, spec) in obj {
                    let local = spec.as_str().is_some_and(|s| {
                        s.starts_with("workspace:")
                            || s.starts_with("file:")
                            || s.starts_with("link:")
                    });
                    deps.push(Dep {
                        name: dep_name.clone(),
                        local,
                    });
                }
            }
        }
        Ok(ParsedManifest { name, deps })
    }
}

struct Cargo;
impl ManifestParser for Cargo {
    fn filename(&self) -> &'static str {
        "Cargo.toml"
    }
    fn ecosystem(&self) -> Ecosystem {
        Ecosystem::Cargo
    }
    fn parse(&self, path: &Path) -> Result<ParsedManifest> {
        let value: toml::Value = read_toml(path)?;
        let name = value
            .get("package")
            .and_then(|p| p.get("name"))
            .and_then(|v| v.as_str())
            .map(str::to_string);
        let mut deps = Vec::new();
        for table in ["dependencies", "dev-dependencies", "build-dependencies"] {
            if let Some(obj) = value.get(table).and_then(|v| v.as_table()) {
                for (dep_name, spec) in obj {
                    let local = spec.get("path").is_some();
                    deps.push(Dep {
                        name: dep_name.clone(),
                        local,
                    });
                }
            }
        }
        Ok(ParsedManifest { name, deps })
    }
}

struct Go;
impl ManifestParser for Go {
    fn filename(&self) -> &'static str {
        "go.mod"
    }
    fn ecosystem(&self) -> Ecosystem {
        Ecosystem::Go
    }
    fn parse(&self, path: &Path) -> Result<ParsedManifest> {
        Ok(parse_go_mod(&read_file(path)?))
    }
}

/// Parse the `module`, `require` (single + block), and `replace` directives of a
/// go.mod. A module with a `replace ... => ./local` is treated as a local dep.
fn parse_go_mod(text: &str) -> ParsedManifest {
    let mut name = None;
    let mut deps: Vec<Dep> = Vec::new();
    let mut replaced_local = std::collections::HashSet::new();
    let mut in_require = false;

    for raw in text.lines() {
        let line = strip_line_comment(raw).trim();
        if line.is_empty() {
            continue;
        }
        if let Some(rest) = line.strip_prefix("module ") {
            name = Some(rest.trim().to_string());
        } else if let Some(rest) = line.strip_prefix("replace ") {
            if let Some((from, to)) = rest.split_once("=>") {
                if is_local_path(to.trim()) {
                    replaced_local.insert(first_token(from.trim()).to_string());
                }
            }
        } else if line.starts_with("require (") {
            in_require = true;
        } else if in_require && line == ")" {
            in_require = false;
        } else if in_require {
            push_go_require(line, &mut deps);
        } else if let Some(rest) = line.strip_prefix("require ") {
            push_go_require(rest.trim(), &mut deps);
        }
    }

    for dep in &mut deps {
        if replaced_local.contains(&dep.name) {
            dep.local = true;
        }
    }
    ParsedManifest { name, deps }
}

fn push_go_require(line: &str, deps: &mut Vec<Dep>) {
    let module = first_token(line);
    if !module.is_empty() {
        deps.push(Dep {
            name: module.to_string(),
            local: false,
        });
    }
}

fn strip_line_comment(line: &str) -> &str {
    line.split_once("//").map_or(line, |(before, _)| before)
}

fn first_token(s: &str) -> &str {
    s.split_whitespace().next().unwrap_or("")
}

fn is_local_path(s: &str) -> bool {
    s.starts_with("./") || s.starts_with("../") || s.starts_with('/')
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn pep508_strips_specifiers() {
        assert_eq!(pep508_name("acme-core").unwrap(), "acme-core");
        assert_eq!(pep508_name("fastapi>=0.110").unwrap(), "fastapi");
        assert_eq!(pep508_name("pkg[extra] >= 1.0").unwrap(), "pkg");
    }

    #[test]
    fn canonicalize_python_names() {
        assert_eq!(canonicalize(Ecosystem::Python, "Acme_Core"), "acme-core");
        assert_eq!(canonicalize(Ecosystem::Cargo, "Serde_Json"), "Serde_Json");
    }

    #[test]
    fn go_mod_require_block_and_replace() {
        let text = "module example.com/app\n\ngo 1.22\n\nrequire (\n\tgithub.com/spf13/cobra v1.8.0\n\texample.com/shared v0.0.0\n)\n\nreplace example.com/shared => ../shared\n";
        let m = parse_go_mod(text);
        assert_eq!(m.name.unwrap(), "example.com/app");
        let cobra = m.deps.iter().find(|d| d.name.contains("cobra")).unwrap();
        assert!(!cobra.local);
        let shared = m.deps.iter().find(|d| d.name.contains("shared")).unwrap();
        assert!(shared.local);
    }

    #[test]
    fn go_mod_single_line_require() {
        let m = parse_go_mod("module m\nrequire github.com/x/y v1.0.0\n");
        assert_eq!(m.deps.len(), 1);
        assert_eq!(m.deps[0].name, "github.com/x/y");
    }
}
