# How credentials reach the engines

Written on 01/09/2026 by reading the code, not from memory. Everything drawn
here was checked in the source or by running it; where a thing is **not** there,
the drawing says so instead of leaving it implied.

The question it is born from: *how do you handle the parallelism of several
command lines with different credentials?*

---

## 1. Who launches whom

A flow speaks to no provider. It starts **child processes**, one per call, and
each of them speaks on its own behalf.

```mermaid
flowchart TD
    F["sailor: ONE process"]
    F --> T1["thread of step A"]
    F --> T2["thread of step B"]
    F --> T3["thread of step C"]
    T1 --> P1["child process: codex<br/>CODEX_HOME = home X"]
    T2 --> P2["child process: claude<br/>CLAUDE_CONFIG_DIR = home Y"]
    T3 --> P3["child process: gemini<br/>GEMINI_CLI_HOME = home Z"]
    P1 --> A1(["codex's provider"])
    P2 --> A2(["Claude's provider"])
    P3 --> A3(["Gemini's provider"])
```

**Parallel steps are threads inside one single process** (`std::thread::scope`
in `crates/flow/src/executor.rs`). This is why the following point is not a
detail but the thing holding all the rest up.

---

## 2. The rule that makes parallelism safe

There are two ways of giving a program a different home of credentials. Sailor
uses the second, and the difference is all here.

```mermaid
flowchart LR
    subgraph WRONG["How it is NOT done"]
        direction TB
        S1["change the variable<br/>of the parent process"] --> S2["launch the child"]
        S2 --> S3["put the variable back<br/>the way it was"]
        S3 -.->|"two threads in here<br/>steal each other's identity"| S1
    end
    subgraph RIGHT["How it is done"]
        direction TB
        G1["compose a map<br/>for THIS call"] --> G2["pass it to the child<br/>and to it alone"]
        G2 --> G3["the parent was<br/>never changed"]
    end
```

Checked: in the production code of the whole workspace **there is no call that
changes the process environment** — the only ones are inside the tests. The
environment reaches the child with `cmd.env(key, value)`, that is, laid over the
inherited one, for that child alone.

**Consequence: different command lines, in parallel, with different credentials,
work.** Not through the care of whoever writes the steps: by construction.

---

## 3. How the environment of a call is composed

Three layers. Whoever sits higher wins.

```mermaid
flowchart TD
    E1["1. the environment of whoever opened the terminal<br/><i>inherited</i>"]
    E2["2. the home of the profile active for THAT command line<br/><i>profiles::build_environment</i>"]
    E3["3. the variables written inside the step<br/><i>spec.env</i>"]
    E1 --> E2 --> E3 --> OUT["the environment of the child process"]
```

The direction is a decision, not an accident: whoever writes a variable **inside
a step** is saying something precise about *that* call, and must not be
overridable by a state that lives elsewhere and that the step does not name.

The link between a command line and its variable goes through the
**executable**, not through the name the catalogue gives it: what reads
`CLAUDE_CONFIG_DIR` is the `claude` binary, whatever whoever names it calls it.
And the recognition is by exact name, never by prefix: a `claude-wrapper` does
not get `claude`'s home.

---

## 4. What is shared and what is not — the point of the question

```mermaid
flowchart TD
    ST[("~/.claude/state/profili.json<br/><b>active: codex → «prove»</b>")]
    ST --> C1["call to codex<br/>from step A"]
    ST --> C2["call to codex<br/>from step B"]
    ST --> C3["the person meanwhile<br/>working in the terminal"]
    C1 --> R1["home «prove»"]
    C2 --> R2["home «prove»"]
    C3 --> R3["home «prove»"]
    P["a step writing<br/>CODEX_HOME in its spec.env"] -->|"overrides, but<br/>overrides the mechanism"| R4["any home at all"]
```

**The identity is chosen per command line, not per run and not per step.**
`active: codex → prove` is **one single switch** for the whole lot:

- two parallel steps that both want codex take the **same** identity;
- the state is **read again at every call** — on purpose, so that a change takes
  effect at once — so if somebody changes it while a run is going, two calls of
  the *same run* end up on two identities. It now stays **written down** which
  one each of them used (the store records the resolved profile), so it can be
  seen afterwards: it is not prevented;
- a person working in the terminal shares that switch with the flow.

The only way, today, of having two different identities in parallel on the same
command line is to write the variable by hand inside the step — that is, to
**override the profiles instead of using them**.

---

## 5. The two questions asked before launching

There are two, and they see different things. Confusing them has already cost a
false green.

```mermaid
flowchart TD
    subgraph SCREENING["the dry screening — «is the line assembled right?»"]
        direction TB
        V1["assemble the line from the descriptor"] --> V2["run it TAKING AWAY the question"]
        V2 --> V3["the engine stops on<br/>«you gave me nothing to do»"]
        V3 --> V4["verdict on the LINE"]
    end
    subgraph ACCESS["the access state — «is this home authenticated?»"]
        direction TB
        L1["ask the engine, in the named home"] --> L2["codex login status<br/>claude auth status"]
        L2 --> L3["verdict on the HOME"]
    end
    V3 -.->|"everything the engine checks<br/>AFTER the question stays invisible:<br/>the credentials are over there"| ACCESS
```

The dry screening takes the question away **on purpose**: that is how it tries a
real line without spending anything. But for that same reason it can see nothing
of what comes after. Measured: with an empty home and with the real one,
`codex exec < /dev/null` gives the **same** answer — and for an hour
`flow check` said «line healthy» about a home with no credentials.

The second question was born on 01/09 for this. The words of the yes and of the
no **are declared by the engine's descriptor**, not by the code, as already
happens for the other answers an engine can give. Whoever does not declare them
gets «nobody looked», never «it is authenticated»: `gemini` and `agy` are in
this state, with a note of where the looking happened and what was not found.

A trap the drawing does not show and the code does: **«Not logged in» contains
«logged in»**. The no is read before the yes, and the yes is declared with more
than one word.

---

## 6. How many start together

```mermaid
flowchart LR
    W["width of the front"] --> M["how much is left of the spending ceiling<br/>divided by the dearest call seen"]
    M --> N["from 1 to the ceiling"]
    I["how many calls are<br/>already going on THIS identity"] -.->|"nobody counts it"| W
```

The width narrows when the money goes down, all the way to one. **It does not
narrow when it is the identity that is under strain**: five steps side by side
on the same profile are five calls against the same hourly limit, and that count
does not exist.

---

## 7. What is there and what is missing

| | state |
|---|---|
| different command lines in parallel, different homes | **there**, by construction |
| the parent never changes identity | **there**, checked across the whole workspace |
| the step overrides the profile | **there** |
| knowing, afterwards, which profile a call used | **there** since 01/09 |
| knowing, **beforehand**, whether a home is authenticated | **there** since 01/09, for the engines that declare it |
| **«this run uses profile X»** | **missing** |
| **«this step uses profile X»** — naming a profile, not a variable | **missing** |
| counting the calls in flight per identity | **missing** |
| reading the quota **of the profile** instead of the person's | **missing** — `sailor remaining` reads the person's home |

The first two absences are the same thing, and it is the **third level**: the
research of 29/08/2026 (the note `profili-e-consumo`) had already measured that
AWS, Google Cloud and Kubernetes all three have the same shape — a permanent
file, an environment variable, and **a per-command override** — and that Sailor
has the first two. The third has a side effect that is worth it on its own: **a
run goes back to being a measurement**, instead of the sum of whatever happened
to be active from instant to instant.
