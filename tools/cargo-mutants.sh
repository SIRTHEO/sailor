#!/usr/bin/env bash
# I mutanti generati a macchina: l'unica scelta che non eredita i punti ciechi
# di chi ha scritto il codice.
#
#   bash tools/cargo-mutants.sh -p guards -f 'crates/guards/src/instincts.rs'
#   bash tools/cargo-mutants.sh -p guards          tutta la cassa
#
# Il gemello di `tools/mutants.sh`, che i mutanti se li fa dettare a mano: quello
# prova le invarianti che qualcuno ha pensato, questo prova quelle che nessuno ha
# pensato. Restano due strumenti perché non condividono niente — là si muta in
# sede con copia di sicurezza, qui la copia la fa `cargo mutants`.
#
# PERCHÉ NON SI CHIAMA `cargo mutants` A MANO. Lo strumento copia i sorgenti in
# una cartella temporanea e pretende che la batteria sia verde **lì**. Una prova
# della batteria interroga l'indice di git di questo deposito, e su una copia
# senza `.git` cade: base rossa, zero mutanti misurati, che è lo stato in cui la
# configurazione è rimasta fino al 24/08/2026. La variabile qui sotto le ridà il
# deposito vero. `cargo mutants` non sa passare variabili ai figli, ma i figli
# ereditano l'ambiente di chi lo lancia — per questo basta esportarla qui.
#
# Fuori dal sandbox: dentro, `/tmp` e `/var/folders` sono negati e la copia
# risulta rossa per finta.
set -uo pipefail

ROOT="$HOME/.claude/rust"
export CLAUDE_HOOKS_REPO_ROOT="$HOME/.claude"

# `cargo mutants` dà a ogni lavoro parallelo la propria copia dell'albero, e
# quindi la propria cartella di build — ma una `CARGO_TARGET_DIR` ereditata le
# rimette tutte sulla stessa, e i lavori si sovrascrivono a vicenda: un mutante
# risultava «ucciso» con un numero impossibile. Qui si toglie di mezzo, invece
# di imporre `-j 1` a chi lancia.
unset CARGO_TARGET_DIR

cd "$ROOT" || exit 1
exec cargo mutants "$@"
