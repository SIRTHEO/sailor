# Changelog

This file follows [Keep a Changelog](https://keepachangelog.com/en/1.1.0/) and
starts at the first tagged version. What changed inside a version is read from
where it is written with its real author and date, not copied here by hand:

- **the commits**, in Conventional Commits form, whose bodies carry the why:
  `git log --first-parent`;
- **the fault register**, kept in Sailor's own store (`sailor faults list`), one
  entry per defect with how it surfaced and what would have stopped it; the
  open ones a user can meet are described in `docs/faults-encountered.md`;
- **the decisions that do not reopen**, `docs/decisions.md`.

<!-- ## [VERSION — coordinator names it] - DATE — coordinator names it
     The section below becomes this release's notes when the tag is cut;
     nothing else is invented ahead of that. -->

## [Unreleased]

## [0.1.0] - 2026-09-19

The first tagged version. Everything before it is read from the commits, which
is why this section says what Sailor **is** at 0.1.0 rather than what moved
since a version that never existed.

### Added

- Sailor runs a piece of work as a **flow**: a file of steps whose nodes are
  actions the binary registers, recorded in a ledger as it goes. `sailor flow`
  checks one before it runs, names the actions it is missing, and says what it
  has cost and how often it has run.
- **Engines are data, not code.** A command line is described by a descriptor:
  no provider is named anywhere in the product, and a capability nothing
  declares is written down as absent rather than assumed (ADR-001, ADR-013).
  A forge, a remote and a trunk are declared the same way (ADR-020).
- **Terminals are first class.** `sailor terminal` holds a live shell under a
  name, hands a mandate to the session inside it, and outlives the window that
  opened it. A terminal that has been owed a handover says so.
- **Accounts are read from the engines' own records.** `sailor accounts` opens
  each home once, with the identity beside it, and says what that account
  really did — terminal sessions included — rather than what Sailor believes it
  asked for.
- **Delivery is written down** in `docs/the-delivery-loop.md` and governed by
  `.sailor/delivery-policy.json`, read as the trusted trunk committed it and
  never as the branch under review would have it read.
- **Judges that may only get stricter.** A ratchet measures a property of the
  tree against a seed that can fall and never rise, so a debt cannot grow in
  silence. Seeds are measured on a clean `HEAD`.
- **The trunk is reached only through a merged request**, behind six required
  checks, one of them an attestation for private names that only the owner's
  own machine can post.
- A desktop window, a menu-bar dot and a fault register, each a release target
  of its own.

### Known limits, stated rather than left to be discovered

- The four flows that carry delivery exist and resolve, and have **never run**:
  most of their work is still shell written inside JSON.
- Spend does not reconcile to a single total, and terminal sessions do not
  reach it at all.
- Sailor has no notion of context: it watches a compaction happen and does
  nothing with it.

These are tracked as open issues rather than described as finished.
