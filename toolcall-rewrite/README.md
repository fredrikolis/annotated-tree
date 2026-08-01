<!-- Concern: what toolcall-rewrite does and exactly how to switch it on | Non-concern: the annotation format (docs/annotation-guide.md owns it) and the invariants governing annotated-tree itself (SPEC.md) | IO: none -->
# toolcall-rewrite

Your agent starts every task by searching. What comes back is a list of paths that says
nothing about what any of them is **for**, so it opens them to find out.

This puts the answer in the search result itself.

```
$ grep -rl "Renderer" src            # what your agent asked for
src/lib.rs
src/mcp.rs
src/model.rs
src/render/json.rs
src/render/md.rs
src/render/mod.rs
src/render/text.rs

$ grep -rl "Renderer" src            # what it gets with this installed
src/lib.rs  # Concern: the library surface — run(), which drives config, the walk, and either tree or strict output | …
src/mcp.rs  # Concern: the MCP stdio server — the crate's one async surface — exposing the map, graph, and strict-check builders as tools | …
src/model.rs  # Concern: the canonical in-memory codebase map and every filesystem read behind it | …
src/render/json.rs  # Concern: serializes the canonical map as the versioned, machine-readable JSON contract | …
src/render/md.rs  # Concern: formats the canonical map as human-facing Markdown | Non-concern: filesystem reads | …
src/render/mod.rs  # Concern: the renderer seam — the `Renderer` trait, the format -> renderer factory | …
src/render/text.rs  # Concern: formats the canonical map as a `tree`-style text view | Non-concern: filesystem reads | …
```

Same command, same files, same order, one line each. Every file's contract is appended to
its own line. Your agent learns no new tool and types nothing different. A hook pipes the
command's output through an annotator before the agent sees it.

The `  # ` separator is the one [`annotated-tree`](../README.md) itself renders, so a contract
looks the same wherever an agent meets it, and `#` marks the text as a note about the line
rather than part of what the tool printed.

*(Contracts abridged above for width, and the lines sorted for readability. The wrapper
itself does neither.)*

The one thing it does shorten is a contract containing a newline, which it cuts at the first
one. A contract has to fit on the line it describes.

**Prerequisite:** your files need annotations for there to be anything to show. See the
[main README](../README.md).

## Set it up

**1. Install with cargo.** The binaries ship in the `annotated-tree` crate:

```sh
cargo install annotated-tree
```

That puts `annotated-tree`, `annotated-bash-wrapper` and `annotated-toolcall-rewrite` on
your `PATH`. **Only `cargo install` builds them.** The curl installer, npm and
`cargo binstall` each fetch a single prebuilt binary.

**2. Turn the hook on.**

```sh
annotated-toolcall-rewrite --install-hook
```

That merges one `PreToolUse` entry into `~/.claude/settings.json`, so the hook is on for
every project. For one repo instead, name the file:

```sh
annotated-toolcall-rewrite --install-hook .claude/settings.local.json
```

**It merges; it does not overwrite.** That file holds the permissions you have accepted,
and every other key in it survives, including any hook someone else put there. Running it
twice changes nothing, and `--uninstall-hook` takes out only the entry it added.

This is the entry it adds, so you can write it by hand or check what landed:

```json
{
  "hooks": {
    "PreToolUse": [
      {
        "matcher": "Bash",
        "hooks": [{ "type": "command", "command": "annotated-toolcall-rewrite" }]
      }
    ]
  }
}
```

**If you use a Bash allowlist, read this.** `PreToolUse` runs *before* permissions are
evaluated, so the string your rules are matched against is the rewritten one, which begins
`( grep …` and names the annotator by absolute path. A `Bash(grep:*)` rule stops matching,
and searches that used to be auto-approved start prompting. A rule matching the rewritten
shape has to encode the annotator's absolute path, which differs per machine, so there is no
portable one to ship. The options are to write that machine-local rule yourself, allow `Bash`
broadly, approve the prompts, or leave the hook off where the allowlist matters more than the
contracts.

`settings.local.json` is per-user and normally gitignored, so this affects you and not
your team until you decide otherwise.

**3. Check it worked.** Start a session and ask your agent to `grep` for something; the
results come back carrying contracts. To see the decision for any command without running
it:

```sh
$ annotated-toolcall-rewrite --check 'grep -rn Renderer src'
( grep -rn Renderer src | '/home/you/.cargo/bin/annotated-bash-wrapper' grep -rn Renderer src ;
  __ps=("${PIPESTATUS[@]}"); __rc=${__ps[0]};
  [[ -o pipefail ]] && for __s in "${__ps[@]}"; do ((__s)) && __rc=$__s; done; exit $__rc )
```

(Shown across three lines for width; it is emitted as one.)

