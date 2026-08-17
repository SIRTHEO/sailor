#!/usr/bin/env bash
# La batteria si prova rompendo il controllo, non aggiungendo casi verdi.
#
# Un confronto che dà «zero divergenze» non dimostra niente finché non si sa che
# cosa lo farebbe diventare rosso. Qui si rompe una per una le invarianti che
# ogni guardia dichiara di reggere, e si pretende che il confronto se ne accorga.
# Il mutante più informativo è togliere ciò che il commit dichiara di proteggere.
#
#   bash tools/mutants.sh linear-readonly
#   bash tools/mutants.sh code-language
#   bash tools/mutants.sh                  tutte
#
# Uno script solo e non uno per guardia: la parte che cambia è l'elenco dei
# mutanti, tutto il resto — copia di sicurezza, ricompilazione, confronto,
# ripristino — è identico, e la seconda copia diverge alla prima correzione
# fatta da una parte sola.
#
# Ripristina i sorgenti in ogni caso, anche se interrotto.
set -uo pipefail

ROOT="$HOME/.claude/rust"
BACKUP=$(mktemp -d)
FILES=(
  "crates/guards/src/duplication.rs"
  "crates/guards/src/linear_readonly.rs"
  "crates/claude-hooks/src/linear.rs"
  "crates/guards/src/code_language.rs"
  "crates/guards/src/language.rs"
)
for f in "${FILES[@]}"; do
  mkdir -p "$BACKUP/$(dirname "$f")"
  cp "$ROOT/$f" "$BACKUP/$f"
done
restore_files() { for f in "${FILES[@]}"; do cp "$BACKUP/$f" "$ROOT/$f"; done; }

# Ripristinare i sorgenti non basta: **il binario resta quello mutato**, ed è
# quello che i ganci di questa macchina eseguono davvero. Il 17/08/2026 questo
# script ha lasciato in produzione il binario dell'ultimo mutante, e il confronto
# lanciato subito dopo ha dato 318 divergenze che sembravano un difetto del
# porting. Ricompilare fa parte del ripristino, non è un di più.
restore() {
  restore_files
  (cd "$ROOT" && cargo build --release >/dev/null 2>&1) \
    || echo "ATTENZIONE: ricompilazione fallita, il binario è ancora quello mutato"
  rm -rf "$BACKUP"
}
trap restore EXIT

survivors=0
broken=0
WHICH="${1:-tutte}"
COMPARE=""

mutate() {
  local name="$1" file="$2" script="$3"
  restore_files
  # Un mutante che non muta niente risulterebbe «sopravvissuto» senza aver mai
  # cambiato il comportamento: è un difetto dello script, non un punto cieco
  # della batteria, e va distinto ad alta voce.
  if ! python3 - "$ROOT/$file" <<PY
import pathlib, sys
p = pathlib.Path(sys.argv[1]); before = p.read_text(); s = before
$script
p.write_text(s)
sys.exit(0 if s != before else 1)
PY
  then
    echo "  NON APPLICATO  $name  <- lo script del mutante è da correggere"
    broken=$((broken + 1))
    return
  fi
  if ! (cd "$ROOT" && cargo build --release >/dev/null 2>&1); then
    echo "  $name: non compila — mutante scartato"
    return
  fi
  if (cd "$ROOT" && python3 "tools/$COMPARE" >/dev/null 2>&1); then
    echo "  SOPRAVVIVE  $name  <- la batteria non lo vede"
    survivors=$((survivors + 1))
  else
    echo "  ucciso      $name"
  fi
}

# ── linear-readonly ─────────────────────────────────────────────────────────
if [ "$WHICH" = "linear-readonly" ] || [ "$WHICH" = "tutte" ]; then
  echo "mutanti su linear_readonly.rs:"
  COMPARE="compare-linear-readonly.py"
  G="crates/guards/src/linear_readonly.rs"
  S="crates/claude-hooks/src/linear.rs"

  # L'elenco chiuso diventa aperto: ogni sottocomando delle due CLI passa. È il
  # cuore del gancio — il difetto che un giudice indipendente trovò nella prima
  # versione, quando l'elenco era quello delle scritture.
  mutate "elenco delle letture aperto" "$G" \
    's = s.replace("    Some(format!(\n        \"{label} {}\",", "    if true { return None }\n    Some(format!(\n        \"{label} {}\",", 1)'

  # La valvola torna a valere ovunque nella riga, non solo in testa: è la falla
  # chiusa il 29/07/2026, quando cercarla come stringa autorizzava tutto.
  mutate "valvola cercata ovunque" "$G" \
    's = s.replace("pub fn declared(line: &str) -> bool {", "pub fn declared(line: &str) -> bool {\n    if line.contains(\"OK_UTENTE=1\") { return true }", 1)'

  # Una valvola che autorizza il proprio smontaggio non è una valvola.
  mutate "nucleo declassato a scavalcabile" "$G" \
    's = s.replace(".map(|f| (*f, Valve::Core))", ".map(|f| (*f, Valve::UserDeclared))", 1)'

  # `timeout 30 linear issue close` era la scorciatoia più corta per aggirare la
  # versione precedente.
  mutate "involucri non spogliati" "$G" \
    's = s.replace("fn strip_wrappers(mut names: Vec<String>) -> Vec<String> {", "fn strip_wrappers(mut names: Vec<String>) -> Vec<String> {\n    return names;\n    #[allow(unreachable_code)]", 1)'

  # Stesso esito, traccia diversa: prova che il confronto guarda il file scritto
  # e non solo la decisione.
  mutate "esito del nucleo indistinto" "$S" \
    's = s.replace("\"negato-nucleo\"", "\"negato\"", 1)'
