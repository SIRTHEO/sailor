# The delivery loop

One flow, ten steps, from taking work to closing it. The same steps whether a
person runs the next one from a terminal or Sailor runs it as a recorded
flow: the only permitted difference is who requests the next transition,
never what is checked, what evidence it leaves, or what counts as complete.

## What this loop exists to prevent

1. Work claimed without reading the policy that governs it.
2. A worktree nobody can trace back to a run.
3. A change merged without a counterexample proving the claimed property.
4. A mandate misread, found out only after delivery.
5. A reviewer who shares the author's blind spot.
6. A review that approved a tree different from the one that shipped.
7. An integration that validated something other than the real candidate.
8. A publication step skipped because the branch already "looked" merged.
9. A release installed without exercising the binary it replaces.
10. A ref moved between two steps that assumed it would not.
11. A run abandoned mid-way with no visible record of what is left open.

## The ten steps

For each: who runs it today, who runs it once it is a flow step, the
mechanical check and where it lives, the evidence it leaves, and which of the
eleven mistakes above it closes.

| step | today | as a flow | check | lives in | evidence | closes |
|---|---|---|---|---|---|---|
| 1. Resolve authority and take work | worker claims, coordinator initiates | required policy-resolution, then atomic claim | resolve the trusted policy first; store the mandate as an immutable file; acknowledge its digest and the acceptance conditions before working | policy file below; a future claim/acknowledgment step | claim row, mandate, acknowledgment | 4, 9; owns 11 |
| 2. Create and hold the working tree | coordinator invokes the worktree action | required tree-allocation action | start from the recorded trunk commit; register worktree, branch, owning process and run; pin checkpoint commits under refs the run owns; compare expected against actual refs each step | holdings ledger and a process-boundary ownership check — a git hook cannot reliably stop a `reset` or a stray ref move | holdings row, base commit, checkpoint refs, mutations | 2; supports 11 |
| 3. Implement and prove the change | worker implements, gates check | worker step, then required checks | one versioned gate resolver — letter A plus every applicable letter; a failing counterexample before the passing correction; the real command line where the claim concerns it; commits by explicit path | the gate manifest, [`gates.md`](gates.md), read by the same resolver everywhere | commit, gate results, counterexample and CLI transcript | 1, 3, 7, 8; helps 5, 6 |
| 4. Publish the checkpoint | coordinator invokes publication | required policy-controlled step | after each commit round: preflight passes, remote branch resolves to the intended commit, a draft pull request exists; missing approval is "pending", never reported delivered | release preflight, policy file's `push` setting | remote commit, pull-request id, receipt or pending obligation | 3, 8, 9, 11 |
| 5. Review an immutable candidate | independent reviewer, this tree's worktree convention | required reviewer step in a separate checkout of the pinned commit | mandate, diff, affected callers, written contracts, proof strength, defaults checked explicitly; reviewer works from artifacts alone, no author conversation; verdict binds to commit, mandate, base, gate digest | the fresh-context review convention already run here; not yet a recorded digest-bound verdict | verdict naming findings, checked contracts, uncertainty | 5, 6, 7; supports 2 — lowers risk, proves nothing absent |
| 6. Resolve findings or park | worker fixes, reviewer rechecks, coordinator parks the rest | bounded fix/review cycle with a required verdict check | every fix republishes a checkpoint and reruns affected evidence; after two failed rounds, or unresolved scope, hand off with a concrete question | the parked-row convention in this tree's coordination window | link from finding to fixing commit, replacement verdict, or a parked row | 4, 5, 6; prevents silent 11 |
| 7. Validate and authorize integration | coordinator prepares, gates validate, a person approves where policy asks | required build, gate, policy-check steps | build the combined tree against current trunk first; run the full battery `--no-fail-fast` plus applicability checks and preflight there; measure ratchets against the trusted baseline, only the allowed direction; if trunk moved underneath it, rebuild and revalidate | `sailor ratchet`, measured on a clean `HEAD` never a shared tree, and letter F | candidate commit, full results, ratchet measurements, approval, ref-update receipt | 1, 5, 6, 8 |
| 8. Publish integration, reconcile the pull request | the forge merges the pull request; the coordinator asks and reconciles | required policy-controlled steps | nothing is pushed to the trunk: the forge makes the merge commit, and only once every check it requires is green. The candidate's own id never lands, so what is compared is the tree — the trunk must carry the reviewed commit, carry the tree the gates ran on, and the host must report the pull request merged, not merely pushed; an API failure is a retryable obligation | the trunk's ruleset and its required checks, the private-names check this machine posts, the `sailor/integrated` status only this flow posts, policy file's `merge`/`push` settings, the account a tree calls the forge as | merge commit, landed tree, observed pull-request state | 9, 11 |
| 9. Accept and release the candidate | machine builds, a person runs the journey, coordinator releases | required build, journey-handoff, policy-controlled steps | build from the validated commit; run the release battery isolated and repeat preflight; install into an isolated location and run the versioned journey against that binary, never the old one; bind the signed result to its digest, verify the active version after install | `sailor release`, policy file's `release` setting — mandatory either way, since it governs who authorizes, not whether the journey runs | source/binary digests, journey result, release note, installation receipt | 7, 8, 9 |
| 10. Close and release holdings | coordinator initiates reconciliation | required closure actions | update the issue/queue row with proof links; reconcile the fault register; confirm remote publication before deleting a branch; remove worktrees only after readers finish; retry each external operation idempotently — complete only when every obligation is satisfied | holdings ledger, fault register, this tree's branch lifecycle (born from `main`, proved superseded, deleted together) | closure receipts, register revision, pull-request state, released holdings, final ledger row | 10, 11 |

## What is implemented today, and what is not