Your command runs **exactly as you wrote it**, same program and same flags, and its output is
piped into the annotator. The argv is repeated so the annotator knows which directory bare
names are relative to: `ls docs` prints `x.rs`, meaning `docs/x.rs`.

The tail is exit-code recovery. Appending a stage would otherwise make the pipeline report the
annotator's status instead of your tool's, so the original stage's status is read back out of
`PIPESTATUS` and re-raised, so `grep`'s exit 1 on no-match still means no-match. The `pipefail`
branch is there because under `set -o pipefail` bash takes a pipeline's status from the rightmost
failing stage, and that option may have been set by an earlier command in the same shell.

## What gets rewritten

| your agent types | where that tool names the file a line is about |
|---|---|
| `ls` | the line, or its trailing field |
| `find` | the line |
| `grep` | the `path:` prefix (anything after it is file content, not a subject) |

Everything else is left exactly as written. Adding a tool is one line in
[`src/map.rs`](src/map.rs), which buys the eligibility decision and the choice between the two
shapes above: the annotator describes whatever paths a tool printed rather than predicting
where they will be. It does not buy the tool's flag grammar. `ls` and `grep` each have a table
in [`src/run.rs`](src/run.rs) naming the options that take a value, which is what tells an
operand from a flag's argument; a new tool starts with an empty one and needs its own if its
flags matter.

**No tool is ever substituted.** The program token is left exactly as your agent typed it, so
your shell resolves it exactly as it would have and the answer is identical to the
un-annotated one. This matters where a shell defines `grep` or `find` as a function, as
Claude Code does: spawning the binary instead would search a different file set. `\grep`
therefore needs no special handling. It runs the real binary, exactly as you asked, and its
paths still get contracts. `sudo grep` and `/usr/bin/grep` are left alone instead, because
the program token reads as `sudo` or as a path and neither is in the map; they run untouched
and carry no contracts.

## Why it is safe to leave on

**One line in, one line out.** A contract is appended to the line its path appeared on and
never printed on a line of its own, so no downstream stage sees a line count it did not
expect.

**The annotator is the LAST stage**, so `| sort`, `| uniq`, `| sort -u` and `| grep -v` all
see your tool's raw output and behave exactly as they would without this installed,
ordering real paths and collapsing real duplicates. `| wc` is **refused**, because what would
reach the annotator is a count rather than a list of paths, and `| head -c` is refused
because a byte count can cut a record in half.

**It refuses anything it cannot guarantee.** A command is left alone whenever the lines
reaching the annotator would no longer be the paths the tool printed: rewritten by `| sed`,
counted by `| wc` or `| grep -c`, byte-truncated by `| head -c`, redirected to a file,
consumed by `xargs` or `find -exec`, emitted NUL-delimited (`grep -Z`, `find -print0`),
wrapped in `$( )` or in a `for`/`while`/`if` body, or written in a form it cannot parse. A
missed rewrite costs nothing; a wrong one would corrupt a pipeline you then have to debug.

One refusal is easy to miss because the command looks harmless. `ls -R` and `ls docs src`
print `<dir>:` headers and then name entries relative to the last one, so their output means
what it says only in the order printed. Anything downstream that reorders it (`| sort`),
drops from the middle (`| grep -v`), or renumbers the lines (`| nl`, `| cat -n`) detaches a
header from its block, so those combinations are refused too. `| cat` and `| head` keep both
the order and the text, and stay.

It emits no permission decision of its own, so nothing is ever waved through that would not
have been, and it never blocks a command. The exit code is re-raised as the command would
have reported it, with `set -o pipefail` honoured. Two things it does change: the string
permission rules are matched against (see the note in step 2), and, because the pipeline is
wrapped in a subshell, `${PIPESTATUS[i]}` inspected *afterwards* would describe that
subshell. A command mentioning `PIPESTATUS` is therefore left alone entirely. `$?` is unaffected.

**It makes output bigger.** A contract runs about 200 bytes. Spend those tokens once at the
listing, and the intent is that the agent stops opening files for the rest of the session to
find out what they are: the same bargain `annotated-tree` makes with the tree, moved onto your
agent's own tool calls. A result large enough to truncate costs one more tool call to narrow
it.

We expect the effect to compound past the token count, because an agent that did not fill its
context with five speculative file reads has more of it left for the decision. That is the
mechanism, not a measurement:
[the extended argument](../README_APPENDIX.md#future-work) names first-pass yield as the thing
it most wants measured and has not measured.

## Turning it off

```sh
annotated-toolcall-rewrite --uninstall-hook
```

Name the same file you installed into, if it was not the default. It removes only its own
entry and leaves the rest of the file alone. The binaries stay installed and do nothing; no
other file changes.
