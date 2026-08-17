#!/usr/bin/env python3
"""Confronta il gancio che concede le cancellazioni col suo porto in Rust.

L'ORACOLO È IL PYTHON. Su un gancio che **concede permessi** il confronto conta
più che altrove: una divergenza qui non è un messaggio diverso, è una
cancellazione autorizzata che non doveva esserlo. Si confrontano uscita, stdout
byte a byte — la sua *forma* è la decisione, `{"behavior": "allow"}` — e la riga
lasciata nel registro delle concessioni.

DUE HOME FINTE, UNA PER PARTE. Il gancio **scrive** quando concede, e con una
cartella sola la seconda implementazione leggerebbe il lavoro della prima.

I COLLEGAMENTI SIMBOLICI VOGLIONO UN DISCO VERO, ed è l'unica parte del giudizio
che a tavolino non si vede: un worktree può contenere un collegamento a
`node_modules` del checkout canonico, e cancellare *attraverso* quel collegamento
cancella roba del canonico. Quindi l'albero finto si costruisce davvero, con un
collegamento vero che esce dai confini.

`WORKSPACES` DIVENTA QUELLO FINTO PER ENTRAMBI: il Python lo legge da una
variabile di modulo calcolata sull'HOME, il Rust da `$HOME/orca/workspaces`.
Puntando `HOME` alla stessa cartella finta, i due giudicano lo stesso albero —
senza toccare le copie di lavoro vere.

    python3 tools/compare-worktree-deletes.py
"""
import json
import os
import shutil
import subprocess
import sys
from pathlib import Path

ROOT = Path.home() / '.claude'
HOOK = ROOT / 'skills/hooks/allow-worktree-deletes.py'
BINARY = ROOT / 'rust/target/release/claude-hooks'
SCRATCH = Path(os.environ.get(
    'RELAY_SCRATCH',
    '/private/tmp/claude-501/-Users-theo-orca-general/scratchpad')) / 'worktree-deletes'


def prepara(home: Path) -> str:
    """Costruisce l'albero finto e ritorna la radice delle copie di lavoro."""
    shutil.rmtree(home, ignore_errors=True)
    w = home / 'orca' / 'workspaces'
    (home / '.claude' / 'state').mkdir(parents=True)
    # Un albero minimo ma vero: due copie di lavoro, materiale dentro, e un
    # collegamento che esce.
    for p in ('repo/wt/dist', 'repo/wt/src', 'altro/wt2/dist', 'suite/wt/build'):
        (w / p).mkdir(parents=True, exist_ok=True)
    fuori = home / 'canonico' / 'node_modules'
    fuori.mkdir(parents=True, exist_ok=True)
    link = w / 'repo' / 'wt' / 'node_modules'
    if not link.exists():
        link.symlink_to(fuori)
    (w / 'repo' / 'wt' / 'dist' / 'file.txt').write_text('x')
    return str(w)


def registro(home: Path):
    """L'ultima riga del registro delle concessioni, normalizzata sull'HOME."""
    p = home / '.claude' / 'state' / 'cancellazioni-concesse.jsonl'
    if not p.exists():
        return None
    righe = [r for r in p.read_text(errors='replace').splitlines() if r.strip()]
    if not righe:
        return None
    try:
        o = json.loads(righe[-1])
    except Exception:
        return {'<riga illeggibile>': righe[-1]}
    # L'istante differisce per costruzione; l'HOME anche, ed è la stessa
    # normalizzazione degli altri confronti.
    o.pop('quando', None)
    return {k: (v.replace(str(home), '<HOME>') if isinstance(v, str) else v)
            for k, v in o.items()}


def esegui(comando, home: Path, command_line: str, tool='Bash'):
    w = prepara(home)
    payload = {
        'tool_name': tool,
        'session_id': 'prova-wd-0000-0000',
        'cwd': str(home),
        'tool_input': {'command': command_line.replace('<W>', w)},
    }
    env = dict(os.environ)
    env['HOME'] = str(home)
    p = subprocess.run(comando, input=json.dumps(payload), capture_output=True,
                       text=True, timeout=60, cwd=str(home), env=env)
    # La HOME isolata si chiama `home-python` di qua e `home-rust` di la, e il
    # messaggio cita la radice delle copie: senza normalizzarla ogni concessione
    # risulta diversa. Sette divergenze finte al primo giro, tutte qui — la
    # stessa trappola gia' pagata dal confronto della staffetta.
    return p.returncode, p.stdout.replace(str(home), '<HOME>'), registro(home)


