#!/usr/bin/env python3
"""Confronta il gancio Linear in Rust con l'originale in Python, caso per caso.

L'ORIGINALE È L'ORACOLO. Finché non lo cancelliamo, l'esito atteso lo produce
lui: se i due divergono il difetto è del nuovo fino a prova contraria — ed è
stato vero tre volte su quattro nelle sette adozioni precedenti.

I CASI NON SI RICOPIANO. Vengono presi a runtime da
`prova-linear-sola-lettura.py`, che è la batteria già scritta e già verde:
173 casi fra letture da lasciar passare, scritture da negare, valvole vere e
finte, ingressi malformati e strumenti MCP. Ricopiarli qui creerebbe un secondo
elenco da tenere allineato, e il primo a divergere sarebbe questo.

SI CONFRONTA CIÒ CHE IL GANCIO PRODUCE, non solo cosa decide: codice di uscita,
stdout (che per questo gancio è il rifiuto vero e proprio), stderr, **e la riga
scritta nel registro**, tipi compresi. Il campo `quando` è l'unico escluso,
perché cambia a ogni esecuzione.

LO STATO È FINTO. `HOME` punta a una cartella temporanea con dentro una copia
dei due file del gancio: il Python li riconosce come «propri» da `__file__`, il
Rust da `HOME`, e senza la copia i due guarderebbero cose diverse. È la stessa
trappola che ha reso verde per il motivo sbagliato il confronto del gate
SocratiCode.

    python3 tools/compare-linear-readonly.py
"""
import importlib.util
import json
import os
import shutil
import subprocess
import sys
import tempfile
import time

HOOKS = os.path.expanduser('~/.claude/skills/hooks')
PYTHON_HOOK = os.path.join(HOOKS, 'linear-sola-lettura.py')
BATTERY = os.path.join(HOOKS, 'prova-linear-sola-lettura.py')
BINARY = os.path.expanduser('~/.claude/rust/target/release/claude-hooks')


def load_battery():
    """Le liste di casi della batteria, senza eseguirla."""
    spec = importlib.util.spec_from_file_location('battery', BATTERY)
    module = importlib.util.module_from_spec(spec)
    spec.loader.exec_module(module)
    return module


def fake_home(root):
    """Una HOME con dentro quel tanto che serve al gancio per riconoscersi."""
    hooks = os.path.join(root, '.claude', 'skills', 'hooks')
    state = os.path.join(root, '.claude', 'state')
    os.makedirs(hooks, exist_ok=True)
    os.makedirs(state, exist_ok=True)
    for name in ('linear-sola-lettura.py', 'prova-linear-sola-lettura.py'):
        shutil.copy2(os.path.join(HOOKS, name), os.path.join(hooks, name))
    with open(os.path.join(root, '.claude', 'settings.json'), 'w') as f:
        f.write('{"hooks": {}}\n')
    return os.path.join(hooks, 'linear-sola-lettura.py')


def run_one(command, tool, tool_input, home, hook_copy, register, permission,
            raw=None):
    """Esegue Python e Rust sullo stesso ingresso e restituisce i due esiti.

    `raw` serve agli ingressi malformati, che sono JSON interi senza la forma
    canonica: lì il punto è proprio che manca qualcosa."""
    data = raw if raw is not None else {
        'tool_name': tool, 'session_id': 'confronto', 'cwd': '/tmp',
        'tool_input': tool_input if tool_input is not None
        else {'command': command}}
    payload = json.dumps(data)

    path = os.path.join(home, '.claude', 'state', 'linear-permesso.json')
    if permission is None:
        if os.path.exists(path):
            os.remove(path)
    else:
        with open(path, 'w') as f:
            json.dump(permission, f)

    out = []
    for argv in ([sys.executable, hook_copy], [BINARY, 'linear-readonly']):
        if os.path.exists(register):
            os.remove(register)
        env = dict(os.environ, HOME=home, LINEAR_GANCIO_REGISTRO=register)
        r = subprocess.run(argv, input=payload, capture_output=True,
                           text=True, env=env)
        rows = []
        if os.path.exists(register):
            with open(register) as f:
                for line in f:
                    if line.strip():
                        row = json.loads(line)
                        row.pop('quando', None)
                        rows.append(row)
        out.append({'uscita': r.returncode, 'stdout': r.stdout.strip(),
                    'stderr': r.stderr.strip(), 'registro': rows})
    return out


