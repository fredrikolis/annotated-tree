<!-- Concern: universal programming principles - KISS, YAGNI, DRY, dependency inversion (SOLID mapped to first principles), DbC, canonical representation at boundaries, fail-fast, SoC, agent UX, file size, file annotations | Non-concern: language- and interface-specific patterns and standards (concrete timestamp/timezone forms, idiomatic types, per-language testing, CLI grammar such as envelope shapes and exit-code tables) | IO: none -->
# Repo Standards

Universal principles. All languages. All paradigms.

---

## AUTO-REJECT (Stop Work Immediately)

**Universal Blockers** (-∞):

- **Circular imports**: Module A imports B, B imports A → Restructure
- **Failing tests**: All tests pass before commit
- **Hardcoded secrets**: API keys, passwords, tokens in code → Environment variables
- **Force push to main/master**: Never on protected branches

---

## PART 1: Decision-Making Philosophy

### Evidence-Based Decisions

**Measure → Decide. Not: Opinion → Decide.**

| Cargo Cult                   | Evidence Required                             |
| ---------------------------- | --------------------------------------------- |
| "Framework X is best"        | Benchmark for THIS use case                   |
| "Microservices scale better" | Measured current bottleneck? Need that scale? |
| "NoSQL is faster"            | Profiled for THIS query pattern?              |

**Default**: Boring technology. Optimize when proven necessary.

---

### KISS (Keep It Simple)

**Simplest working solution wins.**

**Decision filter**:

```
Problem → Simplest approach works?
  ├─ Yes → Ship it
  └─ No → Justify (benchmark/requirement/evidence)
```

| Justified Complexity                        | Unjustified Complexity          |
| ------------------------------------------- | ------------------------------- |
| Benchmark proves simple approach inadequate | Premature optimization          |
| Current solution demonstrably fails         | Framework for single use case   |
| Explicit requirement demands it             | "Future-proofing" hypotheticals |

**Three duplicate lines > premature abstraction.**

---

### YAGNI (You Aren't Gonna Need It)

**Build for today. Not tomorrow.**

| Situation            | Ship This          | Not This                        |
| -------------------- | ------------------ | ------------------------------- |
| Single output format | That format        | Pluggable format system         |
| One user type        | One implementation | Role-based permission framework |
| Local deployment     | Local setup        | Cloud-agnostic abstraction      |
| Fixed config         | Hardcoded values   | Dynamic config system           |

**Exception**: Extensibility explicitly required → Design for extension, implement one.

---

## PART 2: Architecture & System Design

### Separation of Concerns (SoC)

**Every unit does its job and stays out of every other unit's job.**

SoC is not a layering rule — it is the *ownership* rule, and it recurses at every scale. One question, asked of a package, a file, a class, a function, a variable:

**What is this thing's ONE job, and what is explicitly NOT its job?**

| Scale | Does its job | Stays out of others' jobs |
|-------|--------------|---------------------------|
| Package / crate | Owns one capability | No reaching into another package's internals |
| File / module | Owns one concern | No neighbor's work (see File Annotations `Non-concern:`) |
| Class / type | One reason to change (→ SRP) | No knowledge of another type's internals |
| Function | One thing, one level of abstraction | No reaching across a call boundary to fix a caller's mistake |
| Variable / name | Holds one meaning | Not recycled for a second purpose (no reused `tmp`, no overloaded flag) |

**Layering is SoC applied to runtime dependencies** — one special case, not the whole principle:

```
Presentation (API/UI)  →  Business Logic  →  Data Access
```

Each layer depends only on the layer below. Never above.

