<!-- Concern: the canonical guide to writing a good first-line annotation | Non-concern: enforcing the format or the argument for annotations at all | IO: none -->
ANNOTATION GUIDE — write a map an agent can route from WITHOUT opening the file.

Every source file's first line states its ONE job, in three ` | `-delimited fields:
  {TEMPLATE}
Example:  {EXAMPLE}

  Concern      the file's ONE job — a verb-led phrase. "utils" or "helpers" is not a job.
  Non-concern  a concern an agent would expect here but this file does NOT own — not its
               own internals. This field IS the point.
  IO           (inputs) -> outputs, OR the literal `none` (config, data, docs).

BREVITY — every field states WHAT, never why, how, or when: no mechanism, rationale, or
conditions in any of them. Naming WHERE an excluded concern lives is OPTIONAL — add the
pointer only when the tree does not already show the owner. An agent ingests a whole
workspace's map in one pass; brevity buys that. The bound is 200 characters over the whole
annotation, comment marker excluded (`--max-length` / `[rules] max_annotation_length`).
Never raise it to fit a line — the bound is the detector, and a bigger number only hides
what it found. A line that will not fit is an ARCHITECTURE defect: write it denser AND
split the file that owns two jobs.

GOOD   // Concern: memoizes lookups | Non-concern: eviction (LRU owns it) | IO: (Key) -> Value
FAILS  // Concern: memoizes lookups | IO: (Key) -> Value
<!-- more -->
FOLDER CHARTERS — a `.annotation` states the DIRECTORY's one job, one altitude above its files.
  Never enumerate or restate what the files inside say. The map already shows them.
  Repeating a file's line means the charter sits too low. Raise it.
  Two file jobs joined by "and" is the tell. Name the one thing they serve.
  The charter is what the files have in common, not their sum.
  A charter that will not fit is restating its files. Raise the altitude first. If it still
  will not fit at the right altitude, the DIRECTORY owns more than one job. Split it.
  An `.annotation` file is ONE line and nothing else. A trailing newline is fine; a note written
  underneath is not — the charter resolves to nothing and the row goes bare. Put it in a README.

HOW TO FIND THE NON-CONCERN
  Ask: what would an agent WRONGLY assume this file does? Negate that. Name where the work
  lives instead only when the owner is not already obvious from the tree.
  A pointer to a file on the next line of the tree is bloat — the map already said it.
  If the exclusion is true of every file, it is a truism, not a boundary. Sharpen it.
  Honesty over tidiness: a truthful line exposing a messy boundary beats a tidy one that hides it.
  Read a folder's annotations together — they should partition the work, no two claiming one job.
  Marker varies by language: # Python/shell/TOML/YAML, // Rust/Go/TS, <!-- --> HTML/Markdown, -- SQL.
  A file with NO comment syntax (CSV, data, binaries) takes a `<name>.annotation` sidecar
  beside it holding the line BARE, with no marker — the same shape a folder's `.annotation`
  holds. One line there too. Writing one is what lists the file at all. A file that can hold a
  comment must.
  YAML frontmatter, like a shebang, keeps line 1: put the annotation on the line after it.
  In a YAML file itself, `---` starts a document, not frontmatter: the annotation goes on line 1, above it.
  An extensionless script is its shebang's language, so a git hook takes a `#` line, not a sidecar.

REWRITING MANY AT ONCE — strip them first: `strip -R -y <dir>`. Shown an existing line, you
  will edit it instead of reading the code, and a sweep returns reworded versions of the old
  lines. Removing them first forces you back to the source. Nothing is written without `-y`,
  and a directory needs `-R`.
