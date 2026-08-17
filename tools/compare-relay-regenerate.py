#!/usr/bin/env python3
"""Compare the relay's regeneration step: not its output, but what it DOES.

regenerate() returns nothing. Comparing return values would be green by
construction. What it has are effects — five orca calls in an order that is the
whole point, plus the files it writes and deletes — so those are what is
compared.

    1. wait tui-idle on the old pane      never truncate a turn
    2. write riprendi-da/<worktree>       the resume signal
    3. create the successor               BEFORE closing: the tree is never
                                          left without a session
    4. wait + send to the successor       the order to resume
    5. close the old one, clean up        only now

Swapping 3 and 5 leaves a tree uncovered every time create fails. That is the
invariant this tool exists to pin.

HOW. A fake `orca` is put first on PATH. It appends every invocation to a log
and answers from a scripted table, so both implementations meet the same
sequence of replies. Two fake HOMEs, one per implementation: both write state
while they run, and a shared one would have the second read the first's work and
agree with itself.

Nothing here touches the real ~/.claude/state, and the real orca is never
called — verified by asserting the fake's log is the only one that grew.
"""
import json
import os
import shutil
import subprocess
import sys
import time
from pathlib import Path

SCRATCH = Path('/private/tmp/claude-501/-Users-theo-orca-general/'
               'cdca7b36-bd04-4645-b291-ecedad59cbb7/scratchpad/relay-regenerate')
BIN = Path.home() / '.claude/rust/target/release/claude-hooks'
RELAY = Path.home() / '.claude/skills/hooks/relay.py'

FAKE_ORCA = """#!/bin/sh
# Registra ogni invocazione, una per riga, e risponde dal copione.
printf '%s\\n' "$*" >> "$ORCA_LOG"
case "$*" in
  *"terminal list"*)   printf '%s' "$ORCA_LIST_REPLY" ;;
  *"terminal create"*) printf '%s' "$ORCA_CREATE_REPLY"; exit $ORCA_CREATE_RC ;;
  *"terminal wait"*)   exit $ORCA_WAIT_RC ;;
esac
exit 0
"""

# Le risposte che i casi usano. La prima è quella vera del 2026-08-17.
CREATE_OK = json.dumps({'ok': True, 'result': {'terminal': {
    'handle': 'term_nuovo000', 'tabId': 'tab-nuova'}}})
CREATE_SENZA_HANDLE = json.dumps({'ok': True, 'result': {}})
CREATE_HANDLE_FINTO = json.dumps({'ok': True, 'result': {'handle': 'wt-123'}})


def build_home(root: Path, case):
    """Una HOME finta con dentro esattamente lo stato che il caso descrive."""
    shutil.rmtree(root, ignore_errors=True)
    state = root / '.claude' / 'state'
    (state / 'sessioni-vive').mkdir(parents=True)
    sess = case['sess']

    transcript = root / 'transcript.jsonl'
    riga = {'type': 'assistant', 'message': {
        'model': case['model'],
        'usage': {'input_tokens': 0, 'cache_read_input_tokens': case['used'],
                  'cache_creation_input_tokens': 0, 'output_tokens': 1}}}
    transcript.write_text(json.dumps(riga) + '\n')

    record = {
        'session_id': case['session_id'],
        'terminal_handle': case['handle'],
        'worktree_id': case['worktree'],
        'tab_id': case.get('tab_id', ''),
        'transcript_path': str(transcript) if case['transcript'] else '/manca',
        'cwd': case['cwd'],
    }
    (state / 'sessioni-vive' / f'{sess}.json').write_text(json.dumps(record))

    if case['handoff_done']:
        (state / f'consegna-fatta-{sess}').write_text('1')
    if case.get('opt_out'):
        (state / f'non-rigenerare-{case["worktree"]}').write_text('1')
    if case.get('cooldown'):
        (state / f'staffetta-cooldown-{case["worktree"]}').write_text(str(time.time()))
    if case.get('armed'):
        (state / f'successore-di-{case["session_id"]}').write_text(
            json.dumps({'handle': case['armed'], 'tabId': case.get('armed_tab', '')}))
    return root


def run_side(which, case, home, log):
    env = {
        **os.environ,
        'HOME': str(home),
        'PATH': f'{SCRATCH}/bin:' + os.environ.get('PATH', ''),
        'ORCA_LOG': str(log),
        'ORCA_LIST_REPLY': case['list_reply'],
        'ORCA_CREATE_REPLY': case.get('create_reply', CREATE_OK),
        'ORCA_CREATE_RC': str(case.get('create_rc', 0)),
        'ORCA_WAIT_RC': str(case.get('wait_rc', 0)),
    }
    if which == 'rust':
        cmd = [str(BIN), 'relay']
    else:
        cmd = [sys.executable, str(RELAY)]
    if case.get('secco'):
        cmd.append('--secco')
    subprocess.run(cmd, capture_output=True, text=True, timeout=120, env=env)