**SoC governs several principles here.** A violation surfaces downstream as defensive code (DbC — doing a job you don't own), a leaking API (Minimal API — blast radius crossing a boundary), or a multi-concern monolith (File Size — split by concern *first*). Fix it at the source: give each unit one job.

#### Refactoring lens: keep vs move vs delete

Every refactor is an ownership audit — the *what* to do; Remove-then-Replace covers the *how* of a rewrite. Ask in order:

1. **Intended** — what was this supposed to own? (its name, its reason to exist — the contract)
2. **Actual** — what does it handle now? (the drift from #1)
3. **Live** — does anyone still care about that concern?

The verdict falls out — argue the concern, never the code:

| Intended vs Actual | Concern wanted? | Verdict |
|--------------------|-----------------|---------|
| Doing exactly its job | Yes | **Keep** |
| Its job **+ extra** | Yes | **Split** — extract the extra to its rightful owner |
| A **different** job than its name claims | Yes | **Move / rename** to where that concern lives |
| Its job fine, concern is **dead** | No | **Delete** |
| Its job, concern **already owned elsewhere** | Owned better elsewhere | **Delete, consolidate** (→ DRY) |

**"Should we just delete it?" is the most under-asked refactor question.** A unit can do its job perfectly and still deserve deletion — because no one owns, wants, or needs its concern anymore. Doing a dead job well is still waste.

---

### Dependency Inversion

**Depend on abstractions, not concretions. Point dependencies at stability.**

Ownership includes owning your dependency *direction*. High-level policy must not depend on low-level detail; both depend on an abstraction (interface, trait, protocol) — the volatile concrete depends on the stable abstract, never the reverse. This is SoC applied to the direction of coupling: the *what* (contract) and *how* (implementation) change independently, and the unit becomes testable, its collaborators swappable.

| Pattern | Score | Notes |
|---------|-------|-------|
| Policy depends on an abstraction; detail implements it | +10 | Stable core, swappable edges |
| Concrete injected behind an interface/trait | +9 | Testable, decoupled |
| High-level module imports a concrete low-level one | -8 | Volatile detail drags policy with it |
| Abstraction that leaks its single implementation | -6 | Not an abstraction — a rename |

*SOLID mapped to first principles: SRP → SoC (class scale) · LSP → DbC · ISP → Minimal API · OCP → Dependency Inversion · DIP → Dependency Inversion (here).*

---

### Minimal API Surface

**Expose minimum necessary interface.**

- Internal details: Private
- Public API: Minimal, stable
- Implementation: Changeable without breaking clients

**Result**: Smaller blast radius. Easier to understand. Harder to misuse.

**From the consumer's side** this is interface segregation: a client depends only on the slice it uses, never on capability it doesn't touch.

---

### Agent UX — Design for the Agent as Primary User

**When a tool's primary consumer is an AI agent, agent UX IS the UX. Any commit that touches the invocation surface — command syntax, flags, defaults, output, errors, exit codes, `--help` — is an agent-UX change, and is scored on whether an agent parses, trusts, and acts on it more reliably.**

An agent invokes the tool programmatically — it parses the output, branches on it, and pays a token/latency cost per call. The human reading the same run is the *dual-render* of one structured object, never a separate code path. Design for the agent first; the human view falls out for free. The test for every surface change: does it convert an act of inference into an act of reading? And the surface only ratchets forward — a regression in agent ergonomics is a blocker, not a tradeoff.

Beyond any single call, the compounding value is *comprehension without reading source* — the agent routes a change or judges a boundary from the rendered map, not by re-deriving it from the code. Optimize the surface so the agent trusts it enough to act *without* opening the files it summarizes; a map that still forces a source-read to resolve a routing question has leaked its job back to the code.

The objective behind all of it: **maximize the share of the agent's work that is productive** — output kept, not code caught in review and redone. Every wasted token — a wrong change, a re-read forced by an opaque surface — is the cost. Good agent UX raises that productive fraction toward the ceiling where nearly everything the agent emits is worth keeping; poor agent UX spends the agent's scarcest resource (context) on re-derivation and its output on work that gets thrown away.

**Core contract**:

- **Parseable** — structured data (JSON envelope) to stdout, progress/debug to stderr, never mixed
- **Unambiguous empties** — empty is a first-class value (`[]`, zero count), distinct from error and from not-found; one null convention, never mixed
- **Stable dispatch keys** — agents branch on a namespaced `code`, never on message prose; prose may change, codes are an API
- **Syntax is an API too** — a flag rename, output reshape, or default flip is a breaking change to unattended callers; it lands with its `--help`/schema/docs update in the same commit
- **Located, fixable diagnostics** — `code` + `location` (byte span and line:col) + `fix`, one object per finding, not one opaque error string that discards count, location, and remedy
- **Non-interactive** — nothing on the default path blocks; gate danger behind `--confirm`/`--yes`
- **Deterministic** — same input → same output; meaningful, consistent exit codes to branch on
- **Verdict-driven exit** — `status`/exit code follow the verdict (input rejected or not), never "any diagnostics present"; a warning on accepted input is not a failure
- **Token-economical** — dense, zero filler; context is the agent's scarcest resource, and noise degrades it, compounding across retry loops
- **Self-correcting `--help`** — usage, examples, output schema, exit codes, so an agent repairs its own call without a human

The contract above is universal; the concrete grammar — envelope shape, exit-code table, verb and flag conventions — is an interface-level concern, out of scope here.

| Pattern | Score | Notes |
|---------|-------|-------|
| New output path: structured, stdout-clean, dispatchable | +10 | Agent parses and branches reliably |
| Diagnostic carries `code` + `location` + `fix` | +9 | Agent applies, doesn't infer |
| Flag renamed, output reshaped, or default flipped without same-commit `--help`/schema/docs | -9 | Breaks unattended agent callers mid-run |
| One canonical object, dual-rendered to human + JSON | +8 | No second code path to drift |
| Human-only, unparseable output ("Done!") from an agent-first tool | -9 | Human-first regression; breaks the primary consumer |
| Agent forced to branch on message text | -8 | Brittle — prose drift breaks callers |
| Interactive prompt on the default path | -10 | Hangs autonomous execution |
| Warning flips the exit code on accepted input | -8 | Every warning halts unattended automation |
| Empty, null, error, and not-found conflated in output | -7 | Agent can't branch; forces a re-run or source-read |
| Progress/debug on stdout, corrupting the parse | -8 | Poisons the data stream |
| Tool makes the agent's semantic call (guesses whether a thing is true / right / dead) | -7 | Non-deterministic; can't be trusted to branch on, and invites scope-creep |

**Render, don't reason.** The tool's job is to make state *observable* — deterministically and cheaply — not to make the semantic judgments the agent exists to make (is this annotation *true*? does this change *belong*? is this code *dead*?). Keep the tool simple and the intelligence in the agent: a zero-inference, deterministic surface is more trustworthy to branch on than a "smart" one that can be wrong, and it keeps the tool's own scope honest. This is Separation of Concerns at the tool↔agent boundary — the complexity belongs in the agent, not the instrument.

#### When the output is an optimization target (an agent's fitness function)

Some agent-first tools do more than report — their signal becomes something an agent *optimizes against*, closing a dev loop around it the way it closes one around tests (behavior) or types (contracts). When the output is a target, three properties decide whether the loop improves the real thing or just games the proxy (Goodhart's law):

- **Observable** — a machine-readable, dispatchable signal (stable `code`s + counts), never a human verdict the agent must interpret.
- **Convergent** — a *gradient*, not a pass/fail gate. Emit a distance-to-done (N of M, a decreasing violation count) so the agent knows it is getting warmer and can recognize *done*. A binary gate is a weak target; a slope is a strong one.
- **Goodhart-resistant** — satisfying the metric must *require* improving (or honestly reporting) the underlying property. Reject filler that passes the format but carries no meaning; an anti-filler gate must cover *every* required slot, because gating some fields but not all just relocates the filler to the unchecked one. Reward **honesty over tidiness** — surface the real state (dead code, cycles, overlapping responsibilities), never a description that conceals a mess; honest overlap between two units is an architecture finding (keep/move/delete), not something to reword away. Anchor the signal in what the tool *observes* about the system and cross-check it against what the code *self-reports*: the discrepancy between claim and reality is the least gameable signal of all.

| Pattern | Score | Notes |
|---------|-------|-------|
| Metric is a gradient with an explicit distance-to-done | +9 | Agent can converge and know when finished |
| Signal anchored in observed facts, cross-checked vs self-report | +10 | Discrepancy is un-gameable |
| Passing requires improving the real property | +9 | Optimizer and intent aligned |
| Metric satisfiable by filler / tidy-but-false reporting | -10 | Goodhart — optimizes the proxy, corrupts the loop |
| Anti-filler gate covers some required slots but not all | -6 | Filler relocates to the ungated field |
| Binary pass/fail with no convergence signal | -5 | Weak target; agent can't tell it's getting warmer |

**The rendered map is itself an optimization target, kept honest at a human-authored ceiling by two checks.** A *charter* — a package/repo-scale annotation whose `Non-concern:` clauses are concrete enough to *reject* an ill-fitting feature by naming the sibling that owns it, not a strawman (a repo charter rejecting "add a program executor" because that is a runtime tool; a service charter rejecting "parse rules in the handler" because a named CLI owns that). And a *stress test* — replaying realistic change-requests to confirm each routes to exactly one unit from the map alone. A charter too vague to reject scope-creep is the map's failure mode, not the agent's.

**Applies when** agents are a primary caller (CLIs agents invoke, MCP servers, function-calling tools, batch/CI interfaces). Human-primary tools (interactive TUI, GUI) optimize for the human and treat agent-parseability as secondary. State which consumer is primary; don't split the difference.

---

### File Size — Agent-Manageable Modules

**Keep files at a size an agent can hold and edit confidently — split ONLY at a natural seam.**

A large file taxes every agent operation — re-reads, ambiguous exact-match edits, hidden diffs, serialized parallel work.

Split when a file outgrows the budget **and** has a seam the code already has (phases, construct-families, strands). A behavior-preserving split is gated like any change — a contract/test proving equivalent behavior before/after.

**Heuristic (not a hard line)**: ~1.5–2k lines AND a clean seam → split; else leave it.

| Pattern | Score | Notes |
|---------|-------|-------|
| Cohesive module split at a natural seam when it outgrows the budget | +9 | Bounded, reviewable, parallelizable |
| Behavior-preserving split, gated by a contract/test | +9 | Equivalence proven before/after |
| Cohesive file left intact at size (no natural seam) | +5 | Correct — don't split for its own sake |
| Forced split fragmenting one concern across files | -9 | Worse than the monolith — scatters cohesion |
| Unbounded growth of a hot file (no size discipline) | -7 | Compounding tax; serializes parallel work |

A multi-concern monolith is a Separation of Concerns problem, not a file-size one — split by concern first (see SoC), then apply size discipline within each.

---

### Refactoring: Remove-then-Replace

**Delete old → Build new. Boundary tests are the spec.**

```
Phase 1: REMOVE              Phase 2: BUILD
──────────────               ─────────────
Delete old implementation    Implement new
Delete internal tests        Build to pass boundary tests
Keep boundary tests
```

| Delete | Keep |
|--------|------|
| Old implementation | Boundary/contract tests |
| Unit tests of internals | Integration tests at edges |
| Tests coupled to old structure | Tests that define WHAT, not HOW |

**Why delete internal tests**: They constrain new implementation to match old structure.

**Scoring**:

| Pattern | Score | Notes |
|---------|-------|-------|
| Remove old, keep boundary tests | +10 | Clean slate, tests = spec |
| Delete internal tests during rewrite | +9 | No structural constraint |
| Preserving old code "for reference" | -8 | Shapes new implementation |
| Keeping internal tests during rewrite | -6 | Constrains to old structure |

---

## PART 3: Code Design & Implementation

### Design by Contract (DbC)

**Own both sides → know contract → fail fast. No defensive code for own types.**

**Defensive code ONLY for**: External APIs | User input | Library boundaries | Migration compat*

**NOT for**: Own modules | Own data structures | Internal packages | Code you control

*\*Migration compat: ONLY when production consumers exist outside your control. In monorepos where you control both sides of a contract, backwards-compat shims are a MAJOR VIOLATION - update all call sites instead.*

**Red flags** (defensive ignorance):

| Pattern                                  | Problem                        | Fix                                 |
| ---------------------------------------- | ------------------------------ | ----------------------------------- |
| `x = a or b or c`                        | Which is it? You control it.   | Trace producer, pick ONE            |
| `value?.deeply?.nested \|\| default`     | Contract uncertainty           | Document structure, access directly |
| `if (response.success \|\| response.ok)` | What does YOUR API return?     | Pick one, document                  |
| `isinstance(x, (A, B, C))`               | Multiple types from YOUR code? | Unify contract, single type         |

```python
# GOOD: DbC
def handle_event(event: Event):
    return event.data['value']  # Precondition: event.data has 'value'. Fail fast if violated.
```

**Subtypes too**: a subtype must honor its base type's contract (Liskov substitution) — one that quietly does a different job is a broken contract, not a variant.

**DbC = DRY**: Validate once at boundary, trust internally.

---

### Canonical Representation at Boundaries

**One canonical internal form. Convert only at the edges.**

Pick a single representation for each quantity in the core (timestamps → UTC epoch; money → integer minor units; text → normalized form). Store, compare, and compute in that form exclusively; convert to/from local or display forms only at I/O boundaries. Never let two representations coexist in the interior.

| Pattern | Score | Notes |
|---------|-------|-------|
| One canonical form, converted at the edge | +10 | No ambiguity internally |
| Convert to local/display at I/O boundary only | +9 | Surface concern, not core |
| Two representations mixed in the core | -10 | Which is authoritative? |
| Boundary value stored without normalizing | -8 | Drift, comparison bugs |

Language-specific canonical forms (e.g. a language's idiomatic timestamp type) are a language concern, out of scope here.

---

### Fail Fast

**Detect errors at source. Not downstream.**

```python
def process_user(user):
    if 'name' not in user or 'email' not in user:
        raise ValueError(f"Invalid user: missing required fields. Got: {user.keys()}")
```

**Explicit error > Silent fallback > Runtime confusion**

---

### DRY (Don't Repeat Yourself)

**Single source of truth for knowledge/logic.**

**Eliminate duplication when**:

- Same knowledge (repeated business logic)
- Same behavior (identical algorithm, multiple locations)
- Single source (one change affects all)

**Duplication OK when**:

- Accidental similarity (looks similar, different concepts)
- Decoupling needed (independent modules)
- Premature abstraction (too early to know)

**Rule of Three**: Duplicate once (2 instances), refactor at third.

**Wrong abstraction > duplication.**

---

### Documentation: Self-Documenting Code

**Well-written code documents itself. Docstrings = DRY violation.**

**Write docstrings ONLY when**:

- External consumer requires it (API doc generators, decorators, frameworks)
- Public library interface (consumed outside your control)
- Complex algorithm requiring mathematical/domain explanation

**Do NOT write docstrings for**:

- Internal functions/methods (code you control)
- Obvious implementations (name + signature + body tell the story)
- Simple business logic (refactor unclear code instead)

**Why docstrings harm**:

| Cost | Impact |
|------|--------|
| Refactoring penalty | Change code → change docstring (2x maintenance) |
| DRY violation | Same information in signature, types, implementation, AND docstring |
| Reduced readability | More lines to scan, signal buried in noise |
| Staleness risk | Code evolves, docstrings lag, lies accumulate |

**Anti-patterns**:

```python
# BAD: Docstring repeats obvious information
def calculate_total(items: list[Item]) -> float:
    """Calculate total price of items.

    Args:
        items: List of items to calculate total for

    Returns:
        Total price as float
    """
    return sum(item.price for item in items)

# GOOD: Self-documenting
def calculate_total(items: list[Item]) -> float:
    return sum(item.price for item in items)
```

**Exceptions requiring docstrings**:

```python
# GOOD: FastAPI uses docstrings for OpenAPI docs (external consumer)
@app.post("/users")
async def create_user(user: UserCreate) -> User:
    """Create new user account with email verification."""
    ...

# GOOD: Public library, complex algorithm
def optimized_levenshtein_distance(s1: str, s2: str) -> int:
    """Compute edit distance using Wagner-Fischer O(mn) algorithm.

    Uses space optimization: O(min(m,n)) instead of O(mn).
    See: Wagner & Fischer (1974) for proof of correctness.
    """
    ...
```

**Decision flow**:

```
Need to document?
  ├─ Unclear what code does? → Refactor code (better names, extract methods)
  ├─ Decorator/framework reads docstring? → Write docstring
  ├─ Public library API? → Write docstring
  └─ Internal implementation? → No docstring
```

**Philosophy**: Code expresses intent through structure, naming, types. Docstrings duplicate. Invest in clarity, not commentary.

---

### File-Level Annotations (Codebase Discoverability)

**Every file's first line describes its responsibility.**

**The Goal**: Running Bash(annotated-tree) should describe the entire codebase's functionality. Between file names, folder structure, and first-line annotations, the app's purpose and organization should be clear _without reading any code_.

**Format** (first non-shebang, non-empty line) — one invariant shape, every file, every language: three ` | `-delimited (space-pipe-space) keyed fields.

```
<comment> Concern: <what it does> | Non-concern: <what it deliberately isn't> | IO: <(in) -> out  OR  none>
```

Or as a docstring:

```python
"""Concern: <what it does> | Non-concern: <what it deliberately isn't> | IO: (in) -> out"""
```

`Concern:` and `Non-concern:` are required and reject filler (`none`/`n/a`/`nothing`/…);
`IO:` is required but `none` is a first-class blessed value (config, data, and docs use
`IO: none`). The deliberate asymmetry — `none` is filler in Concern/Non-concern but blessed
in IO — is the point: a file must state what it does and does not own, but may legitimately
have no callable contract. The format is INVARIANT (not configurable), so a cross-repo agent
can trust the map's shape without reading each repo's config.

**Why it matters**:

- **Quick orientation**: Understand codebase without reading implementation
- **Clear boundaries**: The `Non-concern:` field prevents scope confusion (name the sibling that owns what this file leaves alone)
- **IO contracts**: See how pieces connect without tracing imports
- **Tool support**: Bash(annotated-tree) extracts these for instant codebase snapshot; `--strict-check` enforces the shape and prints the annotation guide on failure to teach it (`--no-guide` opts out)

**Folder charters (directory-scale SoC).** A directory owns one concern too — the coarsest
routing decision an agent makes ("does this change go in here?"). A directory carries its charter
in the SAME three-field grammar, promoted onto its render row, resolved most-explicit-first:

1. A `.annotation` breadcrumb in the directory — a bare, marker-less `Concern: … | Non-concern: … | IO: …`
   line whose whole subject is the directory. Overrides everything; always optional, but
   `--strict-check` enforces its shape when present (a malformed one FAILS — opting in means doing it right).
2. Else the directory's code entry file's annotation, promoted (a crate's `src/lib.rs`/`src/main.rs`,
   a module's `mod.rs`, a package's `__init__.py`, a JS/TS `index.*`, a Go `doc.go`) — so a code
   folder gets a charter for free.
3. Else nothing — a charter-less directory renders exactly as before.

The authored charter renders beside the observed dep facts (`# Concern: … · used by: […]`): claim
cross-checked against reality. The charter is a deterministic lookup, never synthesized from a
folder's children (render, don't reason). The opt-in `[rules] require_package_charter` gate can
require a manifest-bearing package to resolve one; it is off by default (a grouping folder may
honestly have no single concern).

---

## Summary

| Principle                     | Essence                             | Violation Signal                          |
| ----------------------------- | ----------------------------------- | ----------------------------------------- |
| **Evidence-Based**            | Measure → decide                    | "Best practice" without context           |
| **KISS**                      | Simplest working solution           | Complexity without justification          |
| **YAGNI**                     | Build for today                     | Features for hypothetical future          |
| **SoC**                       | Every unit owns one job; fractal pkg→variable | Unit doing a neighbor's job; "just use layers" |
| **Dependency Inversion**      | Depend on abstractions; point at stability | High-level module depends on low-level detail |
| **Minimal API**               | Expose only necessary               | Leaking implementation details            |
| **Agent UX**                  | Agent is the primary user; every invocation-surface change scored for it, ratcheting forward | Surface change that makes an agent's parse, dispatch, or self-repair worse |
| **Remove-then-Replace**       | Delete old, boundary tests = spec   | Keeping internal tests during rewrite     |
| **File Size**                 | Agent-manageable; split at natural seams | Multi-thousand-line multi-concern monolith |
| **DbC**                       | Own interface → know contract       | Defensive code for own types              |
| **Canonical Representation**  | One internal form, convert at edges | Two representations mixed in the core     |
| **Fail Fast**                 | Errors at source                    | Silent fallbacks masking problems         |
| **DRY**                       | Single source of truth              | Duplicated business logic                 |
| **Documentation**             | Self-documenting code               | Docstrings for internal functions         |
| **File Annotations**          | First line describes responsibility | Files without purpose description         |

---

## References

- [Design by Contract vs Defensive Programming](https://softwareengineering.stackexchange.com/questions/125399/differences-between-design-by-contract-and-defensive-programming)
- [SOLID Principles](https://www.ultracodes.io/blog/principles-of-software-development)
