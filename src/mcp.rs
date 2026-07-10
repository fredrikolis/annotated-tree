// MCP server: The one async surface — an rmcp stdio server exposing the sync map/graph/strict builders as MCP tools for agents and editors. NOT concerned with reimplementing any builder logic (thin adapters only). | I/O: (Cli) -> stdio JSON-RPC server -> exit code
//
// This is the ONLY async module, gated behind the `mcp` cargo feature so the default
// build links no tokio/rmcp and stays sync (see the `lib.rs` header).
//
// `--mcp` is genuine IPC (JSON-RPC over stdio), so the edge is async — but the builders
// it drives are BLOCKING (filesystem reads + a thread-pool directory walk) and must
// never run on the async executor. Every tool handler wraps its blocking work in
// `tokio::task::spawn_blocking`, so the sync core stays untouched.

#[cfg(not(feature = "mcp"))]
pub fn serve(_cli: &crate::cli::Cli) -> anyhow::Result<i32> {
    anyhow::bail!(
        "--mcp requires a build with the `mcp` feature (rebuild with `cargo build --features mcp`)"
    )
}

#[cfg(feature = "mcp")]
pub use imp::serve;

#[cfg(feature = "mcp")]
mod imp {
    use std::path::PathBuf;
    use std::sync::Arc;

    use anyhow::{Context, Result};
    use globset::GlobSet;
    use rmcp::handler::server::wrapper::Parameters;
    use rmcp::model::{CallToolResult, ContentBlock, ServerCapabilities, ServerInfo};
    use rmcp::transport::stdio;
    use rmcp::{schemars, tool, tool_handler, tool_router, ErrorData, ServerHandler, ServiceExt};
    use serde::Deserialize;
    use serde_json::json;

    use crate::cli::Cli;
    use crate::config::{CliOverrides, Config};
    use crate::render::{JsonRenderer, Renderer};
    use crate::{graph, strict, util, walk};

    /// Owned launch context, shared (behind `Arc`) into every tool invocation so the
    /// `'static` async handlers never borrow the `Cli`. `overrides`/`excludes` mirror
    /// exactly what `lib::run` derives, so a tool builds identically to the CLI.
    struct ServerState {
        /// Roots the server was launched over — the search scope for package queries
        /// (`dependencies`/`dependents`), which take a name rather than a path.
        roots: Vec<PathBuf>,
        overrides: CliOverrides,
        excludes: GlobSet,
    }

    #[derive(Clone)]
    struct AnnotatedTree {
        state: Arc<ServerState>,
    }

    /// A tool failure, split by whose problem it is (rmcp draws the same line):
    /// `Tool` becomes an `isError` result the caller sees (bad path, runaway-scope
    /// trip); `Internal` becomes a JSON-RPC protocol error (a bug on our side).
    enum ToolError {
        Tool(String),
        Internal(String),
    }

    impl ToolError {
        fn into_result(self) -> Result<CallToolResult, ErrorData> {
            match self {
                // Tool-level error: the server ran the tool and it failed in a way
                // the caller should see and act on. Crucially, a runaway-scope trip
                // lands here (NOT a process exit) so the long-lived server survives.
                ToolError::Tool(msg) => Ok(CallToolResult::error(vec![ContentBlock::text(msg)])),
                ToolError::Internal(msg) => Err(ErrorData::internal_error(msg, None)),
            }
        }
    }

    fn ok_json(value: &serde_json::Value) -> Result<CallToolResult, ErrorData> {
        // Serialization of plain owned data cannot fail (DbC — both sides ours).
        let text = serde_json::to_string_pretty(value).expect("tool payload serializes to JSON");
        Ok(CallToolResult::success(vec![ContentBlock::text(text)]))
    }

    /// The runaway-scope trip, phrased for the server: it stays alive, so the fix is
    /// to relaunch with a higher cap (never a silent truncation — Fail-Fast).
    fn limit_message(e: &walk::LimitExceeded) -> String {
        format!(
            "aborted: '{}' has more than {} code files (limit --max-files {}). \
             Relaunch the MCP server with a higher --max-files <N> (or --no-limit).",
            e.root.display(),
            e.limit,
            e.limit,
        )
    }

    #[derive(Debug, Deserialize, schemars::JsonSchema)]
    struct MapArgs {
        /// Directory to map (the codebase root or a subtree of it).
        path: String,
        /// Optional maximum directory depth to expand.
        max_depth: Option<usize>,
        /// Optional git ref: restrict the map to files changed since it, plus their
        /// blast radius (the same `--since` filter the CLI applies).
        since: Option<String>,
    }

    #[derive(Debug, Deserialize, schemars::JsonSchema)]
    struct PackageArgs {
        /// Package name as it appears in its manifest.
        package: String,
    }

    #[derive(Debug, Deserialize, schemars::JsonSchema)]
    struct PathArgs {
        /// Directory to lint.
        path: String,
    }

