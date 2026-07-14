<!-- Concern: the rubric a fresh-context reviewer scores README.md / README_APPENDIX.md changes against - the voice the docs use, and that every claim, example, and link still matches the shipped code | Non-concern: the review mechanism (the .githooks commit-msg gate owns when it runs and how it is reported), the annotation format (annotation-guide.md), universal code principles (repo-standards.md), or the citations and bibliography (README_APPENDIX.md owns them) | IO: none -->
# Communication Style

The rules for reviewing changes to README.md and README_APPENDIX.md, scored one changed
line at a time.

The first seven are voice, a judgment call per line. The last three are checkable, not
matters of taste: verify them against the code and the headings.

| Rule | Flag a changed line when it... |
| ---- | ------------------------------ |
| **Matter-of-fact** | leans on metaphor, rhythm, or a soft pointer where a plain fact belongs. "the craft lives in the middle one" for a fact you could just state; "X is where it is spelled out" instead of "See X". Declarative, concrete, subject-verb-object. |
| **Don't announce the point** | opens with a setup clause that promises a point and colons into it ("The map is the payoff, and it comes for free: once every file..."). Delete the preamble, lead with the substance. A colon is for a definition or a list, not for clearing your throat. |
| **Scannable** | opens a paragraph that should be a bulleted list, states a key claim with nothing bolded, or leads with context instead of the point. |
| **Reader-first** | leads with what we built or how hard it was instead of a pain the reader has hit, or states a benefit before the problem is felt. Name the problem, then the fix, then the benefit. "What's in it for me" beats "what we did". |
| **Tight** | carries a windup, throat-clearing, or a sentence that explains our reasoning to ourselves rather than moving the reader. Every word earns its place. |
| **No em-dashes** | contains an em-dash. Use commas, periods, colons, or parentheses. |
| **Plain, not hyped** | reaches for hype (superlatives, "blazingly", "seamless", "simply", "just") or stacks hedges ("we believe it might possibly"). Confident and direct. |
| **Claims match the code** | teaches a flag, command, config key, default, or behavior the shipped CLI does not have or that behaves differently now. |
| **Examples lint clean** | shows an example annotation the linter (`--strict-check`) would reject. |
| **Links resolve** | uses a `[...](#anchor)` whose heading does not exist, or a relative path to a file that is not there. |

**Checking the last three.** Spot-check every `--flag`, `.toml` key, install command,
example annotation, and link in the changed lines against `annotated-tree --help`, the
source, and the actual headings. A feature described but not built is worse than one
left out: it sends the reader down a path that is not there.

**Out of scope.** The academic citations and their bibliography live in
README_APPENDIX.md and are reviewed there. Do not re-litigate an existing source. A
*new* claim that cites a source needs only that the source is real and says what the
line claims it says.
