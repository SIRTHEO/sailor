#!/usr/bin/env python3
"""Confronta il presidio della consegna (PostToolUse) col suo porto in Rust.

L'ORACOLO È IL PYTHON. Il porto è giusto quando non lo si distingue: stessa
uscita **byte a byte**, stesso codice, stessi marcatori lasciati sul disco,
stessa riga nel registro dei ganci.

L'USCITA È LA DECISIONE, e va confrontata come testo. Questo gancio non ritorna
un verdetto: ritorna un JSON la cui *forma* determina chi lo legge —
`systemMessage` arriva all'utente e Claude non lo vede, `additionalContext`
dentro `hookSpecificOutput` arriva all'assistente, `decision: block` rifiuta lo
strumento. Un confronto che normalizzasse il JSON non vedrebbe la differenza fra
un avviso che raggiunge chi può agire e uno che si perde — ed è un difetto già
capitato una volta in questa migrazione.

DUE SPECIE DI CASI, e servono entrambe:
  * TRASCRIZIONI VERE, per il realismo della misura e del modello;
  * FASCE COSTRUITE, perché sul disco di oggi non esiste una sessione che stia
    fra l'avviso e l'obbligo, e senza quella il ramo dell'avviso non viene mai
    percorso: sarebbe verde perché nessuno ci passa.

DUE HOME FINTE, UNA PER PARTE. Il gancio **scrive** mentre decide — il marcatore
d'avviso, il contatore dei rifiuti, il memo della misura, quello delle
ripartenze. Con una cartella sola la seconda implementazione leggerebbe il
lavoro della prima e sarebbe d'accordo con sé stessa. Nessuna delle due è
`~/.claude/state`, che viene fotografato prima e dopo.

    python3 tools/compare-handoff-required.py
"""
import json
import os
import shutil
import subprocess
import sys
from pathlib import Path

ROOT = Path.home() / '.claude'
HOOK = ROOT / 'skills/hooks/handoff-required.py'
BINARY = ROOT / 'rust/target/release/claude-hooks'
REAL_STATE = ROOT / 'state'
SCRATCH = Path(os.environ.get(
    'RELAY_SCRATCH',
    '/private/tmp/claude-501/-Users-theo-orca-general/scratchpad')) / 'handoff-required'


def transcript_con(used, model='claude-opus-5', restarts=0):
    """Una trascrizione costruita che misura esattamente `used` token.

    Compatta, senza spazi dopo i due punti: è la forma con cui l'harness scrive
    le trascrizioni vere, e i filtri grezzi delle due implementazioni cercano le
    chiavi alla lettera.
    """
    compatto = lambda d: json.dumps(d, separators=(',', ':'))
    righe = []
    for _ in range(restarts):
        righe.append(compatto({'type': 'user', 'message': {
            'content': 'This session is being continued from a previous conversation'}}))
    righe.append(compatto({'type': 'assistant', 'message': {
        'model': model,
        'usage': {'input_tokens': 0, 'cache_read_input_tokens': used,
                  'cache_creation_input_tokens': 0, 'output_tokens': 1}}}))
    return '\n'.join(righe) + '\n'


def build_home(root: Path, caso):
    shutil.rmtree(root, ignore_errors=True)
    state = root / '.claude' / 'state'
    state.mkdir(parents=True)
    sess = caso['session'][:8]

    if caso.get('transcript_reale'):
        percorso = caso['transcript_reale']
    else:
        percorso = str(root / 'transcript.jsonl')
        Path(percorso).write_text(transcript_con(
            caso['used'], caso.get('model', 'claude-opus-5'),
            caso.get('restarts', 0)))

    # IL REGISTRO FA PARTE DELLA RISPOSTA, e con una HOME finta il Python non lo
    # trova: `hook_log` sta sotto `$HOME/.claude/scripts`, quindi l'import
    # ripiega su una funzione muta e il confronto vedrebbe registrare solo il
    # Rust. Sedici divergenze su diciotto, tutte dell'apparecchio.
    scripts = root / '.claude' / 'scripts'
    scripts.mkdir(parents=True, exist_ok=True)
    shutil.copy2(ROOT / 'scripts' / 'hook_log.py', scripts / 'hook_log.py')

    if caso.get('consegna_fatta'):
        (state / f'consegna-fatta-{sess}').write_text('1')
    if caso.get('consegna_a_ripartenze') is not None:
        (state / f'consegna-fatta-ripartenze-{sess}').write_text(
            str(caso['consegna_a_ripartenze']))
    if caso.get('gia_avvisata'):
        (state / f'consegna-avvisata-{sess}').write_text('1')
    if caso.get('blocchi'):
        (state / f'consegna-blocchi-{sess}').write_text(str(caso['blocchi']))
    return root, percorso


def run_side(which, caso, home, percorso):
    payload = {
        'session_id': caso['session'],
        'transcript_path': percorso,
        'tool_name': caso['tool'],
        'tool_input': caso.get('tool_input', {}),
    }
    payload.update(caso.get('payload_extra', {}))
    if caso.get('payload_grezzo') is not None:
        testo = caso['payload_grezzo']
    else:
        testo = json.dumps(payload)
    cmd = [str(BINARY), 'handoff-required'] if which == 'rust' else [sys.executable, str(HOOK)]
    out = subprocess.run(cmd, input=testo, capture_output=True, text=True,
                         timeout=300, env={**os.environ, 'HOME': str(home)})
    return out.returncode, out.stdout, out.stderr


