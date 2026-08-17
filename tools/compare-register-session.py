#!/usr/bin/env python3
"""Confronta il gancio che registra la sessione viva col suo porto in Rust.

L'ORACOLO È IL PYTHON. Questo gancio non concede e non blocca: ciò che può
divergere è **cosa registra** e **cosa cancella**. Si confrontano byte per byte
l'uscita, stdout, stderr, il record `state/sessioni-vive/<sess>.json` e quali
marcatori sopravvivono in `state/`.

PERCHÉ NON RIUSA UN COMPARATORE ESISTENTE. I diciannove `compare-*.py` non hanno
una base comune, e non per distrazione: ognuno confronta un oggetto diverso —
l'albero dei file per `spotlight-marker`, il registro dei lanci per
`link-worktree-rules`, una `TMPDIR` separata per `handoff-threshold`, la riga di
`ganci.jsonl` per `scope-drift`. Quello che si ripete è lo scheletro (due
cartelle usa-e-getta, due esecuzioni, un diff), non la parte che decide se il
porto è giusto. Qui l'oggetto sono i file di stato che il gancio scrive e
cancella.

DUE HOME FINTE, UNA PER PARTE. Il gancio scrive e cancella: con una cartella
sola la seconda implementazione troverebbe il segnale di ripresa già consumato
dalla prima e tacerebbe — verde per il motivo sbagliato.

`updated_at` SI NORMALIZZA. È l'orologio: fra le due esecuzioni passa qualche
millisecondo. Confrontarlo alla lettera darebbe rosso ovunque, quindi diventa
`<TIME>` — ma un controllo a parte pretende che il campo ci sia, altrimenti un
porto che non lo scrivesse passerebbe grazie alla normalizzazione.

L'AMBIENTE È PARTE DEL CASO. Manico, tab e worktree arrivano da `ORCA_*`, non
dal payload: un confronto che non li varia non tocca nessuna delle tre porte
d'ingresso del gancio.

    python3 tools/compare-register-session.py
"""
import json
import os
import re
import subprocess
import sys
import tempfile
import time
from pathlib import Path

CONFIG = Path("/home/someone/.claude")
ORIGINAL = CONFIG / "skills" / "hooks" / "register-session.py"
BINARY = CONFIG / "rust" / "target" / "release" / "claude-hooks"

SESSION = "31a570a5-1111-2222-3333-444455556666"


def prepare(home: Path, case: dict) -> None:
    state = home / ".claude" / "state"
    (state / "sessioni-vive").mkdir(parents=True, exist_ok=True)
    (state / "riprendi-da").mkdir(parents=True, exist_ok=True)
    for name in case.get("markers", []):
        (state / name).write_text("x")
    signal = case.get("signal")
    if signal is not None:
        key = re.sub(r"[/\\]", "_", case["env"].get("ORCA_WORKTREE_ID", ""))
        p = state / "riprendi-da" / f"{key}.txt"
        p.write_text(signal["body"])
        if signal.get("age"):
            old = time.time() - signal["age"]
            os.utime(p, (old, old))


def snapshot(home: Path) -> dict:
    state = home / ".claude" / "state"
    live = state / "sessioni-vive"
    out = {}
    for p in sorted(live.glob("*.json")):
        body = re.sub(r'"updated_at": \d+', '"updated_at": <TIME>', p.read_text())
        out[f"live/{p.name}"] = body
    out["markers"] = sorted(
        p.name for p in state.iterdir() if p.is_file() and not p.name.startswith(".")
    )
    out["resume"] = sorted(p.name for p in (state / "riprendi-da").glob("*.txt"))
    return out


def run_side(side: str, case: dict) -> dict:
    with tempfile.TemporaryDirectory() as tmp:
        home = Path(tmp)
        prepare(home, case)
        env = {
            **os.environ,
            "HOME": str(home),
            "ORCA_TERMINAL_HANDLE": "",
            "ORCA_WORKTREE_ID": "",
            "ORCA_TAB_ID": "",
            **case.get("env", {}),
        }
        argv = case.get("argv", [])
        cmd = (
            [sys.executable, str(ORIGINAL), *argv]
            if side == "python"
            else [str(BINARY), "register-session", *argv]
        )
        p = subprocess.run(
            cmd, input=case["stdin"], capture_output=True, text=True, env=env, timeout=60
        )
        return {
            "exit": p.returncode,
            "stdout": p.stdout,
            "stderr": p.stderr,
            **snapshot(home),
        }


PAYLOAD = json.dumps(
    {
        "session_id": SESSION,
        "transcript_path": "/tmp/t.jsonl",
        "cwd": "/home/someone/orca/general",
        "source": "startup",
    }
)

