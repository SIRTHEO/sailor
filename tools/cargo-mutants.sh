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
# risultava «ucciso» con un numero impossibile. Qui si toglie di mezzo.
unset CARGO_TARGET_DIR

# Il tetto al parallelismo. `-j` non moltiplica solo i lavori: ogni lavoro lancia
# un `cargo` che a sua volta impegna tutti i core, e la memoria dei collegamenti
# si somma. Misurato il 24/08/2026 sullo stesso lotto, acceso e spento tre volte:
# -j 1 → 956 MB di picco, -j 2 → 1.045 MB, -j 4 → 2.036 e poi 2.443 MB.
# Con -j 4 quella notte lo swap si è esaurito, il kernel ha sospeso Arc e Orca e
# Theo ha subìto tre chiusure forzate. Chi non dichiara niente prende 2; chi
# dichiara di più tiene la sua scelta e legge sullo schermo quanto costa —
# imporlo di forza toglierebbe una misura legittima a chi la vuole fare.
JOBS_CAP=2

# `cargo mutants` accetta quattro forme: `-j N`, `-jN`, `--jobs N`, `--jobs=N`.
declared_jobs=""
awaiting_value=""
for arg in "$@"; do
  case "$arg" in
    -j | --jobs)
      awaiting_value="yes"
      continue
      ;;
    -j*) declared_jobs="${arg#-j}" ;;
    --jobs=*) declared_jobs="${arg#--jobs=}" ;;
    *)
      if [ -n "$awaiting_value" ]; then declared_jobs="$arg"; fi
      ;;
  esac
  awaiting_value=""
done

case "$declared_jobs" in
  '')
    set -- -j "$JOBS_CAP" "$@"
    ;;
  *[!0-9]*)
    # Non è un numero: lascia protestare `cargo mutants`, che lo dice meglio.
    ;;
  *)
    if [ "$declared_jobs" -gt "$JOBS_CAP" ]; then
      {
        echo "AVVERTIMENTO: -j $declared_jobs supera il tetto di casa ($JOBS_CAP)."
        echo "  Picco di memoria misurato il 24/08/2026 sullo stesso lotto:"
        echo "    -j 1 -> 956 MB     -j 2 -> 1.045 MB     -j 4 -> 2.036 e 2.443 MB"
        echo "  Con -j 4 lo swap si è esaurito: il kernel ha sospeso Arc e Orca,"
        echo "  e sono seguite tre chiusure forzate. La scelta resta la tua."
      } >&2
    fi
    ;;
esac

cd "$ROOT" || exit 1
# `--gitignore=true` non è il valore predefinito in `cargo-mutants 27.1.0`: senza,
# ogni copia dell'albero si porta dietro i `target-*` delle sessioni parallele.
# Misurato il 24/08/2026 sullo stesso lotto: 2,3 GB per copia contro 303 MB.
# LA RIGA DI `.gitignore` DEL COMMIT `7ad541b` NON FA QUESTO LAVORO, e il suo
# messaggio afferma il contrario: le copie fatte venticinque minuti dopo quel
# commit contenevano ancora tutte e tre le cartelle. Sta scritto qui perché chi
# rilegge quel messaggio lo prenderebbe per vero.
# Non toglie `target/`, che arriva da `--copy-target` (acceso di suo) e va
# tenuto: è ciò che evita di ricompilare da zero a ogni copia. Sta prima di
# `"$@"` perché è un valore predefinito, non una museruola: chi ha motivo di
# copiare anche gli artefatti scrive `--gitignore=false` in coda e vince lui.
exec cargo mutants --gitignore=true "$@"
