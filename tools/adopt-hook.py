#!/usr/bin/env python3
"""Fa puntare un gancio registrato al binario Rust, tenendo il vecchio come rete.

Uno strumento solo, perché i ganci da portare sono 29 e la prima versione di
questo file era già stata ricopiata due volte: la parte che cambia è il nome del
gancio e la riga da sostituire, tutto il resto è identico.

    adopt-hook.py cd-guard 'python3 /Users/theo/.claude/skills/hooks/cd-guard.py'
    adopt-hook.py --revert cd-guard

L'ORDINE CONTA, ed è la lezione del 16/08/2026: prima il binario dimostra di
decidere bene (`--check`), poi la configurazione lo nomina. L'ordine inverso ha
spento `Bash` in tutte le sessioni della macchina, e da dentro non si rimediava.

Non si usa `binario && … || vecchio`: se il binario blocca — uscita 2, che è una
**decisione**, non un guasto — quella forma eseguirebbe anche il vecchio, con
doppio messaggio e doppio effetto. Serve un `if`, che costa un avvio di shell
(~3 ms).

La rete copre il caso «binario assente o non eseguibile». Non copre un binario
che parte e sbaglia: per quello c'è `--check`, che va rilanciato a ogni build.
"""
import argparse
import json
import shutil
import subprocess
import sys
import time

SETTINGS = '/Users/theo/.claude/settings.json'
BINARY = '/Users/theo/.claude/rust/target/release/claude-hooks'


def registered(hook: str, fallback: str, async_out: str = '') -> str:
    """La riga che va in settings.json, con la rete di sicurezza.

    `async_out` serve ai ganci che non si possono aspettare. Il censimento dei
    ganci impiega **sedici minuti** e sta su `SessionEnd`: registrato nella forma
    sincrona bloccherebbe la chiusura di ogni sessione, e l'uscita finirebbe sul
    terminale invece che nel suo rapporto. Il Python originale infatti è
    registrato con `nohup … > file 2>&1 &`, e la rete deve conservarlo.
    """
    if async_out:
        # Un comando che finisce con `&` è già terminato: aggiungergli un `;`
        # produce `& ; fi`, che è un errore di sintassi — e una riga di
        # `settings.json` che non si legge rompe l'evento intero, in silenzio.
        # Successo il 17/08/2026 al primo uso di questa forma.
        tail = fallback if fallback.rstrip().endswith('&') else f'{fallback};'
        return (
            f'B={BINARY}; if [ -x "$B" ]; then nohup "$B" {hook} > {async_out} 2>&1 & '
            f'else {tail} fi'
        )
    return (
        f'B={BINARY}; if [ -x "$B" ]; then "$B" {hook}; else {fallback}; fi'
    )


def self_check() -> None:
    result = subprocess.run([BINARY, '--check'], capture_output=True, text=True)
    if result.returncode != 0:
        sys.exit(f'--check fallito, non tocco la configurazione:\n{result.stderr}')
    print(f'  --check: {result.stdout.strip()}')


def swap(old: str, new: str, everywhere: bool = False) -> None:
    text = open(SETTINGS).read()
    needle = json.dumps(old)          # con le virgolette e gli escape del file
    replacement = json.dumps(new)
    found = text.count(needle)
    if found == 0:
        sys.exit(f'la riga da sostituire non compare: {old!r}')
    # Lo stesso gancio registrato su due eventi è normale — `link-worktree-rules`
    # sta su `SessionStart` e su `PostToolUse` con la stessa identica riga — ma
    # non si sostituisce alla cieca: chi adotta deve dire che le vuole entrambe.
    # Aggiunto il 17/08/2026, dopo che l'adozione si era fermata proprio lì.
    if found != 1 and not everywhere:
        sys.exit(
            f'la riga da sostituire compare {found} volte, non una: {old!r}\n'
            '  se le vuoi tutte: --tutte'
        )
    # La riga dev'essere shell valida PRIMA di entrare nella configurazione.
    # `bash -n` la analizza senza eseguirla; senza questo controllo, il 17/08 una
    # riga con `& ; fi` è finita in `SessionEnd` e nessuno se ne sarebbe accorto
    # fino alla chiusura di una sessione.
    check = subprocess.run(['bash', '-n', '-c', new], capture_output=True, text=True)
    if check.returncode != 0:
        sys.exit(f'la riga non e shell valida, non la scrivo:\n  {new}\n{check.stderr}')
    text = text.replace(needle, replacement)
    json.loads(text)                  # non si scrive un JSON che non si rilegge
    stamp = time.strftime('%Y%m%d-%H%M%S')
    shutil.copy(SETTINGS, f'/Users/theo/.claude/backups/settings.json.{stamp}')
    open(SETTINGS, 'w').write(text)


def main() -> None:
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('hook', help='il sottocomando del binario, es. cd-guard')
    parser.add_argument('fallback', nargs='?', help='il comando da sostituire e da tenere come rete')
    parser.add_argument('--revert', action='store_true', help='torna al comando originale')
    parser.add_argument('--tutte', action='store_true',
                        help='sostituisce ogni occorrenza, per un gancio registrato su piu eventi')
    parser.add_argument('--async-out', default='', metavar='FILE',
                        help='per i ganci lenti: gira in sottofondo scrivendo qui')
    # Serve quando la riga da togliere non è più deducibile: una registrazione
    # sbagliata scritta da una versione precedente di questo strumento. Il
    # presidio della configurazione rifiuta ogni altra strada, e giustamente:
    # chi ha rotto la riga deve poterla riparare *da qui*, non a mano.
    parser.add_argument('--replace-exact', default='', metavar='TESTO',
                        help='sostituisce questa riga esatta col comando di ripiego')
    args = parser.parse_args()

    if args.replace_exact:
        if not args.fallback:
            sys.exit('serve anche il comando che deve prendere il suo posto')
        swap(args.replace_exact, args.fallback, args.tutte)
        print(f'{args.hook}: riga sostituita con {args.fallback[:60]}…')
        return

    if args.revert:
        if not args.fallback:
            sys.exit('per tornare indietro serve il comando originale')
        swap(registered(args.hook, args.fallback, args.async_out), args.fallback, args.tutte)
        print(f'{args.hook}: torna a {args.fallback}')
        return

    if not args.fallback:
        sys.exit('serve il comando attuale, quello che resta come rete')
    self_check()
    swap(args.fallback, registered(args.hook, args.fallback, args.async_out), args.tutte)
    print(f'{args.hook}: ora passa dal binario, con ritorno al vecchio se manca')


if __name__ == '__main__':
    main()