def cases(battery):
    """Ogni caso: (etichetta, comando, strumento, ingresso, permesso, grezzo)."""
    for command in battery.LETTURE:
        yield ('lettura', command, 'Bash', None, None, None)
    for command in battery.SCRITTURE:
        yield ('scrittura', command, 'Bash', None, None, None)
    for command in battery.VALVOLA_VERA:
        yield ('valvola-vera', command, 'Bash', None, None, None)
    for command in battery.VALVOLA_FINTA:
        yield ('valvola-finta', command, 'Bash', None, None, None)
    for data in battery.MALFORMATI:
        yield ('malformato', None, None, None, None, data)
    for name in battery.MCP_NEGATI + battery.MCP_PASSANO:
        yield ('mcp-nome', None, name, {'query': 'x'}, None, None)
    for tool, tool_input, _atteso in battery.MCP_CONTENT:
        yield ('mcp-contenuto', None, tool, tool_input, None, None)

    # Gli strumenti che scrivono un file senza passare dalla shell: il gancio
    # guardava solo `Bash`, e `Write` sul gancio stesso lo disarmava in una riga.
    for name in ('linear-sola-lettura.py', 'settings.json', 'altro.py'):
        for tool in ('Write', 'Edit', 'NotebookEdit'):
            key = 'notebook_path' if tool == 'NotebookEdit' else 'file_path'
            yield ('file', None, tool,
                   {key: f'/Users/theo/.claude/skills/hooks/{name}',
                    'content': 'x'}, None, None)

    # Le LETTURE dei file protetti. Il corpus non ne aveva nessuna, e per questo
    # il difetto è vissuto a lungo: `sed` stava fra i segni di scrittura come
    # parola nuda, quindi `sed -n '5,10p'` sul gancio era negato mentre `grep`
    # sullo stesso file passava. Il confronto taceva perché non gliel'aveva mai
    # chiesto — un corpus senza il caso è cieco quanto un gate spento.
    hook = '~/.claude/skills/hooks/linear-sola-lettura.py'
    for command in (f"sed -n '5,10p' {hook}",
                    f'sed -n "1,40p" {hook}',
                    f'grep -n deny {hook}',
                    f'head -20 {hook}'):
        yield ('lettura-file-protetto', command, 'Bash', None, None, None)
    # E la direzione opposta, o la riga sopra basterebbe a far passare un porto
    # che ha smesso di guardare `sed` del tutto.
    for command in (f'sed -i "" "s/deny/allow/" {hook}',
                    f'sed --in-place "s/a/b/" {hook}'):
        yield ('scrittura-sed-in-place', command, 'Bash', None, None, None)

    # La valvola sul file di configurazione, che è l'unico posto dove vale.
    # I casi `VALVOLA_FINTA` della batteria riguardano tutti scritture su Linear,
    # dove `OK_UTENTE=1` non viene nemmeno consultata: senza queste righe, un
    # gancio che cercasse di nuovo la valvola come stringa — la falla chiusa il
    # 29/07/2026 — passerebbe il confronto senza che nessun caso se ne accorga.
    # Trovato da `tools/mutants.sh`, non a ragionamento.
    config = '~/.claude/settings.json'
    yield ('valvola-config-vera', f'OK_UTENTE=1 cp /tmp/x.json {config}',
           'Bash', None, None, None)
    yield ('valvola-config-vera', f'OK_UTENTE=1 rm {config}.bak',
           'Bash', None, None, None)
    yield ('valvola-config-finta', f'echo "usa OK_UTENTE=1" > {config}',
           'Bash', None, None, None)
    yield ('valvola-config-finta', f'cp /tmp/x.json {config} # OK_UTENTE=1',
           'Bash', None, None, None)
    yield ('valvola-config-finta',
           f'echo OK_UTENTE=1 | tee {config}', 'Bash', None, None, None)

    # Il permesso di Theo, nei suoi quattro stati. Non stanno nelle liste della
    # batteria perché lì li costruisce `tests_on_permission` manipolando il file.
    future = time.time() + 600
    past = time.time() - 600
    scritture = {'ambito': 'scritture', 'scade': future, 'schede': []}
    done_hrd = {'ambito': 'done', 'scade': future, 'schede': ['HRD-1']}
    scaduto = {'ambito': 'done', 'scade': past, 'schede': ['HRD-1']}
    modifica = 'orca linear ' + 'comment ' + 'add HRD-1 --body x'
    chiusura = 'orca linear ' + 'status ' + 'set HRD-1 --status Done'
    altra = 'orca linear ' + 'status ' + 'set HRD-99 --status Done'
    for permission, label in ((None, 'assente'), (scritture, 'scritture'),
                              (done_hrd, 'done'), (scaduto, 'scaduto')):
        for command in (modifica, chiusura, altra):
            yield (f'permesso-{label}', command, 'Bash', None, permission,
                   None)
    # Con il permesso valido, `Write` sulla configurazione passa senza valvola.
    yield ('permesso-file', None, 'Write',
           {'file_path': '/x/settings.json', 'content': 'y'}, scritture, None)


def main():
    if not os.path.exists(BINARY):
        sys.exit(f'manca il binario: {BINARY}\n  cargo build --release')

    battery = load_battery()
    root = tempfile.mkdtemp(prefix='confronto-linear-')
    hook_copy = fake_home(root)
    register = os.path.join(root, 'registro.jsonl')

    total = 0
    divergent = []
    for label, command, tool, tool_input, permission, raw in cases(battery):
        total += 1
        python, rust = run_one(command, tool, tool_input, root, hook_copy,
                               register, permission, raw)
        if python != rust:
            divergent.append((label, command or json.dumps(tool_input or raw),
                              python, rust))

    shutil.rmtree(root, ignore_errors=True)

    print(f'{total} casi confrontati, {len(divergent)} divergenze')
    for label, what, python, rust in divergent[:20]:
        print(f'\n── {label}: {what[:150]}')
        for key in ('uscita', 'stdout', 'stderr', 'registro'):
            if python[key] != rust[key]:
                print(f'   {key}:')
                print(f'     python: {str(python[key])[:400]}')
                print(f'     rust:   {str(rust[key])[:400]}')
    if len(divergent) > 20:
        print(f'\n… e altre {len(divergent) - 20}')
    return 1 if divergent else 0


if __name__ == '__main__':
    sys.exit(main())
