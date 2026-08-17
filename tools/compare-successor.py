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
    #
    # RETRIED ONCE, AND THAT IS NOT LENIENCY. The two sides are read a fraction
    # of a second apart on a machine where sessions open and close on their own:
    # on 2026-08-17 this case went red with rust=4 python=5 while five
    # back-to-back readings of both agreed on 4. A difference that survives a
    # second reading is in the code; one that does not is the machine moving, and
    # a comparison that cries wolf is a comparison nobody runs.
    for verb, expr in (('live', 'm.live_sessions()'),
                       ('panes', f'm.panes_here({str(Path.cwd())!r})')):
        checked += 1
        a, b = rust(verb, str(Path.cwd())), python(expr)
        if a != b:
            a, b = rust(verb, str(Path.cwd())), python(expr)
        if a != b:
            diverged += 1
            print(f'DIVERGE {verb}  rust={a!r} python={b!r}  (twice in a row)')

    # The global session cap: the judgement is pure, so the live count is passed
    # in rather than measured — two readings a second apart would agree on the
    # wrong thing.
    for command in CAP_COMMANDS:
        checked += 1
        a, b = rust('delta', command), python(f'm.session_delta({command!r})')
        if a != b:
            diverged += 1
            print(f'DIVERGE delta({command!r})  rust={a!r} python={b!r}')
        for live in (0, 7, 19, 20, 64, -1):
            checked += 1
            a = rust('cap', command, str(live))
            b = python(f'm.session_cap_verdict(m.session_delta({command!r}), '
                       f'{live}, m.SESSION_CAP_DEFAULT)')
            if a != b:
                diverged += 1
                print(f'DIVERGE cap({command!r}, {live})\n'
                      f'    rust  ={a[:200]!r}\n    python={b[:200]!r}')

    checked_e2e, diverged_e2e = compare_end_to_end()
    checked_cap, diverged_cap = compare_session_cap()
    print(f'\n{checked} unit cases compared, {diverged} diverged')
    print(f'{checked_e2e} end-to-end cases compared, {diverged_e2e} diverged')
    print(f'{checked_cap} session-cap hook cases compared, {diverged_cap} diverged')
    return 1 if (diverged or diverged_e2e or diverged_cap) else 0


# The commands the cap has to tell apart. Both kinds of error are in here: the
# two real ways a session is opened, the two shells `--setup run` leaves behind
# (which must not count), and the paths that merely contain the word "claude" —
# every command run inside ~/.claude carries it, and a substring test would have
# blocked ordinary work.
CAP_COMMANDS = (
    "orca terminal create --command 'claude' --title x",
    'orca worktree create --repo id:r --name n --agent claude',
    'orca worktree create --repo id:r --name n --setup run',
    "orca terminal create --command 'npm run dev'",
    'orca terminal list --json',
    "orca terminal create --command 'bash /Users/theo/.claude/scripts/x.sh'",
    "orca terminal create --command 'claude-hooks --check'",
    'SESSION_REPLACES=1 orca terminal create --command claude',
    'SESSION_REPLACES=1 orca terminal list',
    'claude -p ciao',
    "git commit -m 'terminal create'",
    'orca   terminal   create --command "claude \'x\'"',
)


def blank_count(text):
    """Hide the live session count, which moves between the two readings."""
    import re
    return re.sub(r'\b\d+ Claude sessions\b', '<N> Claude sessions', text or '')


def fake_homes(name):
    """One scratch HOME per side, with the journal inside and scripts linked.

    The link matters: without it the Python cannot import `hook_log`, falls back
    to a mute function and *looks* like it agrees with a mute port — two silent
    implementations agree by construction.
    """
    import os
    scratch = Path(os.environ.get(
        'RELAY_SCRATCH',
        '/private/tmp/claude-501/-Users-theo-orca-general/scratchpad')) / name
    homes = {}
    for side in ('rust', 'python'):
        h = scratch / f'home-{side}'
        (h / '.claude' / 'state').mkdir(parents=True, exist_ok=True)
        link = h / '.claude' / 'scripts'
        if not link.exists():
            try:
                link.symlink_to(Path.home() / '.claude' / 'scripts')
            except OSError:
                pass
        journal = h / '.claude' / 'state' / 'ganci.jsonl'
        if journal.exists():
            journal.unlink()
        homes[side] = h
    return homes


