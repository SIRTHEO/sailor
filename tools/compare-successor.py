#!/usr/bin/env python3
"""Compare the Rust successor-arming logic against handoff-arms-successor.py.

The oracle is the Python hook running on this machine today. What is compared:

    doc          is this a handoff document?      every .md under projects/
    hours        is this hour inside the window?   all 24
    fingerprint  the marker key                    real paths x several sessions
    mandate      the text the successor receives   byte for byte
    agents       panes with an agent in a tree     the live Orca list
    panes/live   the two counts the caps consume   the machine as it is now

WHY EVERY MARKDOWN. is_handoff_doc is the decision that opens a tab, and its
history is one of both kinds of error: it once answered "not a handoff" on a real
handoff (blind while switched on), and once armed a tab on a tool description
that merely carried type: project. Neither was found by invented cases — they
were found by real files, so real files are what it runs on.

Nothing is written and nothing is opened: every probe here is a question.
"""
import json
import subprocess
import sys
from pathlib import Path

BIN = Path.home() / '.claude/rust/target/release/claude-hooks'
HOOK = Path.home() / '.claude/skills/hooks/handoff-arms-successor.py'
PROJECTS = Path.home() / '.claude/projects'


def rust(verb, a='', b='', stdin=''):
    r = subprocess.run([str(BIN), 'successor-probe', verb, a, b],
                       input=stdin, capture_output=True, text=True, timeout=120)
    return r.stdout.rstrip('\n')


def python(expr, stdin=''):
    """Evaluate an expression against the hook, loaded by path (its name has a dash)."""
    code = (
        'import importlib.util, sys, json;'
        f'spec = importlib.util.spec_from_file_location("arms", {str(HOOK)!r});'
        'm = importlib.util.module_from_spec(spec); spec.loader.exec_module(m);'
        f'print({expr}, end="")'
    )
    r = subprocess.run([sys.executable, '-c', code], input=stdin,
                       capture_output=True, text=True, timeout=120)
    if r.returncode != 0:
        return 'ERRORE: ' + (r.stderr or '')[-160:]
    return r.stdout.rstrip('\n')


def markdowns():
    """Every markdown under projects/, which is where handoffs actually live."""
    return sorted(PROJECTS.glob('*/memory/*.md')) + sorted(PROJECTS.glob('*/*.md'))