fi

# ── code-language ───────────────────────────────────────────────────────────
if [ "$WHICH" = "code-language" ] || [ "$WHICH" = "tutte" ]; then
  echo "mutanti su code_language.rs:"
  COMPARE="compare-code-language.py"
  G="crates/guards/src/code_language.rs"
  L="crates/guards/src/language.rs"

  # I commenti tornano dentro: citare una chiamata verrebbe letto come farla, e
  # il gancio rimprovererebbe chi documenta bene.
  mutate "commenti non tolti" "$G" \
    's = s.replace("    comment().replace_all(text, \" \").into_owned()", "    text.to_string()", 1)'

  # Il difetto storico: col punto che attraversa le righe, un commento si mangia
  # il resto del file e il controllo tace su tutto ciò che viene dopo.
  mutate "il commento si mangia il file" "$G" \
    's = s.replace(r"(?m)^[ \t]*(?:#|//).*$", r"(?ms)^[ \t]*(?:#|//).*$", 1)'

  # Sparisce la soglia di lunghezza: `${x}` e le stringhe di due parole tornano
  # a dire qualcosa sulla lingua, che non dicono.
  mutate "soglia di lunghezza rimossa" "$G" \
    's = s.replace("if bare.chars().count() < 12 {", "if false {", 1)'

  # Il perimetro si allarga a tutto: il controllo comincia a giudicare sorgenti
  # che la regola lascia in italiano di proposito.
  mutate "perimetro esteso a ogni file" "$G" \
    's = s.replace("    None\n}\n\n/// `Path(p).suffix`", "    Some(Family::Gate)\n}\n\n/// `Path(p).suffix`", 1)'

  # I nomi inglesi che una radice troppo corta prende per italiani: `mute` da
  # `mut`, `question` da `quest`, `right` da `righ`.
  mutate "elenco dei nomi inglesi svuotato" "$L" \
    's = s.replace("        .into_iter()\n        .collect()\n    })\n}\n\n/// Spezza un identificatore", "        .into_iter()\n        .filter(|_x: &&str| false)\n        .collect()\n    })\n}\n\n/// Spezza un identificatore", 1)'

  # Solo le dichiarazioni: chi **usa** un nome italiano scritto altrove non lo
  # sta introducendo, e accusarlo sposta la colpa sul chiamante.
  mutate "anche gli usi contano come dichiarazioni" "$G" \
    's = s.replace(r"|\b(?:function|const|let|var|class)\s+([A-Za-z_$]\w*)", r"|\b([A-Za-z_$]\w*)\s*\(", 1)'
fi

# ── duplication ─────────────────────────────────────────────────────────────
if [ "$WHICH" = "duplication" ] || [ "$WHICH" = "tutte" ]; then
  echo "mutanti su duplication.rs:"
  COMPARE="compare-duplication.py"
  D="crates/guards/src/duplication.rs"

  # La linea di base ignorata: e' cio' che tiene acceso il gancio. Senza,
  # toccare uno dei 122 file col debito vecchio fa scattare un rimprovero per
  # una copia scritta mesi fa da un altro.
  mutate "linea di base ignorata" "$D" \
    's = s.replace("pub fn load_baseline(root: &Path) -> HashSet<String> {", "pub fn load_baseline(root: &Path) -> HashSet<String> {\n    if true { return HashSet::new() }", 1)'

  # Il minimo di sostanza sparisce: quattro righe di intelaiatura JSX tornano a
  # contare come una copia, e il rapporto si riempie di rumore.
  mutate "minimo di sostanza azzerato" "$D" \
    's = s.replace("const MIN_CHARS: usize = 180;", "const MIN_CHARS: usize = 0;", 1)'

  # Gli import tornano a contare come logica: sono forma condivisa, e contarli
  # segnala come copiati due file che importano le stesse cose.
  mutate "import contati come logica" "$D" \
    's = s.replace("fn is_import(line: &str) -> bool {\n    let t = line.trim_start();", "fn is_import(line: &str) -> bool {\n    if line.len() > 0 { return false }\n    let t = line.trim_start();", 1)'

  # I punti di riuso escono dal confronto: e' il caso piu' prezioso — riscrivere
  # a mano una funzione che esiste gia' in `lib/`.
  mutate "punti di riuso non confrontati" "$D" \
    's = s.replace("""&["/lib/", "/hooks/", "/utils/", "/helpers/", "/shared/"]""", "&[]", 1)'

  # Le prove tornano a confrontarsi col codice di produzione: due mestieri
  # diversi, e il rumore che ne esce non e' azionabile.
  mutate "prove confrontate col codice" "$D" \
    's = s.replace("            if is_test(p) != target_is_test {\n                return false;\n            }\n", "", 1)'

  # L'impronta cambia forma: il congelato del Python diventa illeggibile da qui,
  # e tutto il debito vecchio torna a parlare in una volta.
  mutate "impronta piu' corta" "$D" \
    's = s.replace("hex[..16].to_string()", "hex[..12].to_string()", 1)'
fi

echo
if [ "$broken" -gt 0 ]; then
  echo "$broken mutanti non applicati: correggi lo script prima di leggere il resto"
fi
if [ "$survivors" -eq 0 ] && [ "$broken" -eq 0 ]; then
  echo "tutti i mutanti uccisi: la batteria vede ciò che dichiara di vedere"
else
  echo "$survivors mutanti sopravvissuti: la batteria ha un punto cieco"
fi
exit $((survivors + broken))
