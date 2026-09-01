#!/bin/sh
# Pubblica il ramo corrente, dopo aver verificato le cose che si dimenticano.
#
# **PERCHÉ UNO SCRIPT E NON UNA REGOLA SCRITTA DA QUALCHE PARTE.** È la stessa
# ragione per cui esiste `sailor release`, scritta nel suo modulo: il gesto
# sembra personale e ha effetto collettivo, e una difesa fatta di «ricordarsi
# di» perde contro il gesto che lo strumento suggerisce — `git push` e basta.
# Un comando rende corta la strada giusta.
#
# **IL GUASTO CHE ESISTE PER IMPEDIRE**, misurato il 01/09/2026: trentacinque
# commit e una giornata intera di lavoro sono rimasti sulla macchina di chi li
# aveva scritti, con la batteria verde e niente che lo dicesse. Nessuno aveva
# deciso di non pubblicare: semplicemente non esisteva il momento in cui si
# pubblica.
#
# **DOVE DOVREBBE FINIRE.** Dentro `sailor`, come `sailor release`, il giorno
# che la tabella dei comandi smette di muoversi. Uno script è la forma
# provvisoria: si vede, si legge, e non aspetta nessuno.

set -eu

readonly PUBLISHABLE="main sorgenti"

say() { printf '%s\n' "$*"; }
refuse() {
    printf 'non pubblico: %s\n' "$*" >&2
    exit 1
}

skip_tests=no
for argument in "$@"; do
    case "$argument" in
        --senza-prove)
            # Esiste, si dichiara, e si vede nell'uscita: una scorciatoia presa
            # in silenzio è la stessa cosa di una difesa che non c'è.
            skip_tests=yes
            ;;
        *) refuse "opzione sconosciuta «${argument}» (c'è solo --senza-prove)" ;;
    esac
done

cd "$(git rev-parse --show-toplevel)"
branch=$(git rev-parse --abbrev-ref HEAD)

# Il ramo. Pubblicare da un ramo di lavoro è quasi sempre uno sbaglio di fretta.
#
# **LE GRAFFE NON SONO STILE.** Senza, `$branch»` viene letto come un nome di
# variabile che include il carattere di chiusura: la prima esecuzione di questo
# script è morta con «branch»: unbound variable» invece di stampare il rifiuto.
# Un messaggio in italiano dentro uno script POSIX vuole `${nome}` ogni volta
# che tocca una virgoletta.
case " $PUBLISHABLE " in
    *" $branch "*) ;;
    *) refuse "«${branch}» non è un ramo che si pubblica (sono: $PUBLISHABLE)" ;;
esac

# L'albero. **Un albero sporco vuol dire che quello che pubblichi non è quello
# che hai provato**, ed è successo davvero: trenta file non committati nel clone
# condiviso tenevano il lavoro di tre sessioni diverse, e l'attribuzione è persa.
if [ -n "$(git status --porcelain)" ]; then
    git status --short
    refuse "l'albero non è pulito: quello che pubblicheresti non è quello che hai provato"
fi

# Il remoto. **Non ci si fida del riferimento locale**: dopo un fetch fallito
# `rev-list` risponde «allineato» sul riferimento vecchio, cioè il primo gesto
# di ogni ripresa mente. Si chiede al remoto adesso.
say "chiedo al remoto dove è arrivato…"
git fetch --quiet origin "$branch" 2>/dev/null || say "  (il ramo non c'è ancora sul remoto: sarà il primo invio)"

upstream="origin/$branch"
if git rev-parse --verify --quiet "$upstream" >/dev/null; then
    behind=$(git rev-list --count "$branch..$upstream")
    if [ "$behind" -gt 0 ]; then
        refuse "il remoto è avanti di $behind commit: prima porta qui il lavoro di altri, poi ripubblica"
    fi
    ahead=$(git rev-list --count "$upstream..$branch")
    range="$upstream..$branch"
else
    ahead=$(git rev-list --count "$branch")
    range="$branch"
fi

if [ "$ahead" -eq 0 ]; then
    say "non c'è niente da pubblicare: «${branch}» è già quello che c'è sul remoto."
    exit 0
fi

say "da pubblicare: $ahead commit su «${branch}»."

# Quello che non deve uscire. Non è un antivirus: è la lista delle cose che sono
# davvero finite in un repo per distrazione.
say "guardo cosa sta per uscire…"
leaks=$(git diff "$range" | grep -inE '^\+.*(sk-ant-|ghp_|gho_|github_pat_|BEGIN [A-Z ]*PRIVATE KEY|aws_secret_access_key)' || true)
if [ -n "$leaks" ]; then
    printf '%s\n' "$leaks" | head -5
    refuse "sembra un segreto. Se è un falso allarme, toglilo dal diff o cambialo di forma"
fi

heavy=$(git diff --name-only "$range" | while read -r file; do
    [ -f "$file" ] || continue
    size=$(wc -c < "$file")
    [ "$size" -gt 1000000 ] && printf '%s KB  %s\n' "$((size / 1024))" "$file"
done)
if [ -n "$heavy" ]; then
    printf '%s\n' "$heavy"
    refuse "file sopra il megabyte: un repo git non dimentica, e questi restano per sempre"
fi

# La batteria. **Mai incanalata in `tail` o `grep`**: l'uscita di una pipeline è
# dell'ultimo comando, quindi `cargo test | tail` esce 0 con la batteria rossa.
# Si scrive su file e si legge il codice d'uscita vero.
if [ "$skip_tests" = yes ]; then
    say "prove SALTATE su richiesta (--senza-prove)."
else
    report="${TMPDIR:-/tmp}/pubblica-batteria-$$.txt"
    say "faccio girare la batteria…"
    if cargo test --workspace >"$report" 2>&1; then
        passed=$(grep -oE '^test result: ok\. [0-9]+ passed' "$report" | awk '{sum += $4} END {print sum + 0}')
        say "  verde: $passed prove."
        rm -f "$report"
    else
        tail -30 "$report" >&2
        say "il resoconto intero è in $report" >&2
        refuse "la batteria è rossa"
    fi
fi

say "pubblico «${branch}» su origin…"
git push origin "$branch"
say "fatto: $ahead commit pubblicati."
