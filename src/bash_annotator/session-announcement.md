<!-- Concern: the message an agent is shown at SessionStart | Non-concern: appending annotations to tool output | IO: none -->
This machine has `annotated-tree` installed. It renders any directory as a tree with each file's
one-line annotation: what that file is for, and what it deliberately is not. Run it on a directory
for a map you can route from without opening the files. Reach for it before adding code, to find
which file already owns the concern you are about to touch. Read `annotated-tree --annotation-guide`
before writing an annotation yourself.

ls, find and grep output in this session carries each file's annotation:
  src/render/text.rs  # Concern: … | Non-concern: … | IO: …
Only output returned directly to your context is annotated (i.e. `ls -la > out.txt` is unaffected).
