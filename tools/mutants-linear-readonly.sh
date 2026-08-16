#!/usr/bin/env bash
# La batteria si prova rompendo il controllo, non aggiungendo casi verdi.
#
# Un confronto che dà 204 uguali non dimostra niente finché non si sa che cosa
# lo farebbe diventare rosso. Qui si rompe una per una le invarianti che questo
# gancio dichiara di reggere, e si pretende che il confronto se ne accorga.
# Il mutante più informativo è togliere ciò che il commit dichiara di proteggere.
#
#   bash tools/mutants-linear-readonly.sh
#
# Ripristina il sorgente in ogni caso, anche se interrotto.
set -uo pipefail

ROOT="$HOME/.claude/rust"
GUARD="$ROOT/crates/guards/src/linear_readonly.rs"
STATE="$ROOT/crates/claude-hooks/src/linear.rs"
BACKUP=$(mktemp -d)
cp "$GUARD" "$BACKUP/guard.rs"
cp "$STATE" "$BACKUP/state.rs"
restore_files() { cp "$BACKUP/guard.rs" "$GUARD"; cp "$BACKUP/state.rs" "$STATE"; }
restore() { restore_files; rm -rf "$BACKUP"; }
trap restore EXIT

survivors=0
broken=0

mutate() {
  local name="$1" file="$2" script="$3"
  restore_files
  # Un mutante che non muta niente risulterebbe «sopravvissuto» senza aver mai
  # cambiato il comportamento: è un difetto dello script, non un punto cieco
  # della batteria, e va distinto ad alta voce.
  if ! python3 - "$file" <<PY
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
  if (cd "$ROOT" && python3 tools/compare-linear-readonly.py >/dev/null 2>&1); then
    echo "  SOPRAVVIVE  $name  <- la batteria non lo vede"
    survivors=$((survivors + 1))
  else
    echo "  ucciso      $name"
  fi
}

echo "mutanti su linear_readonly.rs:"

# 1. L'elenco chiuso diventa aperto: ogni sottocomando delle due CLI passa.
#    È il cuore del gancio — il difetto che un giudice indipendente trovò nella
#    prima versione, quando l'elenco era quello delle scritture.
mutate "elenco delle letture aperto" "$GUARD" \
  's = s.replace("    Some(format!(\n        \"{label} {}\",", "    if true { return None }\n    Some(format!(\n        \"{label} {}\",", 1)'

# 2. La valvola torna a valere ovunque nella riga, non solo in testa: è la falla
#    chiusa il 29/07/2026, quando cercarla come stringa autorizzava tutto.
mutate "valvola cercata ovunque" "$GUARD" \
  's = s.replace("pub fn declared(line: &str) -> bool {", "pub fn declared(line: &str) -> bool {\n    if line.contains(\"OK_UTENTE=1\") { return true }", 1)'

# 3. Il nucleo diventa scavalcabile come la configurazione: una valvola che
#    autorizza il proprio smontaggio non è una valvola.
mutate "nucleo declassato a scavalcabile" "$GUARD" \
  's = s.replace(".map(|f| (*f, Valve::Core))", ".map(|f| (*f, Valve::UserDeclared))", 1)'

# 4. Gli involucri non vengono più tolti: `timeout 30 linear issue close` era
#    la scorciatoia più corta per aggirare la versione precedente.
mutate "involucri non spogliati" "$GUARD" \
  's = s.replace("fn strip_wrappers(mut names: Vec<String>) -> Vec<String> {", "fn strip_wrappers(mut names: Vec<String>) -> Vec<String> {\n    return names;\n    #[allow(unreachable_code)]", 1)'

# 5. Il registro smette di distinguere il nucleo: stesso esito, traccia diversa.
#    Prova che il confronto guarda il file scritto, non solo la decisione.
mutate "esito del nucleo indistinto" "$STATE" \
  's = s.replace("\"negato-nucleo\"", "\"negato\"", 1)'

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