def main():
    diverged = 0
    checked = 0

    docs = markdowns()
    if not docs:
        print('no markdown found under projects/: the doc test proves nothing',
              file=sys.stderr)
    for f in docs:
        checked += 1
        a, b = rust('doc', str(f)), python(f'm.is_handoff_doc({str(f)!r})')
        if a != b:
            diverged += 1
            print(f'DIVERGE doc  {f.name}\n    rust={a!r} python={b!r}')

    for h in range(24):
        checked += 1
        a, b = rust('hours', str(h)), python(f'm.within_hours({h})')
        if a != b:
            diverged += 1
            print(f'DIVERGE hours({h})  rust={a!r} python={b!r}')

    # The key that decides whether a second document in the same turn arms a
    # second tab. Empty session included: it is the strict fallback.
    for path in [str(f) for f in docs[:8]] + ['/x/consegna.md']:
        for sess in ('', 'sessione-A', 'cdca7b36-bd04-4645-b291-ecedad59cbb7'):
            checked += 1
            a = rust('fingerprint', path, sess)
            b = python(f'm.armed_marker({path!r}, {sess!r}).name.replace("successore-armato-","")')
            if a != b:
                diverged += 1
                print(f'DIVERGE fingerprint({path!r}, {sess!r})\n    rust={a!r} python={b!r}')

    for path in ('/x/consegna.md', '/p/memory/note-17-08-2026.md'):
        checked += 1
        a, b = rust('mandate', path), python(f'm.mandate({path!r})')
        if a != b:
            diverged += 1
            print(f'DIVERGE mandate({path!r})')
            print(f'    rust    ={a[:200]!r}')
            print(f'    python  ={b[:200]!r}')

    # The live Orca list, judged by both from the same bytes.
    r = subprocess.run(['orca', 'terminal', 'list', '--json'],
                       capture_output=True, text=True, timeout=60)
    raw = r.stdout or ''
    roots = set()
    try:
        d = json.loads(raw)
        items = d.get('result', d)
        if isinstance(items, dict):
            items = items.get('terminals') or []
        roots = {t.get('worktreePath', '') for t in items}
    except Exception:
        pass
    roots.add('/nessun-albero')
    # La lista viva da sola non basta: oggi ogni pannello ha un marcatore, quindi
    # un porto che ignorasse del tutto i marcatori passerebbe il confronto —
    # verificato per mutazione il 17/08/2026, zero divergenze. Le shell che
    # `--setup run` lascia sono il caso che conta, e vanno fabbricate perché in
    # questo momento sulla macchina non ce ne sono.
    fabbricata = json.dumps({'result': {'terminals': [
        {'worktreePath': '/finto', 'title': 'Setup'},
        {'worktreePath': '/finto', 'title': 'Terminal 1'},
        {'worktreePath': '/finto', 'title': '✳ Claude Code'},
        {'worktreePath': '/finto', 'title': '◑ Consegna in corso'},
        {'worktreePath': '/finto', 'title': ''},
        {'worktreePath': '/altro-finto', 'title': '◐ altrove'},
    ]}})
    for root in ('/finto', '/altro-finto', '/mai-visto'):
        checked += 1
        a = rust('agents', root, stdin=fabbricata)
        b = python(
            'm.count_agents((lambda d: (d.get("result", d).get("terminals")'
            ' if isinstance(d.get("result", d), dict) else d.get("result", d)) or [])'
            f'(json.loads(sys.stdin.read())), {root!r})',
            stdin=fabbricata)
        if a != b:
            diverged += 1
            print(f'DIVERGE agents fabbricata({root!r})  rust={a!r} python={b!r}')

    for root in sorted(roots):
        checked += 1
        a = rust('agents', root, stdin=raw)
        b = python(
            'm.count_agents((lambda d: (d.get("result", d).get("terminals")'
            ' if isinstance(d.get("result", d), dict) else d.get("result", d)) or [])'
            f'(json.loads(sys.stdin.read())), {root!r})',
            stdin=raw)
        if a != b:
            diverged += 1
            print(f'DIVERGE agents({root!r})  rust={a!r} python={b!r}')

    # The two counts the caps consume, against the machine as it is right now.
    for verb, expr in (('live', 'm.live_sessions()'),
                       ('panes', f'm.panes_here({str(Path.cwd())!r})')):
        checked += 1
        a = rust(verb, str(Path.cwd()))
        b = python(expr)
        if a != b:
            diverged += 1
            print(f'DIVERGE {verb}  rust={a!r} python={b!r}')

    checked_e2e, diverged_e2e = compare_end_to_end()
    print(f'\n{checked} unit cases compared, {diverged} diverged')
    print(f'{checked_e2e} end-to-end cases compared, {diverged_e2e} diverged')
    return 1 if (diverged or diverged_e2e) else 0


