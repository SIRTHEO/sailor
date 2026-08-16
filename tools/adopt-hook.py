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


def registered(hook: str, fallback: str) -> str:
    """La riga che va in settings.json, con la rete di sicurezza."""
    return (
        f'B={BINARY}; if [ -x "$B" ]; then "$B" {hook}; else {fallback}; fi'
    )


def self_check() -> None:
    result = subprocess.run([BINARY, '--check'], capture_output=True, text=True)
    if result.returncode != 0:
        sys.exit(f'--check fallito, non tocco la configurazione:\n{result.stderr}')
    print(f'  --check: {result.stdout.strip()}')


def swap(old: str, new: str) -> None:
    text = open(SETTINGS).read()
    needle = json.dumps(old)          # con le virgolette e gli escape del file
    replacement = json.dumps(new)
    found = text.count(needle)
    if found != 1:
        sys.exit(f'la riga da sostituire compare {found} volte, non una: {old!r}')
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
    args = parser.parse_args()

    if args.revert:
        if not args.fallback:
            sys.exit('per tornare indietro serve il comando originale')
        swap(registered(args.hook, args.fallback), args.fallback)
        print(f'{args.hook}: torna a {args.fallback}')
        return

    if not args.fallback:
        sys.exit('serve il comando attuale, quello che resta come rete')
    self_check()
    swap(args.fallback, registered(args.hook, args.fallback))
    print(f'{args.hook}: ora passa dal binario, con ritorno al vecchio se manca')


if __name__ == '__main__':
    main()
