<!-- Concern: the invariants annotated-tree must always satisfy, and the vocabulary they quantify over | Non-concern: how any is implemented, why one was adopted, or how to author an annotation | IO: none -->
# SPEC — annotated-tree

The register of decisions annotated-tree is willing to freeze: a vocabulary of product
entities and the invariants over them. How any of them is enforced — the gates, the hooks,
the tests — lives with the enforcement, not here.

## Vocabulary

- **run** — one execution of the tool against one Workspace, from the arguments it is given
  to the output it completes.
- **Workspace** — the set of directory trees a run is pointed at, and everything beneath
  them.
- **Annotation** — a contract a file or directory states about itself: what it is for, what
  it deliberately is not, and its inputs and outputs.
- **Report** — everything a run emits about a Workspace on any channel a caller can observe,
  its exit status included.
- **accessory tool** — a program shipped alongside that helps an agent consume Annotations, such
  as a wrapper that adds them to another program's output. It performs no run and emits no Report,
  so nothing below governs it.

## TREE

**TREE1** — Every Annotation a Report shows is the whole, unaltered text of that Annotation,
and a Report shows no other text in its place.

**TREE2** — Every file and directory of a Workspace either appears in a Report or falls under
an exclusion criterion the Report states and a reader can apply to any path.

## CHECK

**CHECK1** — Every issue a Report raises about a file's or directory's Annotation is that one of
its parts is absent, is empty after trimming whitespace, or is longer than a bound its caller
gives.

## CORE

**CORE1** — Every Report a run produces is determined by that run's arguments, the bytes at the
configuration paths it is given, and the bytes at the paths within its Workspace, and by
nothing else.

**CORE2** — Every Annotation's location is determined by the path of the file or directory it
annotates, and no Annotation appears anywhere else.

**CORE3** — Every Annotation is recoverable by reading a fixed position in the artifact that
carries it.

**CORE4** — Every run creates, changes, or removes no artifact anywhere other than its Report.
