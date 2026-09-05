# Security

## Reporting a vulnerability

**Report it privately, not in a public issue.** Open the repository's
**Security** tab on GitHub and use **Report a vulnerability** — GitHub's private
vulnerability reporting. The report stays between you and the maintainers until
there is a fix.

Useful in a report, in whatever order you have it:

- the version, from `sailor version`, and the commit if you built from source;
- the platform;
- the exact command or flow, and the configuration around it;
- what happened, and what an attacker gets from it;
- the smallest way to reproduce it — **with no real credential in it.** Redact
  tokens, keys and absolute paths from your home before you paste anything.

We will confirm the report and tell you what we find **as soon as we can**. This
is a small project under construction and there is no rota behind it, so that is
the only promise made here: no target, no hours. If a report goes quiet, it is
fine to ask again on the same private thread.

Please give us a chance to ship a fix before writing about it publicly. Credit
goes to the reporter unless you would rather it did not.

## The surface that matters

**Sailor spawns command-line engines as child processes and composes the
environment they start in.** It chooses which engine to call, under which
credential profile — which home directory that engine will read its credentials
from — and with what arguments, environment and working directory. It then
records what happened in a local ledger.

That makes **credential handling the sensitive part of this program.** The
things worth attacking, and worth reporting:

- a credential, token or key leaking out of the profile it belongs to: into
  another engine's environment, into the ledger, into a log line, into an error
  message, into an artefact a flow publishes;
- a profile boundary that does not hold — a call running under an identity other
  than the one the run says it used, or an empty field where the identity should
  be;
- a command line assembled from data (a flow file, a descriptor, a model
  catalogue, an engine's own output) in a way that lets that data choose the
  binary, add arguments or set environment variables it should not;
- a flow file or descriptor from outside the tree that gains powers the
  registry never granted it;
- a child process that survives what was meant to stop it, or a tree of
  processes left running after a stop;
- a path traversal or an overwrite outside the workspace or the store, through a
  flow input, a step's working directory or a release target.

Reports about the desktop window (`desktop/`) and about the terminals it hosts
belong here too.

## Out of scope

- Vulnerabilities in the command-line engines Sailor drives, or in the services
  behind them — report those to whoever ships them. What is in scope here is how
  **Sailor** invokes them and what it hands them.
- Vulnerabilities in third-party crates or npm packages, unless the way this
  project uses them is what makes them exploitable. Report the crate upstream
  and, if you like, open an issue here so the version can move.
- Anything that requires an attacker to already have write access to the tree,
  to the store, or to the account running Sailor. Sailor runs your engines with
  your credentials by design; someone who already is you is not a boundary this
  program can defend.
- Missing hardening with no path to impact, and scanner output with no
  reproduction attached.

## What this project is

Under construction, and used every day by the people writing it. There are no
released versions to support yet: fixes land on the trunk, `sorgenti`, and
whoever runs Sailor from source should track it. Known defects, security ones
included, are written down in `docs/guasti-incontrati.md` — a report that turns
out to be one already there will be pointed at its number.