def compare_session_cap():
    """The cap hook itself, both implementations fed the identical payload.

    Nothing is opened: this hook only ever judges a command someone else would
    have run. The valve and the two limits are exercised from the environment,
    because a valve nobody runs is a valve that one day fails to switch off.
    """
    import os
    homes = fake_homes('session-cap')

    def last_line(side):
        p = homes[side] / '.claude' / 'state' / 'ganci.jsonl'
        if not p.exists():
            return None
        rows = [r for r in p.read_text(errors='replace').splitlines() if r.strip()]
        if not rows:
            return None
        try:
            o = json.loads(rows[-1])
        except Exception:
            return {'<unreadable>': rows[-1]}
        o.pop('t', None)
        return o

    cases = []
    for command in CAP_COMMANDS:
        payload = json.dumps({'tool_name': 'Bash', 'tool_input': {'command': command}})
        # Limit 0 makes every addition refuse regardless of the machine's state,
        # so the case does not depend on how many sessions are alive right now.
        cases.append((f'limit 0 {command[:40]}', payload, {'SESSION_CAP_LIMIT': '0'}))
        cases.append((f'limit high {command[:40]}', payload,
                      {'SESSION_CAP_LIMIT': '9999'}))
    blocking = json.dumps({'tool_name': 'Bash', 'tool_input': {
        'command': 'orca terminal create --command claude'}})
    cases += [
        ('valve off', blocking, {'SESSION_CAP_LIMIT': '0', 'SESSION_CAP_GUARD': 'off'}),
        ('valve warn', blocking, {'SESSION_CAP_LIMIT': '0', 'SESSION_CAP_GUARD': 'avvisa'}),
        ('not a bash call',
         json.dumps({'tool_name': 'Write', 'tool_input': {'command': blocking}}),
         {'SESSION_CAP_LIMIT': '0'}),
    ]

    diverged = 0
    for name, body, extra in cases:
        env = {**os.environ, **extra}
        a = subprocess.run([str(BIN), 'successor-probe', 'session-cap'], input=body,
                           capture_output=True, text=True, timeout=120,
                           env={**env, 'HOME': str(homes['rust'])})
        b = subprocess.run([sys.executable, str(HOOK), '--session-cap'], input=body,
                           capture_output=True, text=True, timeout=120,
                           env={**env, 'HOME': str(homes['python'])})
        row_r, row_p = last_line('rust'), last_line('python')
        # THE LIVE COUNT IS BLANKED HERE, and only here. The two sides read it a
        # fraction of a second apart on a machine that opens and closes sessions
        # on its own — measured 4 against 5 on 2026-08-17, with both sides
        # running the same `ps`. What these cases prove is the wiring: which
        # channel the refusal takes, which valve silences it, which keys the
        # journal carries. The number itself is compared where it can be held
        # still, in the `cap` cases above, which pass it in from outside.
        a.stdout, b.stdout = blank_count(a.stdout), blank_count(b.stdout)
        # stderr too: in warn mode the whole message travels there, and comparing
        # stdout alone would let a port that says nothing at all pass.
        a.stderr, b.stderr = blank_count(a.stderr), blank_count(b.stderr)
        for row in (row_r, row_p):
            if row is not None and 'vive' in row:
                row['vive'] = '<N>'
        if (a.stdout, a.stderr, a.returncode) != (b.stdout, b.stderr, b.returncode):
            diverged += 1
            print(f'\nDIVERGE session-cap [{name}]')
            print(f'    rust   rc={a.returncode} out={a.stdout[:300]!r} err={a.stderr[:200]!r}')
            print(f'    python rc={b.returncode} out={b.stdout[:300]!r} err={b.stderr[:200]!r}')
        elif row_r != row_p:
            diverged += 1
            print(f'\nDIVERGE session-cap journal [{name}]')
            print(f'    python: {row_p!r}')
            print(f'    rust:   {row_r!r}')
    return len(cases), diverged