def compare_end_to_end():
    """The whole hook, both implementations fed the identical payload.

    Every case here runs with at least one brake shut, so nothing is ever opened.
    That is not caution for its own sake: the one path that opens a tab starts a
    real Claude session thirty seconds later, and a comparison tool that spawns
    sessions is a tool nobody can run twice. The open path is covered by the unit
    cases on decide(), where it costs nothing.
    """
    doc = next((f for f in markdowns()
                if python(f'm.is_handoff_doc({str(f)!r})') == 'True'), None)
    if doc is None:
        print('no real handoff document found: end-to-end not compared',
              file=sys.stderr)
        return 0, 0

    payload = json.dumps({
        'session_id': 'prova-e2e-0000',
        'cwd': str(Path.cwd()),
        'tool_name': 'Write',
        'tool_input': {'file_path': str(doc)},
    })
    # Lo stesso caso con una cwd DICHIARATA diversa da quella del processo. Fino
    # al 17/08/2026 ogni caso qui passava la cwd vera in entrambi i posti, e la
    # divergenza restava invisibile: il porto contava i pannelli dell'albero
    # dichiarato nel payload, il Python quelli del processo. Su questa macchina
    # facevano 0 contro 2.
    payload_altrove = json.dumps({
        'session_id': 'prova-e2e-0000',
        'cwd': '/tmp',
        'tool_name': 'Write',
        'tool_input': {'file_path': str(doc)},
    })
    not_a_doc = json.dumps({
        'session_id': 'prova-e2e-0000',
        'cwd': str(Path.cwd()),
        'tool_name': 'Write',
        'tool_input': {'file_path': '/tmp/qualcosa.txt'},
    })

    cases = [
        ('albero affollato', payload, {'CONSEGNA_TETTO_PANNELLI': '0'}),
        ('troppe sessioni', payload, {'CONSEGNA_TETTO_SESSIONI': '0'}),
        ('seconda generazione', payload, {'CLAUDE_NATO_DA_CONSEGNA': '1'}),
        ('non e una consegna', not_a_doc, {'CONSEGNA_TETTO_PANNELLI': '0'}),
        ('cwd dichiarata altrove', payload_altrove, {'CONSEGNA_TETTO_PANNELLI': '0'}),
        ('cwd altrove, tetto sessioni', payload_altrove, {'CONSEGNA_TETTO_SESSIONI': '0'}),
    ]
    # L'albero con più pannelli fra quelli aperti: è lì che le due letture della
    # cwd danno numeri diversi, e quindi l'unico posto da cui il confronto vede
    # la differenza.
    busy_tree = str(Path.cwd())
    try:
        r = subprocess.run(['orca', 'terminal', 'list', '--json'],
                           capture_output=True, text=True, timeout=60)
        d = json.loads(r.stdout)
        items = d.get('result', d)
        if isinstance(items, dict):
            items = items.get('terminals') or []
        conteggio = {}
        for t in items:
            p = t.get('worktreePath') or ''
            if p and any(m in (t.get('title') or '') for m in ('✳', '◑', '◐', '⏳')):
                conteggio[p] = conteggio.get(p, 0) + 1
        if conteggio:
            busy_tree = max(conteggio, key=conteggio.get)
    except Exception:
        pass
    print(f'end-to-end eseguito da {busy_tree}', file=sys.stderr)

    diverged = 0
    import os
    for name, body, extra in cases:
        env = {**os.environ, **extra}
        # Entrambi lanciati DENTRO un albero che ha pannelli. Senza questo il
        # caso «cwd dichiarata altrove» non discrimina: da una cartella qualsiasi
        # sia la cwd del processo sia quella del payload contano zero pannelli, e
        # un porto che legge la seconda invece della prima passa il confronto.
        # Verificato per mutazione il 17/08/2026 — sopravviveva.
        a = subprocess.run([str(BIN), 'handoff-arms-successor'], input=body,
                           capture_output=True, text=True, timeout=120, env=env,
                           cwd=busy_tree)
        b = subprocess.run([sys.executable, str(HOOK)], input=body,
                           capture_output=True, text=True, timeout=120, env=env,
                           cwd=busy_tree)
        if a.stdout != b.stdout or a.returncode != b.returncode:
            diverged += 1
            print(f'\nDIVERGE end-to-end [{name}]')
            print(f'    rust   rc={a.returncode} out={a.stdout[:300]!r}')
            print(f'    python rc={b.returncode} out={b.stdout[:300]!r}')
    return len(cases), diverged


if __name__ == '__main__':
    sys.exit(main())
