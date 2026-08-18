#!/usr/bin/env python3
"""Confronta il gancio Linear in Rust con l'originale in Python, caso per caso.

L'ORIGINALE È L'ORACOLO. Finché non lo cancelliamo, l'esito atteso lo produce
lui: se i due divergono il difetto è del nuovo fino a prova contraria — ed è
stato vero tre volte su quattro nelle sette adozioni precedenti.

I CASI NON SI RICOPIANO. Vengono presi a runtime da
`prova-linear-sola-lettura.py`, che è la batteria già scritta e già verde:
**182** casi fra letture da lasciar passare, scritture da negare, valvole vere e
finte, ingressi malformati e strumenti MCP (contati il 18/08/2026; la docstring
ne dichiarava 173, e nessuno se ne era accorto perché il numero non è di nessun
controllo). Ricopiarli qui creerebbe un secondo elenco da tenere allineato, e il
primo a divergere sarebbe questo. Gli altri li aggiunge `cases()`, per i punti
che la batteria non copre: in tutto 217.

SI CONFRONTA CIÒ CHE IL GANCIO PRODUCE, non solo cosa decide: codice di uscita,
stdout (che per questo gancio è il rifiuto vero e proprio), stderr, **e la riga
scritta nel registro**, tipi compresi. Il campo `quando` è l'unico escluso,
perché cambia a ogni esecuzione.

LO STATO È FINTO. `HOME` punta a una cartella temporanea con dentro una copia
dei **tre** file del gancio — i due di `skills/hooks/` e questo strumento: il
Python li riconosce come «propri» da `__file__`, il Rust da `HOME`, e senza la
copia i due guarderebbero cose diverse. È la stessa trappola che ha reso verde
per il motivo sbagliato il confronto del gate SocratiCode.

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

# I nomi del nucleo restano scritti per intero, qui e più sotto, anche se è
# esattamente ciò che fa negare l'esecuzione di questo file dal gancio che
# prova. Comporli a pezzi — `'linear-sola-' + 'lettura.py'` — lo farebbe
# passare, ma non perché il file è innocuo: perché l'euristica non lo
# riconoscerebbe più. Domani chi ci mettesse dentro un `os.replace` sul gancio
# passerebbe per la stessa porta. La deroga si chiede, non si prende di
# nascosto: sta in `own_files()` del gemello Rust, e ha per prezzo il posto in
# `CORE_FILES` — esentato dalla lettura, e in cambio non più scrivibile da
# dentro una sessione.


def load_battery():
    """Le liste di casi della batteria, senza eseguirla."""
    spec = importlib.util.spec_from_file_location('battery', BATTERY)
    module = importlib.util.module_from_spec(spec)
    spec.loader.exec_module(module)
    return module


def own_tool_in(root):
    """Dove il gancio si aspetta di trovare questo strumento, nella HOME finta.

    `own_files()` del Rust lo cerca sotto `$HOME`, quindi senza una copia qui
    la deroga non verrebbe esercitata da nessun caso e resterebbe non provata.
    """
    return os.path.join(root, '.claude', 'rust', 'tools',
                        os.path.basename(os.path.abspath(__file__)))


def fake_home(root):
    """Una HOME con dentro quel tanto che serve al gancio per riconoscersi."""
    hooks = os.path.join(root, '.claude', 'skills', 'hooks')
    state = os.path.join(root, '.claude', 'state')
    mine = own_tool_in(root)
    os.makedirs(os.path.dirname(mine), exist_ok=True)
    shutil.copy2(os.path.abspath(__file__), mine)
    # Se questa copia sparisse, i due casi della deroga diventerebbero identici
    # — nemmeno il Python legge un file che non c'è — e uscirebbero dal conto
    # delle dichiarate senza entrare fra le nuove: uscita 0, e l'unico sintomo
    # un numero che nessuno guarda. È la trappola che ha reso verde per il
    # motivo sbagliato il confronto del gate SocratiCode.
    assert os.path.exists(mine), f'la copia della deroga non è stata creata: {mine}'
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


def cases(battery, root=None):
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

    # L'esecuzione di questo stesso strumento: la deroga che il Rust concede in
    # `own_files()` e il Python no. Senza questo caso la divergenza esiste
    # ugualmente e nessuna prova la guarda — né il corpus, né i test Rust, che
    # non nominano mai questo file.
    if root:
        mine = own_tool_in(root)
        for command in (f'python3 {mine}', f'python3 {mine} 2>&1'):
            yield (OWN_TOOL_LABEL, command, 'Bash', None, None, None)

    # I modi di rimpiazzare il gancio che l'elenco delle parole di scrittura non
    # conosceva: cinque provati il 18/08/2026, uno solo fermato. Il Rust ora li
    # nega tutti, il Python no e non si può correggere da dentro una sessione.
    # Senza questi casi la divergenza resterebbe fuori da ogni prova, ed è la
    # direzione in cui un ritorno indietro non farebbe rumore.
    long_source = ('/Users/theo/orca/workspaces/general/hamlet/rust/crates/'
                   'guards/src/copie/nuovo-gancio-riscritto.py')
    for command in (f'ln -sf /tmp/vuoto.py {hook}',
                    f'ln -f /tmp/vuoto.py {hook}',
                    f'install -m 644 /tmp/vuoto.py {hook}',
                    f'rsync /tmp/vuoto.py {hook}',
                    f'patch {hook} < /tmp/p.diff',
                    f'cp {long_source} {hook}',
                    # Il sorgente Rust: è il gancio vivo da quando il ramo
                    # Python è solo l'oracolo, e restava fuori dal nucleo che
                    # elencava soltanto i file dell'era precedente. Scriverci
                    # dentro e ricompilare disarmava tutto, senza toccare
                    # niente di protetto.
                    'echo x > ~/.claude/rust/crates/guards/src/linear_readonly.rs'):
        yield (STRICTER_LABEL, command, 'Bash', None, None, None)

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


def decision(out):
    """Il verdetto dentro stdout: `deny`, `allow`, o `muto` se non parla."""
    text = out['stdout'].strip()
    if not text:
        return 'muto'
    try:
        payload = json.loads(text)
    except json.JSONDecodeError:
        return 'illeggibile'
    return payload.get('hookSpecificOutput', {}).get('permissionDecision',
                                                     'assente')


def reason(out):
    """Il motivo dentro stdout, o la stringa vuota."""
    text = out['stdout'].strip()
    if not text:
        return ''
    try:
        payload = json.loads(text)
    except json.JSONDecodeError:
        return ''
    return payload.get('hookSpecificOutput', {}).get(
        'permissionDecisionReason', '')


# Le divergenze che il Rust introduce APPOSTA, misurate il 18/08/2026. Restano
# qui, in elenco chiuso, perché un confronto rosso per sempre non segnala più
# niente: dopo la prima volta nessuno lo guarda, e la divergenza nuova entra in
# mezzo alle ventisei vecchie senza che nessuno la veda.
#
# 1. Il rifiuto su un FILE protetto. Il Python stampa l'omelia sui sottocomandi
#    Linear anche a chi tocca la configurazione; il Rust ha un motivo dedicato
#    (`is_protected_file`, `PROTECTED_FILE_PREFIX`). Stessa decisione, stesso
#    codice di uscita, stesso registro: cambia solo il testo.
# 2. La LETTURA a righe numerate di un file del nucleo. Il Python la nega —
#    `sed` stava fra i segni di scrittura come parola nuda — mentre il suo
#    stesso rifiuto dichiara che «leggerlo resta libero». Il Rust chiede il
#    `-i`. Vale solo per l'etichetta dedicata, non per un `sed` qualunque.
# 3. L'ESECUZIONE di questo stesso strumento. Il suo contenuto copia il gancio
#    in una HOME finta, e quella copia si legge come una riscrittura del
#    nucleo: senza deroga, il gesto che il rifiuto dichiara libero
#    («lanciarne le prove resta libero») era negato, e la parità non si poteva
#    misurare affatto. Il Rust la concede in `own_files()`, pagandola col posto
#    in `CORE_FILES`; il gemello Python no, e non si può correggere da dentro
#    una sessione — il file del gancio è nel nucleo, e il rifiuto rimanda a
#    Theo. Il caso `esecuzione-del-comparatore` la esercita davvero: senza, la
#    divergenza esisterebbe e nessuna prova la guarderebbe.
#
# Le prime due valgono per l'etichetta o per la forma del motivo; la terza per
# l'etichetta. Fuori da qui una divergenza è nuova, e il confronto esce 1.
READ_ONLY_LABEL = 'lettura-file-protetto'
OWN_TOOL_LABEL = 'esecuzione-del-comparatore'
STRICTER_LABEL = 'scrittura-che-il-python-non-vede'

# Quante divergenze dichiarate ci si aspetta: 24 sul testo del rifiuto, 2 sulle
# letture, 2 sull'esecuzione di questo strumento, 7 sulle scritture che il
# Python non vede. Contate il 18/08/2026.
EXPECTED_KNOWN = 35

# Il rifiuto del Rust sui file protetti, per intero e con un buco dove va il
# motivo. Non un prefisso: un modello esatto.
#
# Accettare qualunque testo purché cominci nel modo giusto lascia scoperta tutta
# la parte in mezzo — misurato con un mutante che capovolgeva la frase
# «lanciarne le prove resta libero» in «leggerlo e cercarlo sono vietati quanto
# scriverci», cioè un gancio che dice all'agente l'opposto di quello che fa: il
# confronto restava a «0 divergenze nuove». Il testo di un rifiuto non è
# decorazione, è ciò che decide se chi lo riceve gira intorno al freno.
#
# Se il gancio cambia frase apposta, questa costante va cambiata con lui, e
# l'attrito è voluto: passa da qui la decisione di riscrivere un messaggio.
RUST_REFUSAL = (
    'Questo file regge il divieto di scrittura su Linear (',
    "), e non si modifica dall'interno di una sessione: una valvola che "
    'autorizza il proprio smontaggio non è una valvola. Leggerlo, cercarlo e '
    'lanciarne le prove resta libero. Se il cambiamento serve davvero, dillo a '
    'Theo e lo scrive lui da un terminale fuori da Claude Code — per la '
    "configurazione dei ganci c'è il suo permesso scritto, per il nucleo del "
    "divieto non c'è niente, di proposito.")


def core_of_rust(text):
    """Il motivo dentro il rifiuto del Rust, o None se il testo non è quello.

    Restituire None significa «divergenza nuova»: è la direzione sicura.
    """
    head, tail = RUST_REFUSAL
    if not text.startswith(head) or not text.endswith(tail):
        return None
    return text[len(head):len(text) - len(tail)]


def expected(label, python, rust):
    """La divergenza è una di quelle dichiarate sopra?"""
    if label == STRICTER_LABEL:
        # Qui il Rust è più severo del suo oracolo, ed è voluto: `ln`,
        # `install`, `rsync`, `patch` e un `cp` con sorgente lungo
        # rimpiazzavano il gancio senza che nessuno dei due dicesse niente.
        # Tollerata la sola direzione che stringe: se un giorno il Rust
        # tornasse a tacere, la riga qui sotto lo fa risultare divergenza nuova.
        return (decision(python), decision(rust)) == ('muto', 'deny')
    if label in (READ_ONLY_LABEL, OWN_TOOL_LABEL):
        # Il Rust deve TACERE, e tacere bene. `decision` dice solo che stdout è
        # vuoto, e stdout è vuoto anche in un processo caduto: senza le tre
        # righe qui sotto, un gancio che va in panico su un confine di
        # carattere — il guasto che `floor_boundary` esiste per evitare, e che
        # essendo fail-open lascia passare tutto in silenzio — verrebbe contato
        # fra le divergenze dichiarate e il confronto direbbe «0 nuove».
        if rust['uscita'] != 0 or rust['stderr'] or rust['registro']:
            return False
        return (decision(python), decision(rust)) == ('deny', 'muto')
    if decision(python) != decision(rust):
        return False
    if python['uscita'] != rust['uscita'] or python['registro'] != rust['registro']:
        return False
    if python['stderr'] != rust['stderr']:
        return False
    core = core_of_rust(reason(rust))
    # Il motivo del Python vive dentro le sue parentesi, e deve essere lo stesso.
    return core is not None and f'({core})' in reason(python)


def main():
    if not os.path.exists(BINARY):
        sys.exit(f'manca il binario: {BINARY}\n  cargo build --release')

    battery = load_battery()
    root = tempfile.mkdtemp(prefix='confronto-linear-')
    hook_copy = fake_home(root)
    register = os.path.join(root, 'registro.jsonl')

    total = 0
    divergent = []
    known = 0
    for label, command, tool, tool_input, permission, raw in cases(battery, root):
        total += 1
        python, rust = run_one(command, tool, tool_input, root, hook_copy,
                               register, permission, raw)
        if python == rust:
            continue
        if expected(label, python, rust):
            known += 1
            continue
        divergent.append((label, command or json.dumps(tool_input or raw),
                          python, rust))

    shutil.rmtree(root, ignore_errors=True)

    print(f'{total} casi confrontati, {len(divergent)} divergenze nuove '
          f'({known} dichiarate)')

    # Anche il numero delle dichiarate è un controllo, non una curiosità: se un
    # caso smette di esercitare la sua deroga — il file della HOME finta che non
    # viene creato, un comando che cambia forma — i due gemelli tornano identici
    # e la deroga esce dal conto senza entrare fra le nuove. Uscita 0, e l'unico
    # sintomo un numero più basso che nessuno guarda.
    if known != EXPECTED_KNOWN and not divergent:
        print(f'\nle divergenze dichiarate sono {known}, attese '
              f'{EXPECTED_KNOWN}: o una deroga ha smesso di essere esercitata, '
              f'o ne è comparsa una che nessuno ha dichiarato. Vale come rosso.')
        return 1

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