def casi():
    return [
        ('il caso vero del giro 864', 'rm -rf <W>/packages/theo-auto-pg/packages/geo/dist'),
        ('una variabile del comando', 'R=<W>/repo/wt\nrm -rf "$R/dist"'),
        ('la stessa con le graffe', 'R=<W>/repo/wt\nrm -rf "${R}/dist"'),
        ('piu bersagli dentro', 'rm -rf <W>/repo/wt/dist <W>/repo/wt/src'),
        ('senza -r', 'rm <W>/repo/wt/dist/file.txt'),
        ('con il doppio trattino', 'rm -rf -- <W>/repo/wt/dist'),
        ('solo opzioni, nessun bersaglio', 'rm -rf --'),
        ('il checkout canonico', 'rm -rf /Users/theo/gyver/work/suite/dist'),
        ('la radice di workspaces', 'rm -rf <W>'),
        ('un repo intero', 'rm -rf <W>/repo'),
        ('un worktree intero', 'rm -rf <W>/repo/wt'),
        ('un bersaglio fuori su due', 'rm -rf <W>/repo/wt/dist /tmp/y'),
        ('la risalita che esce', 'rm -rf <W>/repo/wt/../../../../etc'),
        ('la risalita che resta dentro', 'rm -rf <W>/repo/wt/../../altro/wt2/dist'),
        ('un nome che somiglia a workspaces', 'rm -rf <W>XX/aa/bb'),
        ('variabile non definita', 'rm -rf "$NONDEFINITA/x"'),
        ('sostituzione di comando', 'rm -rf $(cat lista)'),
        ('sostituzione con backtick', 'rm -rf `cat lista`'),
        ('find -delete', 'find <W>/repo/wt -name "*.o" -delete'),
        ('xargs', 'echo x | xargs rm -rf'),
        ('find -exec', 'find <W>/repo/wt -name "*.o" -exec rm {} ;'),
        ('secondo comando dopo &&', 'rm -rf <W>/repo/wt/dist && rm -rf /etc/passwd'),
        ('dopo il punto e virgola', 'rm -rf <W>/repo/wt/dist ; rm -rf /var/tmp'),
        ('dopo ||', 'rm -rf <W>/repo/wt/dist || rm -rf /var/tmp'),
        ('dopo una pipe', 'rm -rf <W>/repo/wt/dist | rm -rf /var/tmp'),
        ('bersaglio relativo', 'cd <W>/repo/wt && rm -rf dist'),
        # I due casi che il primo giro di mutanti ha preteso, e che senza di essi
        # lasciavano passare due buchi veri (misurato il 17/08/2026):
        #
        # 1. un `rm` illeggibile ACCANTO a uno buono. Ignorare la riga che non si
        #    sa leggere, invece di fermarsi, concede la prima cancellazione e
        #    lascia correre la seconda: qui il bersaglio buono da solo passerebbe.
        ('un rm leggibile e uno no, nello stesso comando',
         'rm -rf <W>/repo/wt/dist && rm -rf "$IGNOTA/x"'),
        # 2. una variabile ignota IN CODA a un percorso valido. Trattarla come
        #    stringa vuota non produce un percorso strano ma uno legittimo — e
        #    quindi una concessione. Col solo `"$NONDEFINITA/x"` il mutante
        #    sopravvive, perche' anche da vuota quel percorso resta fuori.
        ('variabile ignota in coda a un percorso valido',
         'rm -rf "<W>/repo/wt/dist$IGNOTA"'),
        ('nessuna cancellazione', 'npm test'),
        ('comando vuoto', ''),
        ('solo spazi', '   '),
        ('virgoletta aperta', 'rm -rf "<W>/repo/wt/dist'),
        # I collegamenti: l'unica parte che vuole il disco.
        ('il collegamento stesso', 'rm -rf <W>/repo/wt/node_modules'),
        ('attraverso il collegamento', 'rm -rf <W>/repo/wt/node_modules/qualcosa'),
        ('una cartella vera accanto al collegamento', 'rm -rf <W>/repo/wt/dist'),
        # Perimetro: un altro strumento non è affar suo, per quanto il comando
        # somigli a una cancellazione autorizzabile.
        ('uno strumento che non e Bash', 'rm -rf <W>/repo/wt/dist', 'Read'),
    ]


def main():
    if not BINARY.exists():
        print(f'manca {BINARY}: cargo build --release')
        return 1
    SCRATCH.mkdir(parents=True, exist_ok=True)
    divergenze = 0
    elenco = casi()
    for caso in elenco:
        nome, comando = caso[0], caso[1]
        tool = caso[2] if len(caso) > 2 else 'Bash'
        try:
            attesa = esegui([sys.executable, str(HOOK)], SCRATCH / 'home-python',
                            comando, tool)
            ottenuta = esegui([str(BINARY), 'allow-worktree-deletes'],
                              SCRATCH / 'home-rust', comando, tool)
        except Exception as exc:                       # pragma: no cover
            print(f'  ERRORE      {nome}: {exc}')
            divergenze += 1
            continue
        if attesa == ottenuta:
            concesso = 'ALLOW' if attesa[1].strip() else 'tace'
            print(f'  uguale      {nome}  ({concesso})')
            continue
        divergenze += 1
        print(f'  DIVERGE     {nome}')
        if attesa[0] != ottenuta[0]:
            print(f'      uscita   python={attesa[0]}  rust={ottenuta[0]}')
        if attesa[1] != ottenuta[1]:
            print(f'      python: {attesa[1]!r}')
            print(f'      rust:   {ottenuta[1]!r}')
        if attesa[2] != ottenuta[2]:
            print(f'      registro python: {attesa[2]!r}')
            print(f'      registro rust:   {ottenuta[2]!r}')
    print()
    if divergenze:
        print(f'{divergenze} divergenze su {len(elenco)} casi')
    else:
        print(f'{len(elenco)} casi, nessuna divergenza')
    return 1 if divergenze else 0


if __name__ == '__main__':
    sys.exit(main())
