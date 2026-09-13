# Changelog

There are no released versions yet. Sailor is under construction: fixes land
on the trunk, `main`, and whoever runs it from source tracks that branch.

Until the first tagged version, what changed is read from where it is written
with its real author and date, not copied here by hand:

- **the commits**, in Conventional Commits form, whose bodies carry the why:
  `git log --first-parent main`;
- **the fault register**, `docs/faults-encountered.md`, one entry per defect
  with how it surfaced and what would have stopped it;
- **the decisions that do not reopen**, `docs/decisions.md`.

The first entry below is written when the first version is tagged, and from
then on this file follows [Keep a Changelog](https://keepachangelog.com/en/1.1.0/).
`git tag` names none yet: the statement above is accurate as of this commit,
not aspirational.

<!-- ## [VERSION — coordinator names it] - DATE — coordinator names it
     The section below becomes this release's notes when the tag is cut;
     nothing else is invented ahead of that. -->

## [Unreleased]

- The queue flow, `take-the-next-work`, ships with the product's binary.
- Engine roles resolved from the store are recorded on the CLI path, in the ledger's own row.
- The login probe clears a home's environment but keeps `USER`.
- A home's identity is read off its own file and turned into one verdict every surface acts on.
- The desktop window shows account readings and handed-step state in the strip and the attention queue.
- The walkthrough `docs/walkthrough-a-queue-of-work.md` takes a queue of work from a seeded fixture to a parked task, in five minutes.