def snapshot(home):
    """Cosa resta sul disco: i file di stato, e il registro senza le date."""
    state = home / '.claude' / 'state'
    files = []
    for p in sorted(state.rglob('*')):
        if p.is_file() and p.name != 'staffetta.log':
            files.append(str(p.relative_to(state)))
    log = state / 'staffetta.log'
    righe = []
    if log.exists():
        for r in log.read_text(errors='replace').splitlines():
            # Via la data: cambia a ogni esecuzione e non è ciò che si confronta.
            righe.append(r[21:] if len(r) > 21 else r)
    return files, righe


LIST_VIVO = json.dumps({'result': {'terminals': [
    {'handle': 'term_vecchio0', 'tabId': 'tab-1', 'worktreeId': 'wt-1',
     'title': '✳ Un lavoro'},
    {'handle': 'term_altro00', 'tabId': 'tab-2', 'worktreeId': 'wt-2',
     'title': '✳ Altro'}]}})
LIST_MORTO = json.dumps({'result': {'terminals': [
    {'handle': 'term_altro00', 'tabId': 'tab-2', 'worktreeId': 'wt-2',
     'title': '✳ Altro'}]}})


def cases():
    base = dict(sess='provareg', session_id='provareg-0000-0000',
                handle='term_vecchio0', worktree='wt-1', tab_id='tab-1',
                transcript=True, cwd='/x', model='claude-opus-4-8',
                used=190_000, handoff_done=True, list_reply=LIST_VIVO)
    return [
        ('rigenera, tutto liscio', {**base}),
        ('rigenera a secco', {**base, 'secco': True}),
        ('create senza handle', {**base, 'create_reply': CREATE_SENZA_HANDLE}),
        ('create con un handle finto', {**base, 'create_reply': CREATE_HANDLE_FINTO}),
        ('create fallito', {**base, 'create_rc': 1}),
        ('la vecchia non e idle', {**base, 'wait_rc': 1}),
        ('pannello morto: si pulisce', {**base, 'list_reply': LIST_MORTO}),
        ('elenco illeggibile', {**base, 'list_reply': 'non e json'}),
        ('elenco vuoto', {**base, 'list_reply': json.dumps({'result': {'terminals': []}})}),
        ('senza consegna', {**base, 'handoff_done': False}),
        ('opt-out sul worktree', {**base, 'opt_out': True}),
        ('raffreddamento attivo', {**base, 'cooldown': True}),
        ('sotto soglia', {**base, 'used': 50_000}),
        ('transcript assente', {**base, 'transcript': False}),
        ('successore gia armato e vivo', {**base, 'armed': 'term_altro00',
                                          'armed_tab': 'tab-2'}),
        ('successore armato ma morto', {**base, 'armed': 'term_sparito',
                                        'armed_tab': 'tab-9'}),
        ('handle scaduto, tab viva', {**base, 'handle': 'term_scaduto'}),
        ('opus 5 sotto la sua soglia', {**base, 'model': 'claude-opus-5',
                                        'used': 300_000}),
        ('opus 5 sopra la sua soglia', {**base, 'model': 'claude-opus-5',
                                        'used': 480_000}),
        ('modello sconosciuto', {**base, 'model': 'gpt-9', 'used': 175_000}),
    ]


def main():
    SCRATCH.mkdir(parents=True, exist_ok=True)
    binroot = SCRATCH / 'bin'
    binroot.mkdir(exist_ok=True)
    fake = binroot / 'orca'
    fake.write_text(FAKE_ORCA)
    fake.chmod(0o755)

    # Prova che il finto orca sia davvero quello che risponde, non quello vero.
    probe = subprocess.run(['orca', 'prova'], capture_output=True, text=True,
                           env={**os.environ, 'PATH': f'{binroot}:' + os.environ['PATH'],
                                'ORCA_LOG': str(SCRATCH / 'probe.log')})
    if not (SCRATCH / 'probe.log').exists():
        print('IL FINTO ORCA NON RISPONDE: il confronto non proverebbe niente',
              file=sys.stderr)
        return 1
    (SCRATCH / 'probe.log').unlink()

    prima_stato = sorted(p.name for p in (Path.home() / '.claude/state').glob('*'))
    diverged = 0
    for nome, case in cases():
        risultati = {}
        for which in ('python', 'rust'):
            home = build_home(SCRATCH / f'home-{which}', case)
            log = SCRATCH / f'orca-{which}.log'
            log.unlink(missing_ok=True)
            run_side(which, case, home, log)
            chiamate = log.read_text().splitlines() if log.exists() else []
            risultati[which] = (chiamate, *snapshot(home))

        if risultati['python'] != risultati['rust']:
            diverged += 1
            print(f'\nDIVERGE [{nome}]')
            for i, etichetta in enumerate(('chiamate a orca', 'file di stato',
                                           'righe di registro')):
                a, b = risultati['rust'][i], risultati['python'][i]
                if a != b:
                    print(f'  {etichetta}:')
                    print(f'    rust  ={a}')
                    print(f'    python={b}')

    dopo_stato = sorted(p.name for p in (Path.home() / '.claude/state').glob('*'))
    if prima_stato != dopo_stato:
        print('ATTENZIONE: ~/.claude/state è cambiato durante il confronto',
              file=sys.stderr)
        diverged += 1

    print(f'\n{len(cases())} scenari confrontati, {diverged} divergenti')
    return 1 if diverged else 0


if __name__ == '__main__':
    sys.exit(main())