    #[tool_router]
    impl AnnotatedTree {
        /// Map a directory to the versioned JSON codebase model (the #1 wire schema).
        #[tool(
            description = "Render a directory's annotated dependency map as versioned JSON (schema 1)."
        )]
        async fn map(
            &self,
            Parameters(args): Parameters<MapArgs>,
        ) -> Result<CallToolResult, ErrorData> {
            let state = self.state.clone();
            // ASYNC BOUNDARY: config load + directory walk + graph + model build are
            // all blocking filesystem work — run them on the blocking pool, never the
            // async executor. The builders stay sync and untouched.
            let built = tokio::task::spawn_blocking(move || build_map(&state, args))
                .await
                .map_err(join_error)?;
            match built {
                Ok(payload) => Ok(CallToolResult::success(vec![ContentBlock::text(payload)])),
                Err(e) => e.into_result(),
            }
        }

        /// The internal dependencies a package declares.
        #[tool(description = "List the internal packages a given package depends on.")]
        async fn dependencies(
            &self,
            Parameters(args): Parameters<PackageArgs>,
        ) -> Result<CallToolResult, ErrorData> {
            let state = self.state.clone();
            // ASYNC BOUNDARY: manifest discovery + parse (graph::build) is blocking.
            let built = tokio::task::spawn_blocking(move || dependencies(&state, &args.package))
                .await
                .map_err(join_error)?;
            match built {
                Ok(value) => ok_json(&value),
                Err(e) => e.into_result(),
            }
        }

        /// The packages that depend on a package (reverse edges).
        #[tool(description = "List the internal packages that depend on a given package.")]
        async fn dependents(
            &self,
            Parameters(args): Parameters<PackageArgs>,
        ) -> Result<CallToolResult, ErrorData> {
            let state = self.state.clone();
            // ASYNC BOUNDARY: manifest discovery + parse (graph::build) is blocking.
            let built = tokio::task::spawn_blocking(move || dependents(&state, &args.package))
                .await
                .map_err(join_error)?;
            match built {
                Ok(value) => ok_json(&value),
                Err(e) => e.into_result(),
            }
        }

        /// Run `--strict-check` over a directory and return its report.
        #[tool(
            description = "Lint a directory for conforming file annotations (+ configured architectural rules); returns the report and pass/fail."
        )]
        async fn strict_check(
            &self,
            Parameters(args): Parameters<PathArgs>,
        ) -> Result<CallToolResult, ErrorData> {
            let state = self.state.clone();
            // ASYNC BOUNDARY: the walk + per-file annotation reads are blocking.
            let built = tokio::task::spawn_blocking(move || strict_check(&state, args))
                .await
                .map_err(join_error)?;
            match built {
                Ok(value) => ok_json(&value),
                Err(e) => e.into_result(),
            }
        }
    }

    #[tool_handler]
    impl ServerHandler for AnnotatedTree {
        fn get_info(&self) -> ServerInfo {
            let mut info = ServerInfo::default();
            info.capabilities = ServerCapabilities::builder().enable_tools().build();
            info.instructions = Some(
                "annotated-tree over MCP: `map` renders a directory's annotated dependency \
                 map as versioned JSON; `dependencies`/`dependents` query the package graph \
                 by name; `strict_check` lints annotations. Paths are resolved on the server."
                    .to_string(),
            );
            info
        }
    }

    /// A blocking task panicked or was cancelled — a server-side bug, not the caller's.
    fn join_error(e: tokio::task::JoinError) -> ErrorData {
        ErrorData::internal_error(format!("tool task failed: {e}"), None)
    }

    /// Load config the same way `lib::run` does, discovering `.annotated-tree.toml`
    /// by walking up from `root` (DRY with the CLI's resolution).
    fn load_config(root: &std::path::Path, overrides: &CliOverrides) -> Result<Config, ToolError> {
        Config::load(root, overrides).map_err(|e| ToolError::Internal(format!("{e:#}")))
    }

    fn require_dir(path: &str) -> Result<PathBuf, ToolError> {
        let root = PathBuf::from(path);
        if root.is_dir() {
            Ok(root)
        } else {
            Err(ToolError::Tool(format!("'{path}' is not a directory")))
        }
    }

    /// Thin adapter over the SHARED build pipeline (`crate::build_codebase_map`), so
    /// the `map` tool's schema-1 JSON is byte-for-byte the CLI's `--format json` for
    /// the same inputs — including `--since`/blast-radius filtering and graph warnings.
    /// A runaway-scope trip or a bad `--since` ref becomes a tool error the caller
    /// sees (never a process exit — the server stays alive).
    fn build_map(state: &ServerState, args: MapArgs) -> Result<String, ToolError> {
        let root = require_dir(&args.path)?;
        // `build_codebase_map` loads the root's own `.annotated-tree.toml` from the
        // shared overrides (per-root config), so the map tool needs no separate load.
        let (map, _warnings, _ascii) = crate::build_codebase_map(
            std::slice::from_ref(&root),
            &state.overrides,
            &state.excludes,
            args.since.as_deref(),
            args.max_depth,
        )
        .map_err(|e| match e {
            // The runaway-scope cap and a caller-supplied bad ref are both
            // caller-actionable, so surface them as tool errors (not JSON-RPC faults).
            crate::BuildError::Limit(le) => ToolError::Tool(limit_message(&le)),
            crate::BuildError::Other(err) => ToolError::Tool(format!("{err:#}")),
        })?;
        Ok(JsonRenderer.render(&map))
    }

    fn dependencies(state: &ServerState, package: &str) -> Result<serde_json::Value, ToolError> {
        let graph = build_graph(state)?;
        let pkg = find_package(&graph, package)?;
        Ok(json!({ "package": pkg.name, "dependencies": pkg.internal }))
    }

    /// Build the dependency graph over all roots for the `dependencies`/`dependents`
    /// tools, applying the PRIMARY (first) root's ignore settings to the manifest walk
    /// — the same primary-root precedent the CLI uses for a multi-root run, so the
    /// graph reflects exactly the manifests the tree would show.
    fn build_graph(state: &ServerState) -> Result<graph::Graph, ToolError> {
        let config = load_config(&state.roots[0], &state.overrides)?;
        Ok(graph::build(
            &state.roots,
            config.display.gitignore,
            config.display.include_tests,
            &state.excludes,
        ))
    }

    fn dependents(state: &ServerState, package: &str) -> Result<serde_json::Value, ToolError> {
        let graph = build_graph(state)?;
        // Confirm the package exists (Fail-Fast) so an empty list means "no dependents",
        // not "unknown package".
        let target = find_package(&graph, package)?.name.clone();
        // Reverse edges are the resolved internal deps pointing at `target` — the same
        // relation `DirDeps.used_by` encodes, derived here from the shared edge list.
        let mut names: Vec<String> = graph
            .packages
            .iter()
            .filter(|p| p.internal.iter().any(|d| d.resolved && d.name == target))
            .map(|p| p.name.clone())
            .collect();
        names.sort();
        names.dedup();
        Ok(json!({ "package": target, "dependents": names }))
    }

    fn find_package<'g>(
        graph: &'g graph::Graph,
        package: &str,
    ) -> Result<&'g graph::PackageEdges, ToolError> {
        graph
            .packages
            .iter()
            .find(|p| p.name == package)
            .ok_or_else(|| {
                let mut known: Vec<&str> = graph.packages.iter().map(|p| p.name.as_str()).collect();
                known.sort_unstable();
                ToolError::Tool(format!(
                    "no package named '{package}' in the scanned roots. Known packages: [{}]",
                    known.join(", ")
                ))
            })
    }

    fn strict_check(state: &ServerState, args: PathArgs) -> Result<serde_json::Value, ToolError> {
        let root = require_dir(&args.path)?;
        let config = load_config(&root, &state.overrides)?;
        let files = walk::collect_code_files(&root, &config, &state.excludes)
            .map_err(|e| ToolError::Tool(limit_message(&e)))?;
        // Thin adapter over the ONE shared strict composition (`strict::check_with_rules`,
        // also driven by the CLI's `--strict-check`): annotation linting plus any
        // configured architectural `[rules]`, folded into one report + exit code — so the
        // MCP verdict is byte-for-byte the CLI's for the same directory.
        let (report, code) = strict::check_with_rules(&root, &files, &config, &state.excludes);
        Ok(json!({ "passed": code == 0, "report": report }))
    }

    /// SYNC outer signature: create and own the tokio runtime, block on the async
    /// server, and hand callers back a plain exit code. The `map` tool renders the
    /// same schema-1 JSON the CLI's `--format json` emits (DRY — one wire contract).
    pub fn serve(cli: &Cli) -> Result<i32> {
        // Share the CLI's root resolution (don't copy it) so the server scopes exactly
        // like `annotated-tree <paths>` would.
        let roots = crate::resolve_roots(&cli.paths)?;
        let overrides = cli.overrides();
        let excludes = util::build_globset(&cli.ignore).context("building exclude matcher")?;
        let state = Arc::new(ServerState {
            roots,
            overrides,
            excludes,
        });
        let server = AnnotatedTree { state };

        let runtime = tokio::runtime::Runtime::new().context("starting tokio runtime")?;
        runtime.block_on(async move {
            let service = server
                .serve(stdio())
                .await
                .context("starting MCP stdio server")?;
            service.waiting().await.context("MCP server error")?;
            Ok::<(), anyhow::Error>(())
        })?;
        Ok(0)
    }
}