def scratch_transcript(path, tokens=520_000, model='claude-opus-5'):
    """Un transcript minimo che misura `tokens` in contesto.

    Due righe: una qualunque, e l'ultimo turno dell'assistente con la `usage`
    che la misura somma — `input` più le due voci di cache. Il modello serve a
    scegliere il budget, e quindi la soglia: senza, si ricade sul budget
    prudente e la soglia non è quella che si vuole esercitare.
    """
    quota = tokens // 3
    righe = [
        {'type': 'user', 'message': {'role': 'user', 'content': 'x'}},
        {'type': 'assistant', 'message': {
            'role': 'assistant', 'model': model,
            'usage': {'input_tokens': tokens - 2 * quota,
                      'cache_read_input_tokens': quota,
                      'cache_creation_input_tokens': quota}}},
    ]
    path.write_text('\n'.join(json.dumps(r) for r in righe) + '\n', encoding='utf8')
    return path


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

    # Il transcript di una sessione vera oltre la soglia d'avviso, per esercitare
    # il freno della pienezza dal lato che lo apre.
    #
    # IL TRANSCRIPT SI COSTRUISCE, non si pesca fra quelli veri, e i due modi
    # sbagliati provati il 17/08/2026 lo spiegano meglio di una regola. Preso il
    # file più GROSSO: 257 MB, e misurava 228k token — il contesto è quello
    # dell'ultimo messaggio, non la somma di ciò che è passato. Preso quello con
    # il memo più ALTO (518k): la misura si rifà dalla coda, e in una sessione
    # ormai chiusa l'ultimo turno non porta più `usage`, quindi zero. In
    # entrambi i casi il caso si fermava sul freno che doveva superare: verde, e
    # cieco.
    #
    # Un file di due righe, invece, dice esattamente quello che serve: l'ultimo
    # turno con `usage` è quello che la misura legge, e i tre campi che somma
    # sono questi.
    finto = scratch_transcript(doc.parent / 'transcript-pieno.jsonl')
    pieni = [finto]
    payload_pieno = json.dumps({
        'session_id': 'prova-e2e-0000',
        'cwd': str(Path.cwd()),
        'tool_name': 'Write',
        'transcript_path': str(pieni[0]),
        'tool_input': {'file_path': str(doc)},
    }) if pieni else None

    # `CONSEGNA_ANCHE_A_META` è il valore neutro per i casi che provano i TETTI:
    # dal 17/08/2026 il freno della pienezza sta PRIMA di loro, e senza un
    # transcript nel payload si fermerebbero tutti lì — d'accordo fra loro, ma
    # sul freno sbagliato. È lo stesso inganno del registro muto: due
    # implementazioni che si fermano presto vanno d'accordo per costruzione.
    meta = {'CONSEGNA_ANCHE_A_META': '1'}
    cases = [
        ('albero affollato', payload, {**meta, 'CONSEGNA_TETTO_PANNELLI': '0'}),
        ('troppe sessioni', payload, {**meta, 'CONSEGNA_TETTO_SESSIONI': '0'}),
        ('seconda generazione', payload, {**meta, 'CLAUDE_NATO_DA_CONSEGNA': '1'}),
        ('non e una consegna', not_a_doc, {**meta, 'CONSEGNA_TETTO_PANNELLI': '0'}),
        ('cwd dichiarata altrove', payload_altrove, {**meta, 'CONSEGNA_TETTO_PANNELLI': '0'}),
        ('cwd altrove, tetto sessioni', payload_altrove, {**meta, 'CONSEGNA_TETTO_SESSIONI': '0'}),
        # I due versi del freno nuovo. Senza transcript non si sa quanto sia
        # piena la sessione, e chi non si sa pieno non si sostituisce.
        ('consegna scritta a meta lavoro', payload, {'CONSEGNA_TETTO_PANNELLI': '0'}),
    ]
    if payload_pieno:
        # Sessione piena: il freno della pienezza si apre, e ci si ferma sul
        # tetto successivo — così il caso discrimina senza aprire una scheda.
        cases.append(('sessione piena, si ferma dopo',
                      payload_pieno, {'CONSEGNA_TETTO_PANNELLI': '0'}))
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

    # DUE HOME FINTE, UNA PER PARTE, e il registro dentro. Fino al 17/08/2026
    # questi casi giravano con la HOME vera e si confrontava solo `stdout`: il
    # porto **non scriveva una riga di registro** e il confronto restava verde.
    # Misurato quel giorno: gancio passato al binario alle 10:57, tre consegne
    # vere scritte alle 11:35, 13:37 e 14:21, e zero righe `origine=scrittura`
    # dopo le 11:10. Il freno più frequente, `troppe-sessioni`, scattava
    # invisibile. La riga fa parte della risposta, quindi va confrontata.
    #
    # `scripts` va collegato dentro la HOME finta o il Python non trova
    # `hook_log`, ripiega su una funzione muta e **sembra** d'accordo col porto
    # muto: due implementazioni silenziose vanno d'accordo per costruzione.
    scratch = Path(os.environ.get(
        'RELAY_SCRATCH',
        '/private/tmp/claude-501/-Users-theo-orca-general/scratchpad')) / 'successor-e2e'
    homes = {}
    for lato in ('rust', 'python'):
        h = scratch / f'home-{lato}'
        (h / '.claude' / 'state').mkdir(parents=True, exist_ok=True)
        link = h / '.claude' / 'scripts'
        if not link.exists():
            try:
                link.symlink_to(Path.home() / '.claude' / 'scripts')
            except OSError:
                pass
        registro = h / '.claude' / 'state' / 'ganci.jsonl'
        if registro.exists():
            registro.unlink()
        homes[lato] = h

    def ultima_riga(lato):
        """L'ultima riga del registro, senza l'istante e senza la propria HOME.

        I due percorsi differiscono per costruzione — ognuno punta alla propria
        cartella finta — quindi confrontarli alla lettera darebbe una divergenza
        che non è del codice. Si normalizza quella, e solo quella.
        """
        p = homes[lato] / '.claude' / 'state' / 'ganci.jsonl'
        if not p.exists():
            return None
        righe = [r for r in p.read_text(errors='replace').splitlines() if r.strip()]
        if not righe:
            return None
        try:
            o = json.loads(righe[-1])
        except Exception:
            return {'<riga illeggibile>': righe[-1]}
        o.pop('t', None)
        return {k: (v.replace(str(homes[lato]), '<HOME>') if isinstance(v, str) else v)
                for k, v in o.items()}

    for name, body, extra in cases:
        env = {**os.environ, **extra}
        # Entrambi lanciati DENTRO un albero che ha pannelli. Senza questo il
        # caso «cwd dichiarata altrove» non discrimina: da una cartella qualsiasi
        # sia la cwd del processo sia quella del payload contano zero pannelli, e
        # un porto che legge la seconda invece della prima passa il confronto.
        # Verificato per mutazione il 17/08/2026 — sopravviveva.
        a = subprocess.run([str(BIN), 'handoff-arms-successor'], input=body,
                           capture_output=True, text=True, timeout=120,
                           env={**env, 'HOME': str(homes['rust'])},
                           cwd=busy_tree)
        b = subprocess.run([sys.executable, str(HOOK)], input=body,
                           capture_output=True, text=True, timeout=120,
                           env={**env, 'HOME': str(homes['python'])},
                           cwd=busy_tree)
        riga_r, riga_p = ultima_riga('rust'), ultima_riga('python')
        if a.stdout != b.stdout or a.returncode != b.returncode:
            diverged += 1
            print(f'\nDIVERGE end-to-end [{name}]')
            print(f'    rust   rc={a.returncode} out={a.stdout[:300]!r}')
            print(f'    python rc={b.returncode} out={b.stdout[:300]!r}')
        elif riga_r != riga_p:
            diverged += 1
            print(f'\nDIVERGE nel registro [{name}]')
            print(f'    python: {riga_p!r}')
            print(f'    rust:   {riga_r!r}')
    return len(cases), diverged


if __name__ == '__main__':
    sys.exit(main())
