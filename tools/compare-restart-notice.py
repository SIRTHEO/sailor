#!/usr/bin/env python3
"""Confronta il promemoria di ripartenza in Rust con l'originale in Python.

IL CORPUS SONO LE TRASCRIZIONI VERE, tutte quelle sul disco. È il corpus giusto
perché il difetto che questo gancio esiste per non commettere — contare come
compattazione una riga che *parla* di compattazioni — vive esattamente lì: nelle
sessioni che hanno lavorato su questo codice, dove la frase compare dentro
rapporti, grep e sorgenti. Su casi costruiti non si presenterebbe mai. Misurato
dall'originale il 10/08/2026: 17 presunte ripartenze contro una vera.

TRE DOMANDE, NON UNA:
  1. il conteggio (ripartenze, chiamate) su ogni trascrizione vera;
  2. il testo del messaggio, byte a byte, su una griglia di valori — è ciò che
     la sessione legge, e una parola diversa è una divergenza;
  3. il gancio intero da stdin, dove si decide anche **quando tacere**: solo
     `source == compact` parla, e una partenza normale che parlasse sarebbe
     rumore a ogni avvio.

Il tetto sui byte è dichiarato e stampato: le trascrizioni arrivano a 245 MB e
questo gancio le attraversa tutte, non la coda. Un confronto che ne salta una
parte in silenzio si legge come «ho coperto tutto».

    python3 tools/compare-restart-notice.py [tetto_gb]
"""
import glob
import json
import os
import subprocess
import sys

HOOK = os.path.expanduser('~/.claude/skills/hooks/restart-notice.py')
BINARY = os.path.expanduser('~/.claude/rust/target/release/claude-hooks')
TRANSCRIPTS = os.path.expanduser('~/.claude/projects/*/*.jsonl')


def python_count(path):
    """Il conteggio dell'oracolo, chiamando la sua `count()`."""
    code = (
        'import importlib.util,json,sys;'
        f'spec=importlib.util.spec_from_file_location("rn","{HOOK}");'
        'm=importlib.util.module_from_spec(spec);spec.loader.exec_module(m);'
        'r,t=m.count(sys.argv[1]);'
        'print(json.dumps({"restarts":r,"tool_calls":t}))'
    )
    out = subprocess.run([sys.executable, '-c', code, path],
                         capture_output=True, text=True, timeout=900)
    return out.stdout.strip()


def rust_count(path):
    out = subprocess.run([BINARY, 'restart-count', path],
                         capture_output=True, text=True, timeout=900)
    return out.stdout.strip()


def python_message(restarts, tool_calls):
    code = (
        'import importlib.util,sys;'
        f'spec=importlib.util.spec_from_file_location("rn","{HOOK}");'
        'm=importlib.util.module_from_spec(spec);spec.loader.exec_module(m);'
        'sys.stdout.write(m.message(int(sys.argv[1]),int(sys.argv[2])))'
    )
    return subprocess.run([sys.executable, '-c', code, str(restarts), str(tool_calls)],
                          capture_output=True, text=True, timeout=60).stdout


def hook_end_to_end(which, payload):
    """Il gancio intero: stesso stdin, si confrontano uscita e codice."""
    cmd = [BINARY, 'restart-notice'] if which == 'rust' else [sys.executable, HOOK]
    out = subprocess.run(cmd, input=json.dumps(payload), capture_output=True,
                         text=True, timeout=900)
    return out.returncode, out.stdout, out.stderr