CASES = [
    ("fuori da Orca: niente da registrare", {"stdin": PAYLOAD, "env": {}}),
    (
        "solo la tab, senza manico: si registra lo stesso",
        {
            "stdin": PAYLOAD,
            "env": {"ORCA_WORKTREE_ID": "w::/home/someone/orca/general", "ORCA_TAB_ID": "tab-1"},
        },
    ),
    (
        "solo il manico, senza tab: si registra",
        {
            "stdin": PAYLOAD,
            "env": {
                "ORCA_WORKTREE_ID": "w::/home/someone/orca/general",
                "ORCA_TERMINAL_HANDLE": "term_1",
            },
        },
    ),
    (
        "senza worktree non si registra, anche col manico",
        {"stdin": PAYLOAD, "env": {"ORCA_TERMINAL_HANDLE": "term_1"}},
    ),
    (
        "senza identificativo di sessione non si registra",
        {
            "stdin": json.dumps({"cwd": "/x"}),
            "env": {"ORCA_WORKTREE_ID": "w::/x", "ORCA_TAB_ID": "t"},
        },
    ),
    (
        "stdin che non è JSON: si esce muti",
        {"stdin": "non sono json", "env": {"ORCA_WORKTREE_ID": "w::/x", "ORCA_TAB_ID": "t"}},
    ),
    (
        "senza cwd nel payload si usa quella corrente",
        {
            "stdin": json.dumps({"session_id": SESSION}),
            "env": {"ORCA_WORKTREE_ID": "w::/x", "ORCA_TAB_ID": "t"},
        },
    ),
    (
        "un segnale di ripresa fresco parla e si consuma",
        {
            "stdin": PAYLOAD,
            "env": {"ORCA_WORKTREE_ID": "w::/home/someone/orca/general", "ORCA_TAB_ID": "t"},
            "signal": {"body": "/home/someone/.claude/projects/x/memory/consegna.md\n"},
        },
    ),
    (
        "un segnale stantio si butta senza parlare",
        {
            "stdin": PAYLOAD,
            "env": {"ORCA_WORKTREE_ID": "w::/home/someone/orca/general", "ORCA_TAB_ID": "t"},
            "signal": {"body": "/x/consegna.md\n", "age": 4000},
        },
    ),
    (
        "un segnale vuoto non produce mandato",
        {
            "stdin": PAYLOAD,
            "env": {"ORCA_WORKTREE_ID": "w::/home/someone/orca/general", "ORCA_TAB_ID": "t"},
            "signal": {"body": "   \n"},
        },
    ),
    (
        "il worktree con le barre diventa una chiave di file",
        {
            "stdin": PAYLOAD,
            "env": {
                "ORCA_WORKTREE_ID": "9591c8dd::/home/someone/orca/workspaces/suite/tautog",
                "ORCA_TAB_ID": "t",
            },
            "signal": {"body": "/x/y.md\n"},
        },
    ),
    (
        "--fine porta via record e marcatori della sessione",
        {
            "stdin": PAYLOAD,
            "argv": ["--fine"],
            "env": {"ORCA_WORKTREE_ID": "w::/x", "ORCA_TAB_ID": "t"},
            "markers": [
                "consegna-fatta-31a570a5",
                "consegna-misura-31a570a5",
                "consegna-fatta-ripartenze-31a570a5",
                f"successore-di-{SESSION}",
                "consegna-fatta-altra111",
            ],
        },
    ),
    (
        "--fine non tocca i marcatori di un'altra sessione",
        {
            "stdin": PAYLOAD,
            "argv": ["--fine"],
            "env": {"ORCA_WORKTREE_ID": "w::/x", "ORCA_TAB_ID": "t"},
            "markers": ["consegna-fatta-99999999", "successore-di-99999999-aaaa"],
        },
    ),
    (
        "--fine senza identificativo non cancella niente",
        {
            "stdin": json.dumps({"cwd": "/x"}),
            "argv": ["--fine"],
            "env": {"ORCA_WORKTREE_ID": "w::/x", "ORCA_TAB_ID": "t"},
            "markers": ["consegna-fatta-31a570a5"],
        },
    ),
]


def main() -> int:
    if not BINARY.exists():
        print(f"manca il binario: {BINARY}\n  cargo build --release", file=sys.stderr)
        return 2
    if not ORIGINAL.exists():
        print(f"manca l'originale: {ORIGINAL}", file=sys.stderr)
        return 2

    bad = 0
    for name, case in CASES:
        py = run_side("python", case)
        rs = run_side("rust", case)
        diffs = [k for k in sorted(set(py) | set(rs)) if py.get(k) != rs.get(k)]
        if diffs:
            bad += 1
            print(f"  DIVERGE    {name}  → {', '.join(diffs)}")
            for k in diffs:
                print(f"      python {k}: {str(py.get(k))[:300]!r}")
                print(f"      rust   {k}: {str(rs.get(k))[:300]!r}")
        else:
            print(f"  uguale     {name}")

    # La prova del confronto stesso: se `updated_at` sparisse dal record, la
    # normalizzazione non deve nasconderlo.
    probe = run_side("rust", CASES[1][1])
    record = next((v for k, v in probe.items() if k.startswith("live/")), "")
    if '"updated_at": <TIME>' not in record:
        print("  ATTENZIONE  il record non porta `updated_at`: la normalizzazione copre un buco")
        bad += 1

    print(f"\n{len(CASES)} casi, {'nessuna divergenza' if not bad else f'{bad} divergenze'}")
    return 1 if bad else 0


if __name__ == "__main__":
    sys.exit(main())
