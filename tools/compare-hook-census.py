#!/usr/bin/env python3
"""Confronta il censimento dei ganci col suo porto in Rust.

L'ORACOLO È IL PYTHON. Questo strumento non blocca e non concede niente: ciò
che può divergere è **cosa dichiara** — quanti ganci, quali morti, quali orfani
e con che motivo — e **cosa lascia scritto** (`state/census-needles.txt` e
`state/hook-census.baseline.json`). Si confrontano byte per byte uscita, stdout,
stderr e i due file.

L'ORIGINALE HA LA HOME SCRITTA IN CHIARO (`HOME = Path("/Users/theo/.claude")`),
quindi non si può spostare da fuori. Qui se ne fa una **copia** in una cartella
temporanea con quella riga e la lista delle radici sostituite: l'originale sul
disco non si tocca mai. Il Rust legge le stesse due cose da
`CLAUDE_CENSUS_HOME` e `CLAUDE_CENSUS_ROOTS`, che esistono solo per questo.

PERCHÉ NON SI CONFRONTA SUL DISCO VERO. Una passata sulle trenta radici vere
costa due minuti e attraversa i repo, dove in questo momento lavorano altre
sessioni: il risultato cambierebbe fra una implementazione e l'altra per motivi
che non c'entrano col porto. Peggio, sarebbe verde per caso.

DUE HOME FINTE, UNA PER PARTE: il censimento **scrive** (gli aghi, la linea di
base). Con una cartella sola la seconda implementazione leggerebbe il lavoro
della prima e il confronto direbbe verde per il motivo sbagliato.

    python3 tools/compare-hook-census.py
"""
import json
import os
import re
import shutil
import subprocess
import sys
import tempfile
from pathlib import Path

CONFIG = Path("/Users/theo/.claude")
ORIGINAL = CONFIG / "scripts" / "hook-census.py"
BINARY = CONFIG / "rust" / "target" / "release" / "claude-hooks"


def build_home(root: Path, case: dict) -> tuple[Path, Path]:
    """Costruisce una configurazione finta e la radice che la nomina."""
    home = root / "claude"
    (home / "scripts").mkdir(parents=True, exist_ok=True)
    (home / "skills" / "hooks").mkdir(parents=True, exist_ok=True)
    (home / "state").mkdir(parents=True, exist_ok=True)
    # `{HOME}` nei comandi si risolve solo adesso: la cartella finta cambia a
    # ogni giro, e le due parti ne hanno una ciascuna.
    settings = json.loads(json.dumps(case["settings"]).replace("{HOME}", str(home)))
    (home / "settings.json").write_text(json.dumps(settings, indent=2))

    for rel, body in case["files"].items():
        p = home / rel
        p.parent.mkdir(parents=True, exist_ok=True)
        p.write_text(body)

    caller = root / "callers"
    caller.mkdir(parents=True, exist_ok=True)
    for rel, body in case.get("callers", {}).items():
        p = caller / rel
        p.parent.mkdir(parents=True, exist_ok=True)
        p.write_text(body)

    if case.get("baseline") is not None:
        (home / "state" / "hook-census.baseline.json").write_text(
            json.dumps({"orfani": case["baseline"]}, indent=2) + "\n"
        )
    return home, caller


def patched_python(home: Path, roots: list[str], into: Path) -> Path:
    """Copia dell'originale con HOME e radici sostituite. L'originale non si tocca."""
    source = ORIGINAL.read_text()
    source, n = re.subn(
        r'HOME = Path\("[^"]+"\)', f'HOME = Path({str(home)!r})', source, count=1
    )
    assert n == 1, "la riga HOME dell'originale è cambiata: aggiorna il confronto"
    # La lista delle radici: si sostituisce il blocco che la costruisce, dalla
    # riga `roots = [` fino al `roots = [r for r in roots if ...]`.
    source, n = re.subn(
        r"roots = \[str\(HOME\).*?roots = \[r for r in roots if Path\(r\)\.exists\(\)\]",
        f"roots = [r for r in {roots!r} if Path(r).exists()]",
        source,
        count=1,
        flags=re.S,
    )
    assert n == 1, "il blocco delle radici è cambiato: aggiorna il confronto"
    out = into / "hook-census-patched.py"
    out.write_text(source)
    return out


def run_case(case: dict, argv: list[str]) -> tuple[dict, dict]:
    """Esegue le due implementazioni su due copie identiche e separate."""
    results = []
    for side in ("python", "rust"):
        with tempfile.TemporaryDirectory() as tmp:
            root = Path(tmp)
            home, caller = build_home(root, case)
            roots = [str(home), str(caller)]
            env = {
                **os.environ,
                "CLAUDE_CENSUS_HOME": str(home),
                "CLAUDE_CENSUS_ROOTS": ":".join(roots),
            }
            if side == "python":
                script = patched_python(home, roots, root)
                cmd = [sys.executable, str(script), *argv]
            else:
                cmd = [str(BINARY), "hook-census", *argv]
            p = subprocess.run(cmd, capture_output=True, text=True, env=env, timeout=120)
            needles = home / "state" / "census-needles.txt"
            baseline = home / "state" / "hook-census.baseline.json"
            results.append(
                {
                    "exit": p.returncode,
                    "stdout": p.stdout.replace(str(home), "<HOME>").replace(str(caller), "<CALLERS>"),
                    "stderr": p.stderr.replace(str(home), "<HOME>").replace(str(caller), "<CALLERS>"),
                    "needles": needles.read_text() if needles.exists() else None,
                    "baseline": baseline.read_text() if baseline.exists() else None,
                }
            )
    return results[0], results[1]


