<!-- Concern: the canonical guide to writing a good first-line annotation, embedded and rendered into --help and a failing --strict-check | Non-concern: enforcing the format (src/strict.rs owns the grader) or why annotations exist at all (README.md owns the argument) | IO: none -->
ANNOTATION GUIDE — write a map an agent can route from WITHOUT opening the file.

Every source file's first line states its ONE job, in three ` | `-delimited fields:
  {TEMPLATE}
Example:  {EXAMPLE}

  Concern      the file's ONE job — a verb-led phrase. Filler ("utils", "helpers") FAILS.
  Non-concern  a concern an agent would expect here but this file does not own, and where
               it lives instead: a NAMED sibling, an external system, or out of the repo's
               scope. "nothing"/"n/a"/the file's own internals FAIL (annotation_vacuous).
               This field IS the point.
  IO           (inputs) -> outputs, OR the blessed literal `none` (config, data, docs).

GOOD   // Concern: memoizes lookups | Non-concern: eviction (LRU owns it) | IO: (Key) -> Value
FAILS  // Concern: memoizes lookups | Non-concern: nothing | IO: (Key) -> Value
FAILS  // Concern: memoizes lookups | Non-concern: <Y> | IO: (<inputs>) -> <outputs>
<!-- more -->
HOW TO FIND THE NON-CONCERN
  Ask: what would an agent WRONGLY assume this file does? Negate that, and say where the
  work lives instead: a sibling, an external system, or out of this repo's scope.
  If the exclusion is true of every file, it is a truism, not a boundary. Sharpen it.
  Honesty over tidiness: a truthful line exposing a messy boundary beats a tidy one that hides it.
  Read a folder's annotations together — they should partition the work, no two claiming one job.
  Marker varies by language: # Python/shell, // Rust/Go/TS, <!-- --> HTML/Markdown, -- SQL.