def snapshot(home):
    """I marcatori rimasti, e la riga scritta nel registro dei ganci."""
    state = home / '.claude' / 'state'
    files = []
    for p in sorted(state.rglob('*')):
        if not p.is_file() or p.name.endswith('.jsonl'):
            continue
        try:
            files.append(f'{p.relative_to(state)} = {p.read_text(errors="replace").strip()}')
        except OSError:
            files.append(f'{p.relative_to(state)} = <illeggibile>')
    registro = state / 'ganci.jsonl'
    righe = []
    if registro.exists():
        for r in registro.read_text(errors='replace').splitlines():
            try:
                d = json.loads(r)
            except Exception:
                righe.append(r)
                continue
            # `quando` cambia a ogni esecuzione ed è l'unico campo escluso.
            d.pop('quando', None)
            righe.append(json.dumps(d, sort_keys=True, ensure_ascii=False))
    return files, righe


def casi():
    veri = sorted((ROOT / 'projects').glob('*/*.jsonl'), key=os.path.getsize)
    grande = str(veri[-1]) if veri else ''
    medio = str(veri[len(veri) // 2]) if veri else ''
    base = {'session': 'provahr0-0000-0000', 'tool': 'Bash'}
    return [
        ('sotto l avviso', {**base, 'used': 100_000}),
        ('appena sotto l avviso', {**base, 'used': 389_999}),
        ('esattamente sull avviso', {**base, 'used': 390_000}),
        ('fra le soglie, mai avvisata', {**base, 'used': 400_000}),
        ('fra le soglie, gia avvisata', {**base, 'used': 400_000, 'gia_avvisata': True}),
        ('appena sotto l obbligo', {**base, 'used': 449_999}),
        ('sull obbligo, strumento che non serve', {**base, 'used': 450_000}),
        ('sopra l obbligo, Bash', {**base, 'used': 480_000}),
        ('sopra l obbligo, Read passa', {**base, 'used': 480_000, 'tool': 'Read'}),
        ('sopra l obbligo, Skill passa', {**base, 'used': 480_000, 'tool': 'Skill'}),
        ('sopra l obbligo, Write passa', {**base, 'used': 480_000, 'tool': 'Write'}),
        ('sopra l obbligo, con consegna valida',
         {**base, 'used': 480_000, 'consegna_fatta': True}),
        ('consegna fatta ma la sessione e ripartita dopo',
         {**base, 'used': 480_000, 'consegna_fatta': True, 'restarts': 3,
          'consegna_a_ripartenze': 1}),
        ('consegna fatta, nessun riferimento: vale un giro',
         {**base, 'used': 480_000, 'consegna_fatta': True, 'restarts': 2}),
        ('quinto rifiuto: ancora blocca', {**base, 'used': 480_000, 'blocchi': 5}),
        ('sesto rifiuto: si arrende', {**base, 'used': 480_000, 'blocchi': 6}),
        ('molto oltre il tetto', {**base, 'used': 480_000, 'blocchi': 99}),
        ('la chiamata alla skill handoff',
         {**base, 'used': 480_000, 'tool': 'Skill',
          'tool_input': {'skill': 'handoff'}}),
        ('una Skill che non e handoff',
         {**base, 'used': 480_000, 'tool': 'Skill', 'tool_input': {'skill': 'learn'}}),
        ('modello sconosciuto', {**base, 'used': 175_000, 'model': 'gpt-9'}),
        ('opus 4.8 ha un budget diverso',
         {**base, 'used': 190_000, 'model': 'claude-opus-4-8'}),
        ('transcript assente', {**base, 'used': 480_000,
                                'payload_extra': {'transcript_path': ''}}),
        ('payload non JSON', {**base, 'used': 1, 'payload_grezzo': 'non e json'}),
        ('payload vuoto', {**base, 'used': 1, 'payload_grezzo': '{}'}),
        ('senza tool_name', {**base, 'used': 480_000,
                             'payload_extra': {'tool_name': ''}}),
        ('usage vuoto non azzera la misura',
         {**base, 'used': 480_000,
          'payload_extra': {}, 'usage_vuoto': True}),
        ('trascrizione vera, la piu grande',
         {**base, 'transcript_reale': grande, 'used': 0}),
        ('trascrizione vera, una qualunque',
         {**base, 'transcript_reale': medio, 'used': 0}),
    ]


def main():
    if not BINARY.exists():
        print('binario assente: compila prima', file=sys.stderr)
        return 1
    SCRATCH.mkdir(parents=True, exist_ok=True)
    prima = {str(p): p.stat().st_mtime for p in REAL_STATE.rglob('*') if p.is_file()}

    divergenti = 0
    tutti = casi()
    for nome, caso in tutti:
        got = {}
        for which in ('python', 'rust'):
            home, percorso = build_home(SCRATCH / f'home-{which}', caso)
            esito = run_side(which, caso, home, percorso)
            got[which] = (esito, *snapshot(home))
        if got['python'] != got['rust']:
            divergenti += 1
            print(f'\nDIVERGE [{nome}]')
            etichette = ('uscita del gancio', 'marcatori', 'registro')
            for i, etichetta in enumerate(etichette):
                a, b = got['rust'][i], got['python'][i]
                if a == b:
                    continue
                print(f'  {etichetta}:')
                print(f'    rust  ={a}')
                print(f'    python={b}')

    dopo = {str(p): p.stat().st_mtime for p in REAL_STATE.rglob('*') if p.is_file()}
    if prima != dopo:
        cambiati = {k for k in set(prima) | set(dopo) if prima.get(k) != dopo.get(k)}
        print(f'ATTENZIONE: ~/.claude/state e cambiato: {sorted(cambiati)}',
              file=sys.stderr)
        divergenti += 1

    print(f'\n{len(tutti)} scenari confrontati, {divergenti} divergenti')
    return 1 if divergenti else 0


if __name__ == '__main__':
    sys.exit(main())