HOOK = lambda cmd: {"hooks": {"PreToolUse": [{"matcher": "Bash", "hooks": [{"command": cmd}]}]}}

CASES = [
    (
        "un gancio che punta a uno script esistente",
        {
            "settings": HOOK("python3 {HOME}/scripts/alive.py"),
            "files": {"scripts/alive.py": "print(1)\n"},
        },
        [],
    ),
    (
        "un gancio che punta a un file sparito",
        {
            "settings": HOOK("python3 {HOME}/scripts/ghost.py"),
            "files": {"scripts/keep.py": "print(1)\n"},
        },
        [],
    ),
    (
        "gancio morto, forma breve per SessionStart",
        {
            "settings": HOOK("python3 {HOME}/scripts/ghost.py"),
            "files": {"scripts/keep.py": "print(1)\n"},
        },
        ["--fast"],
    ),
    (
        "tutto a posto, forma breve: deve tacere",
        {
            "settings": HOOK("python3 {HOME}/scripts/alive.py"),
            "files": {"scripts/alive.py": "print(1)\n"},
        },
        ["--fast"],
    ),
    (
        "uno script che non nomina nessuno è orfano",
        {
            "settings": HOOK("python3 {HOME}/scripts/wired.py"),
            "files": {"scripts/wired.py": "print(1)\n", "scripts/lonely.py": "print(2)\n"},
        },
        [],
    ),
    (
        "nominato solo in prosa non è lanciato",
        {
            "settings": HOOK("python3 {HOME}/scripts/wired.py"),
            "files": {"scripts/wired.py": "print(1)\n", "scripts/quoted.py": "print(2)\n"},
            "callers": {"nota.md": "qui si parla di quoted.py e basta\n"},
        },
        [],
    ),
    (
        "nominato da un punto d'esecuzione è vivo",
        {
            "settings": HOOK("python3 {HOME}/scripts/wired.py"),
            "files": {"scripts/wired.py": "print(1)\n", "scripts/called.py": "print(2)\n"},
            "callers": {"launcher.sh": "python3 called.py\n"},
        },
        [],
    ),
    (
        "un modulo importato col nome nudo non è orfano",
        {
            "settings": HOOK("python3 {HOME}/scripts/wired.py"),
            "files": {"scripts/wired.py": "print(1)\n", "scripts/hook_log.py": "def registra(): ...\n"},
            "callers": {"user.py": "import hook_log\n"},
        },
        [],
    ),
    (
        "le prove non si contano fra gli script",
        {
            "settings": HOOK("python3 {HOME}/scripts/wired.py"),
            "files": {
                "scripts/wired.py": "print(1)\n",
                "scripts/prova-x.py": "print(2)\n",
                "scripts/test-y.py": "print(3)\n",
                "scripts/z.test.py": "print(4)\n",
            },
        },
        [],
    ),
    (
        "un orfano già nella linea di base non è una notizia",
        {
            "settings": HOOK("python3 {HOME}/scripts/wired.py"),
            "files": {"scripts/wired.py": "print(1)\n", "scripts/lonely.py": "print(2)\n"},
            "baseline": ["scripts/lonely.py"],
        },
        [],
    ),
    (
        "un orfano nuovo rispetto alla linea di base lo è",
        {
            "settings": HOOK("python3 {HOME}/scripts/wired.py"),
            "files": {
                "scripts/wired.py": "print(1)\n",
                "scripts/lonely.py": "print(2)\n",
                "scripts/fresh.py": "print(3)\n",
            },
            "baseline": ["scripts/lonely.py"],
        },
        [],
    ),
    (
        "--update riscrive la linea di base",
        {
            "settings": HOOK("python3 {HOME}/scripts/wired.py"),
            "files": {"scripts/wired.py": "print(1)\n", "scripts/lonely.py": "print(2)\n"},
            "baseline": [],
        },
        ["--update"],
    ),
    (
        "nessun gancio dichiarato",
        {"settings": {"hooks": {}}, "files": {"scripts/lonely.py": "print(1)\n"}},
        [],
    ),
    (
        "un gancio con due script nello stesso comando",
        {
            "settings": HOOK("sh {HOME}/scripts/a.sh || python3 {HOME}/scripts/b.py"),
            "files": {"scripts/a.sh": "echo 1\n", "scripts/b.py": "print(1)\n"},
        },
        [],
    ),
    (
        "uno script in skills/hooks conta come gli altri",
        {
            "settings": HOOK("python3 {HOME}/scripts/wired.py"),
            "files": {"scripts/wired.py": "print(1)\n", "skills/hooks/nudge.py": "print(2)\n"},
        },
        [],
    ),
]


def main() -> int:
    if not BINARY.exists():
        print(f"manca il binario: {BINARY}\n  cargo build --release", file=sys.stderr)
        return 2

    bad = 0
    for name, case, argv in CASES:
        py, rs = run_case(case, argv)
        diffs = [k for k in ("exit", "stdout", "stderr", "needles", "baseline") if py[k] != rs[k]]
        label = " ".join(argv) or "(nessun argomento)"
        if diffs:
            bad += 1
            print(f"  DIVERGE    {name}  [{label}]  → {', '.join(diffs)}")
            for k in diffs:
                print(f"      python {k}: {str(py[k])[:400]!r}")
                print(f"      rust   {k}: {str(rs[k])[:400]!r}")
        else:
            print(f"  uguale     {name}  [{label}]")

    print(f"\n{len(CASES)} casi, {'nessuna divergenza' if not bad else f'{bad} divergenze'}")
    return 1 if bad else 0


if __name__ == "__main__":
    sys.exit(main())
