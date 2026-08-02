<!-- Concern: the bar a comment must clear to earn its space | Non-concern: the first-line annotation format (src/annotation-guide.md owns it) or prose voice (communication-style.md) | IO: none -->
COMMENT STANDARD — comments aren't free. Default deny: every comment is guilty until it
proves it carries the non-obvious.

Code shows WHAT; names and types show intent; convention fills the rest. A comment earns its
space only by supplying what none of those can. Everything else is pure cost — space, split
attention, and a stale lie waiting for the next edit.

"NON-OBVIOUS" IS NOT "ABSENT FROM THIS FILE". A competent reader, human or agent, infers
convention and common idiom. Nobody needs telling that a retry loop retries. The test is not
*is it in the code* but *can a competent reader derive it*. If inference reaches it, cut it.

DOC COMMENTS — same bar. One restating the signature is a DRY violation that changes twice on
every edit. Write one only when an external consumer parses it, for a public library
interface, or for a genuinely complex algorithm needing domain or math explanation. Never for
an internal function; refactor the unclear code instead.

  Tempted to add a comment?
    |- Reader derives it from code / names / convention? -> delete it, or fix the name
    |- Non-obvious WHY / invariant / safety rationale?   -> keep it; the one justified case
    |- External consumer or public API parses it?        -> doc comment
    `- Complex algorithm needing domain explanation?     -> doc comment

  +9   Carries the non-obvious WHY, an invariant, or a safety rationale
  -6   Restates what the code, the names, or convention already show
  -8   Doc comment restating the name, signature, or types
  -8   History narration: "was X, now Y", "previously", a migration story
  -10  Stale: describes the code as it WAS, or lies about what it does now

Rot compounds. One stale line taxes every future read, and one exposed lie makes the reader
distrust every other comment in the file.

WHAT THIS GATE CAN AND CANNOT DO
  It measures volume inside a function — a comment ratio and consecutive runs — and nothing
  else. It cannot tell a good comment from a bad one and does not try. Tripping it is NOT an
  instruction to delete comments until the number passes. It is a prompt to ask why this
  function needed a block of explanation; the usual answer is that the code is wrong, not the
  comment. Fix the shape: extract it, rename it, split it.
  Never raise a threshold to make a file pass. The bound is the detector, and a bigger number
  only hides what it found.
