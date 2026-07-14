<!-- Concern: the extended argument for annotated-tree (the infinite-context objection, related work, what is still unproven) plus the bibliography for every inline citation | Non-concern: what the tool is or how to run it (README.md owns that) | IO: none -->
# annotated-tree: the deeper argument

The extended case for [annotated-tree](README.md): read it when the question is "why
should I believe this", not "how do I run it". Works cited inline are listed in full at
the [end](#references).

## Would any of this survive an infinite context window?

The strongest objection is that this solves a temporary problem: context windows grow
every year, so one day an agent will simply hold the whole workspace, and everything
it has ever done to it, in working memory. Take the objection at full strength. Grant
one agent an infinite context window, exclusive write access, and immortality: nothing
in the codebase changes without its hand, and it never forgets. That agent has a
perfect mental model, and no annotation can tell it anything it does not already know.
What is left?

**The whiteboard.** A perfect memory still pays attention selectively. Today's
long-context models certainly do: mid-context content is measurably neglected, and
without literal lexical anchors, retrieval quality collapses well below the advertised
window (Liu et al., 2024; Modarressi et al., 2025). A one-line-per-file map is exactly
what such an agent would sketch for itself to keep its own lookups cheap, which is to
say it would invent annotated-tree internally and consult it constantly. People use
tools for things they could do in their heads, precisely so their heads stay free for
the task at hand. The tool is that data structure, persisted.

**Everyone outside that head.** Push the hypothetical further and infinite context
stops being one head at all. It starts to look like a hive mind: many readers and
writers operating on one shared memory. Granted as well, and inside the hive the map
is redundant. But someone is always outside the hive: the human who has to review the
change, the other vendor's agent, next year's model, which is a newcomer no matter how
large its window is. To all of them, a model held in a mind, however perfect, is
invisible: it cannot be read, reviewed, or contradicted, and binds no one. Intent has
to be public to be a contract, and writing it down is what makes it public.

**The world we actually run.** Sessions are mortal and plural. Every step back from the
hypothetical toward reality, finite windows, session resets, many agents, humans in
review, restores in full the per-session tax the README describes.

Growing context windows solve amnesia. They do not create shared truth, and they do
not onboard newcomers. This argument ships attached to the tool for the same reason an
annotation ships attached to its file: a principle filed apart from the thing it
governs is already rotting.

## Related work

We researched the field before building, and again after the argument above was
written. The tools that hand a codebase to a model fall into three families, and
all three are missing the same layer.

**Serializers** (repomix, code2prompt, gitingest, files-to-prompt, and by now
dozens more) pack file contents into one prompt. Useful, and their newer
compression modes keep signatures and drop bodies to save real tokens. But
compressed or not, they ship what the code says. Nothing in a serialized dump
states what a file is for, or what it is deliberately not.

**Derived maps** (aider's repo-map and its many descendants, symbol indexes,
LLM-generated wikis) had the right instinct as early as 2023: give the agent a
compact map instead of a dump. But every one of them derives the map from the
code, so the map can only restate behavior, never record intent. A derived map
cannot disagree with the code, so it can never warn you either. And no tool in
this family enforces anything: coverage is whatever the generator managed today.
The best published validation of the shape itself is Agentless (Xia et al.,
2025): a tree-style structure view plus per-file skeletons beat far heavier
agents at a fraction of the cost. Structure-shaped context wins. It still
derives rather than records.

**Prose context files** (CLAUDE.md, AGENTS.md, per-editor rules files) are the
field's current answer to intent, read natively by every major agent, and the
academic evaluation of them is openly skeptical (they cost tokens without
generally improving success). That result cuts at us too, and we accept
the burden it sets: context must earn its tokens. One line per file, map-shaped,
read on demand rather than pasted into every prompt, and linted for existence and
form is our answer to exactly that bar. A second study of the same file format
found curated context files cut agent runtime by roughly 29% and output tokens by
17% at equal completion rates (Lulla et al., 2026): efficiency, not success. That
is precisely the register the tool optimizes for.

One proof that the mechanism scales comes from outside the agent world entirely:
REUSE/SPDX license headers put a machine-readable one-liner in every file and
lint it in CI, at the scale of the Linux kernel. Enforced per-file one-liners
work. They have simply never been applied to responsibility instead of licensing.

So `annotated-tree` occupies the cell the field leaves empty: authored intent, on
every file, rendered as one map, enforced so it cannot rot. Freeform cousins
circulate (the `ABOUTME:` two-line header convention seen in CLAUDE.md templates)
and prove the appetite; the structured fields, the Non-concern discipline, and
the lint are what turn a habit into a system. The dependency graph keeps one
distinct claim too: manifests (`pyproject.toml`, `package.json`, `Cargo.toml`,
`go.mod`) cross-referenced into internal, external, and used-by edges in the same
tree. Single-ecosystem and file-level graph tools exist and are good; a
cross-ecosystem manifest graph inside the map the agent already reads exists
nowhere else we could find, and we looked twice.

The lineage is older than agents. In
1985 Parnas gave the A-7E aircraft software a "module guide": a hierarchical
responsibility map built so a maintainer could find the parts that mattered
without reading irrelevant detail about the rest (Parnas et al., 1985). The
annotated tree is a module guide made per-file, machine-checked, and rendered on
demand.

Like the better tools in every family above, it is deterministic, makes no
network or model calls, and installs none of the target project's dependencies.
The difference was never determinism. It is that the map records intent, and the
build refuses to let it rot.

## Future work

Everything above is argued from operating experience and prior literature, not
from controlled measurement. That is an acknowledged gap: none of this is proven
in the benchmark sense, and rigor demands it be. The measurement we most want to
see is easy to state and expensive to run: the same task suite, the same agent,
the same workspace, with and without the annotated map, scored on task success,
tokens spent, and how much of the output survives review (first-pass yield,
measured for real). The AGENTS.md evaluation by Gloaguen et al. (2026)
supplies a ready methodology template; what it needs is the per-file, enforced
variant tested alongside the prose one.

We have not done this. What we have is operating evidence: one to three hours of
human steering a day buys roughly twenty-one autonomous hours, and the same agent line
wrote a six-figure line count of production Rust, kept architecturally coherent, in a
language its operator did not start out knowing. That is conviction, not a controlled
experiment. If you have a benchmark harness, an interest in agent-context economics,
and access to cheap tokens, this is a paper waiting to be written, and we would
genuinely like to read it. Open an issue.

This document reads like a research paper and is not one: it is a living document
attached to a living tool, and both take pull requests. If an argument is wrong, an
analogy leaks, or the tool should do something it does not, open an issue or send the
fix.

## References

Full citations for the works cited inline in [README.md](README.md) and above.

1. Kirsh, D. (1995). The intelligent use of space. *Artificial Intelligence, 73*(1–2), 31–68. https://doi.org/10.1016/0004-3702(94)00017-U
2. Yang, J., Jimenez, C. E., Wettig, A., Lieret, K., Yao, S., Narasimhan, K., & Press, O. (2024). SWE-agent: Agent-computer interfaces enable automated software engineering. *NeurIPS 2024*. https://arxiv.org/abs/2405.15793
3. Xia, X., Bao, L., Lo, D., Xing, Z., Hassan, A. E., & Li, S. (2018). Measuring program comprehension: A large-scale field study with professionals. *IEEE Transactions on Software Engineering, 44*, 951–976. https://doi.org/10.1109/TSE.2017.2734091
4. Bainbridge, L. (1983). Ironies of automation. *Automatica, 19*(6), 775–779. https://doi.org/10.1016/0005-1098(83)90046-8
5. Endsley, M. R. (2023). Ironies of artificial intelligence. *Ergonomics, 66*(11), 1656–1668. https://doi.org/10.1080/00140139.2023.2243404
6. Gloaguen, T., et al. (2026). Evaluating AGENTS.md: Are repository-level context files helpful for coding agents? arXiv:2602.11988 (preprint). https://arxiv.org/abs/2602.11988
7. Dijkstra, E. W. (1974). On the role of scientific thought. EWD447; reprinted in *Selected Writings on Computing: A Personal Perspective* (Springer, 1982). https://www.cs.utexas.edu/~EWD/transcriptions/EWD04xx/EWD447.html
8. Lethbridge, T. C., Singer, J., & Forward, A. (2003). How software engineers use documentation: The state of the practice. *IEEE Software, 20*(6), 35–39. https://doi.org/10.1109/MS.2003.1241364
9. Tan, L., Yuan, D., Krishna, G., & Zhou, Y. (2007). /\*iComment: Bugs or bad comments?\*/ *SOSP '07*. https://doi.org/10.1145/1294261.1294276
10. Macke, W., & Doyle, M. (2024). Testing the effect of code documentation on large language model code understanding. *Findings of NAACL 2024*. https://aclanthology.org/2024.findings-naacl.66/
11. LaToza, T. D., Venolia, G., & DeLine, R. (2006). Maintaining mental models: A study of developer work habits. *ICSE 2006*, 492–501. https://doi.org/10.1145/1134285.1134355
12. Liu, N. F., et al. (2024). Lost in the middle: How language models use long contexts. *Transactions of the ACL, 12*. https://doi.org/10.1162/tacl_a_00638
13. Modarressi, A., et al. (2025). NoLiMa: Long-context evaluation beyond literal matching. *ICML 2025*. https://arxiv.org/abs/2502.05167
14. Xia, C. S., Deng, Y., Dunn, S., & Zhang, L. (2025). Agentless: Demystifying LLM-based software engineering agents. *Proceedings of the ACM on Software Engineering, 2*(FSE). https://doi.org/10.1145/3715754
15. Lulla, N., et al. (2026). On the impact of AGENTS.md files on the efficiency of AI coding agents. *JAWs @ ICSE 2026*. arXiv:2601.20404 (preprint). https://arxiv.org/abs/2601.20404
16. Parnas, D. L., Clements, P. C., & Weiss, D. M. (1985). The modular structure of complex systems. *IEEE Transactions on Software Engineering, SE-11*(3), 259–266. https://doi.org/10.1109/TSE.1985.232209
