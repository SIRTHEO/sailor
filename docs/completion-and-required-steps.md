# What completion means, and what a required step withholds

A completed flow has finished according to its graph. That may mean it read
the machine, produced a proposal, or accepted an engine's structured answer.
It does not establish that a code change works. A task needs an executable
acceptance check of its actual result whose failure blocks completion.

A step may declare `required` instead: the run reaches `Complete` only if that
step ran and its check passed, and every other outcome — broken, skipped,
tolerated, or never reached — ends the run naming the step and why. Unlike
`decides_done`, which permits an early success, `required` withholds an
ordinary one. `dispatch-the-work`'s `verdict` declares it; `take-the-next-fault`'s
`warrant` cannot — it has a genuine no-op path, when nothing is open, that a
required step would read as never having run.

## What is not enforced yet

Project rules can be delivered to an agent, but delivery does not enforce
filesystem or network restrictions. Process-boundary enforcement remains
unfinished. A handed step also uses a declared holder name, not an
authenticated identity. These limits matter before entrusting a flow with
unattended work.