def main():
    tetto_gb = float(sys.argv[1]) if len(sys.argv) > 1 else 3.0
    tetto = tetto_gb * 1024 ** 3
    if not os.path.exists(BINARY):
        print('binario assente: compila prima', file=sys.stderr)
        return 1

    files = sorted(glob.glob(TRANSCRIPTS), key=os.path.getsize)
    usati, saltati, letti = [], 0, 0
    for f in files:
        size = os.path.getsize(f)
        if letti + size > tetto:
            saltati += 1
            continue
        usati.append(f)
        letti += size

    def numeri(testo):
        # Si confrontano i VALORI, non le stringhe: `json.dumps` di Python mette
        # uno spazio dopo i due punti e `serde_json` no, e su 1279 trascrizioni
        # quella differenza si legge come 1279 divergenze che non esistono. Dove
        # invece la forma È la risposta — il testo del messaggio, la riga di
        # registro — si confronta il testo, e infatti qui sotto si fa così.
        try:
            return json.loads(testo)
        except Exception:
            return {'illeggibile': testo}

    diverse = 0
    for f in usati:
        a, b = numeri(rust_count(f)), numeri(python_count(f))
        if a != b:
            diverse += 1
            print(f'\nDIVERGE il conteggio su {os.path.basename(f)}')
            print(f'    rust  ={a}')
            print(f'    python={b}')
    print(f'{len(usati)} trascrizioni confrontate ({letti / 1024**3:.1f} GB), '
          f'{diverse} divergenti')
    if saltati:
        print(f'   saltate {saltati} oltre il tetto di {tetto_gb} GB '
              f'(si alza col primo argomento)')

    # 2. il testo, sui due lati di ciascuna soglia e su un valore qualunque
    griglia = [(0, 0), (1, 10), (2, 999), (4, 2999), (5, 10), (0, 3000),
               (5, 3000), (9, 4000), (12, 120000)]
    testo_diverso = 0
    for r, t in griglia:
        atteso = python_message(r, t)
        # Il messaggio del Rust si ottiene dal gancio intero, con un transcript
        # costruito che produce esattamente quei due numeri: è il solo modo di
        # confrontare **ciò che la sessione legge** invece di una funzione.
        got = rust_message(r, t)
        if got != atteso:
            testo_diverso += 1
            print(f'\nDIVERGE il testo per ({r}, {t})')
            for i, (x, y) in enumerate(zip(got.splitlines(), atteso.splitlines())):
                if x != y:
                    print(f'    riga {i}:\n      rust  ={x!r}\n      python={y!r}')
                    break
            if len(got.splitlines()) != len(atteso.splitlines()):
                print(f'    righe: rust={len(got.splitlines())} '
                      f'python={len(atteso.splitlines())}')
    print(f'{len(griglia)} messaggi confrontati, {testo_diverso} divergenti')

    # 3. il gancio intero, dove si decide quando tacere
    scenari = [
        ('partenza normale', {'source': 'startup', 'transcript_path': usati[-1] if usati else ''}),
        ('ripresa', {'source': 'resume', 'transcript_path': usati[-1] if usati else ''}),
        ('cancellata', {'source': 'clear', 'transcript_path': ''}),
        ('compattazione senza percorso', {'source': 'compact', 'transcript_path': ''}),
        ('compattazione su file assente',
         {'source': 'compact', 'transcript_path': '/non/esiste.jsonl'}),
        ('payload senza campi', {}),
    ]
    if usati:
        scenari.append(('compattazione vera',
                        {'source': 'compact', 'transcript_path': usati[0]}))
    ganci_diversi = 0
    for nome, payload in scenari:
        a, b = hook_end_to_end('rust', payload), hook_end_to_end('python', payload)
        if a != b:
            ganci_diversi += 1
            print(f'\nDIVERGE il gancio [{nome}]\n    rust  ={a}\n    python={b}')
    print(f'{len(scenari)} scenari del gancio, {ganci_diversi} divergenti')

    return 1 if (diverse or testo_diverso or ganci_diversi) else 0


def rust_message(restarts, tool_calls):
    """Il messaggio del Rust, ottenuto costruendo un transcript su misura.

    Non c'è un sottocomando che stampi il testo a partire da due numeri, e
    aggiungerlo significherebbe confrontare una funzione invece del gancio. Si
    fabbrica invece una trascrizione che produce esattamente quel conteggio: una
    riga di ripartenza per ognuna, e una riga con N chiamate.
    """
    import tempfile
    # SENZA SPAZI DOPO I DUE PUNTI, e non è pignoleria: il filtro grezzo di
    # entrambe le implementazioni cerca `"type":"tool_use"` alla lettera, che è
    # la forma con cui l'harness scrive le trascrizioni vere. Con lo spazio che
    # `json.dumps` mette di default il conteggio è zero, e le prime otto
    # divergenze di questo confronto erano tutte dell'apparecchio.
    compatto = lambda d: json.dumps(d, separators=(',', ':'))
    riga_restart = compatto({'type': 'user', 'message': {
        'content': 'This session is being continued from a previous conversation'}})
    with tempfile.NamedTemporaryFile('w', suffix='.jsonl', delete=False) as f:
        for _ in range(restarts):
            f.write(riga_restart + '\n')
        if tool_calls:
            f.write(compatto({'type': 'assistant', 'message': {'content': [
                {'type': 'tool_use', 'name': 'X'}] * tool_calls}}) + '\n')
        path = f.name
    try:
        out = subprocess.run([BINARY, 'restart-notice'],
                             input=json.dumps({'source': 'compact',
                                               'transcript_path': path}),
                             capture_output=True, text=True, timeout=120)
        # Il gancio stampa con `println!`: via l'a-capo che l'oracolo non mette.
        return out.stdout[:-1] if out.stdout.endswith('\n') else out.stdout
    finally:
        os.unlink(path)


if __name__ == '__main__':
    sys.exit(main())