**Implemented:** the mandate as a claim a worker reads; an isolated worktree
per run; independent review in a fresh context; the gate manifest as the one
set of letters worker, reviewer and integration all run
([`gates.md`](gates.md)); `sailor ratchet` and the release preflight as
judges over a proposed tree; the parked-row convention; a mandate's digest,
computed before the worker is asked and written into the task's own record,
checked against the first line of the worker's answer and parking the task
without ever running the acceptance command when it does not match
(`crates/sailor/tests/take_the_next_work.rs`:
`a_worker_that_acknowledges_the_mandate_finishes_and_its_record_carries_the_digest`,
`a_worker_that_never_acknowledges_the_mandate_is_parked_before_acceptance_ever_runs`);
the policy file, read through one mechanical resolver — `sailor policy` — bound
to the trunk's own commit rather than the working tree or the branch under
review (`crates/sailor/tests/the_delivery_policy_is_read_from_the_trusted_trunk.rs`).

**Shipped as flows:** steps 4, 5, 7 and 8, 10 and 9 as `open-the-draft-pull-request`,
`review-a-pinned-commit`, `integrate-on-the-trunk`, `close-the-work` and
`cut-a-release` in `crates/flow/system/`. Each forge call acts as the account
in `sailor.forgeAs` and refuses without one; the publication gate
(`scripts/privacy-scan.sh`, on a ref or on `--text <file>`) runs before every
push, pull request, integration, tag and release; the delivery policy is read
from the file the trunk commits before each gesture it governs, and `ask`
hands that gesture to a person. The review pins the head under `refs/sailor/reviews/` and hands the
reviewer a locked checkout of exactly that commit; the integration refuses any
other head, runs `scripts/run-gates.sh --no-fail-fast` on the combined tree,
and completes only when the host reports the pull request merged. **Neither
flow pushes to the trunk**, because the trunk refuses it: the checkpoint is
pushed to its own branch with a lease on the ref it has just read, the
private-names check is posted from here because only this machine holds the
list, and the integration waits for the checks and then asks the forge to
merge. Measured on the host on 21/09/2026: `open-the-draft-pull-request` opens
real requests, and `review-a-pinned-commit` has completed once; no run of
`integrate-on-the-trunk` or `cut-a-release` has reached its end yet, the first
stopping where no review of the branch was recorded and the second where the
sailor in service did not match its recorded sha256. They bootstrap in order: the integration runs
the trunk's `scripts/run-gates.sh`, and the gate refuses a privacy script that
differs from the trunk's, so the resolver and the privacy script with `--text`
reach the trunk by hand before these flows can carry anything.

**What these flows defend against, and what they do not.** They defend against
a different `sailor` found on PATH, since every `sailor` they call is the copy
in service, checked against its recorded sha256 right before it runs. They
also defend against an installed binary that does not match the candidate; a
delivery policy that is missing, is not readable as JSON, or whose `merge`,
`push` and `release` fields are not `auto` or `ask`; and a remote that cannot
be read or has moved. They do not defend against whoever can already write to the directory
of the `sailor` in service: that person can replace the binary between its
sha256 check and its execution. The policy reader inside the flows is also
more permissive than `sailor policy` on malformed input, such as out-of-range
numbers or NUL bytes, in fields nobody reads. And whoever closes a handed step
(`authorize_*`, `manual_gates`, `journey`) does it with the `sailor` on their
own PATH. Only the review brief names the verified `sailor`, and even there
nothing stops a reviewer from using another.

**Not yet implemented:** checkpoint commits pinned under run-owned refs with a
process-boundary ownership check; an acceptance journey bound to the freshly
built candidate's own digest rather than to its commit; a review verdict bound
to a recorded gate result rather than to the manifest's digest; ratchets
measured by the integration flow itself; a candidate installed into an
isolated location before the journey — `cut-a-release` installs it into the
live home once the release is authorized; the bullets of `gates.md` that name
no command, which the resolver lists for the reviewer rather than runs.

No dates are attached to the above: this section says what is true now, not
when the rest lands.

## What a hook cannot do, and what does

- **A local git hook cannot reliably stop a `reset` or an arbitrary ref
  move.** Protect at the process boundary instead: pin reviewed commits under
  refs the run owns, and detect unexpected movement before every transition.
- **Pasted gate output is not evidence.** A check records its command, exit
  status, tested tree, and the gate definition's digest, so every branch and
  the integration resolve gates through the same versioned resolver.
- **A different engine is not independence.** Reviewer independence comes
  from a fresh context and no authorship, not a second tool.
- **Policy is read before work is claimed, not first at integration.** A
  proposed change must never authorize itself.
- **A durable graph does not make execution survive a closed terminal.** That
  needs supervision and restart reconciliation, not a record of intent.
- **An acceptance journey has to exercise the candidate binary.** Testing the
  version already installed proves nothing about the one replacing it.

## The policy file

Stored at [`.sailor/delivery-policy.json`](../.sailor/delivery-policy.json):

```json
{
  "schema_version": 1,
  "merge": "auto",
  "push": "auto",
  "release": "auto"
}
```

Each of `merge`, `push` and `release` is `auto` or `ask`. In this repository
all three are `auto`: a green combined trunk merges, publishes and releases
without a person's explicit yes, and the gates above are what "green" means —
a setting decides who authorizes a transition, never whether it still has to
pass. The acceptance journey in step 9 stays mandatory either way.

Only the owner may change these values; an agent may propose a change but
never make one. The policy is resolved from an owner-approved revision pinned
in this repository's own trusted local configuration, never from the branch
under review — a change authorizing its own delivery is exactly what this
file rules out. The trusted revision is re-read before every mutation it
governs, so changing it invalidates any approval already in flight.
