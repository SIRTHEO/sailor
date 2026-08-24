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

# Un nome che nessuna sezione riconosce esegue zero mutanti, e il verdetto in
# fondo leggeva quello zero come «nessun sopravvissuto»: verde su niente. Si
# ferma prima di toccare qualunque cosa. I nomi si rileggono da questo stesso
# file, cosi' una sezione nuova entra nell'elenco senza doverlo ricopiare.
SELF="$ROOT/tools/mutants.sh"
known_sections() {
  grep -oE '^if \[ "\$WHICH" = "[A-Za-z0-9_-]+"' "$SELF" | sed -E 's/^.*= "//; s/"$//' | sort -u
}
WHICH="${1:-tutte}"
if [ "$WHICH" != "tutte" ] && ! known_sections | grep -qx "$WHICH"; then
  {
    echo "mutants.sh: modulo sconosciuto: '$WHICH' — nessun mutante da eseguire."
    echo "  sezioni disponibili (oppure 'tutte'):"
    known_sections | sed 's/^/    /'
  } >&2
  exit 2
fi

# Senza copia di sicurezza i mutanti finirebbero sui sorgenti veri e il
# ripristino fallirebbe in silenzio: successo il 23/08/2026 in una sandbox che
# nega /var/folders. Qui ci si ferma prima.
BACKUP=$(mktemp -d) || { echo "mutants.sh: mktemp -d fallito, niente copia di sicurezza: mi fermo" >&2; exit 1; }
[ -d "$BACKUP" ] || { echo "mutants.sh: $BACKUP non esiste: mi fermo" >&2; exit 1; }
FILES=(
  "crates/guards/src/duplication.rs"
  "crates/guards/src/linear_readonly.rs"
  "crates/claude-hooks/src/linear.rs"
  "crates/guards/src/code_language.rs"
  "crates/guards/src/language.rs"
  "crates/guards/src/live_rules.rs"
  "crates/claude-hooks/src/live_rules.rs"
  "crates/guards/src/handoff.rs"
  "crates/guards/src/handoff_on_stop.rs"
  "crates/claude-hooks/src/successor.rs"
  "crates/claude-hooks/src/handoff_on_stop.rs"
  "crates/guards/src/worktree_deletes.rs"
  "crates/claude-hooks/src/worktree_deletes.rs"
  "crates/guards/src/scope_drift.rs"
  "crates/claude-hooks/src/scope_drift.rs"
  "crates/guards/src/link_worktree_rules.rs"
  "crates/claude-hooks/src/link_worktree_rules.rs"
  "crates/guards/src/handoff_threshold.rs"
  "crates/claude-hooks/src/handoff_threshold.rs"
  "crates/guards/src/long_session.rs"
  "crates/claude-hooks/src/long_session.rs"
  "crates/guards/src/spotlight_marker.rs"
  "crates/claude-hooks/src/spotlight_marker.rs"
  "crates/guards/src/skill_nudge.rs"
  "crates/claude-hooks/src/skill_nudge.rs"
  "crates/claude-hooks/src/work_status.rs"
  "crates/claude-hooks/src/orca_cleanup.rs"
  "crates/claude-hooks/src/relay.rs"
  "crates/guards/src/phase_router.rs"
  "crates/claude-hooks/src/phase_router.rs"
  "crates/guards/src/instincts.rs"
)
for f in "${FILES[@]}"; do
  mkdir -p "$BACKUP/$(dirname "$f")"
  cp "$ROOT/$f" "$BACKUP/$f"
done
# La copia di sicurezza si prende su tutti, il ripristino no: si rimettono a
# posto **solo i file che questo giro ha davvero mutato**. Prima ricopiava tutti
# e tredici a ogni mutante, e la copia era quella dell'avvio dello script — così
# un giro su una sezione cancellava il lavoro non committato sulle altre dodici.
# Il 17/08/2026 `mutants.sh successor` ha riportato al commit, sette volte in
# venti minuti, `claude-hooks/src/handoff_on_stop.rs` che un'altra sessione stava
# scrivendo: là hanno creduto a un Edit andato storto e riapplicato tre volte.
# I percorsi non contengono spazi, quindi una stringa separata da spazi basta e
# non serve un array (bash 3.2 su questa macchina, dove un array vuoto sotto
# `set -u` è un errore).
TOUCHED=""
restore_touched() { for f in $TOUCHED; do cp "$BACKUP/$f" "$ROOT/$f"; done; }

# ── La sentinella: questo banco muta i sorgenti IN SERVIZIO ──────────────────
#
# Chi legge un sorgente mentre un giro è in volo vede un mutante e lo prende per
# un difetto. Successo la notte del 24/08/2026: la sessione generale ha letto
# `instincts.rs` tre volte in pochi secondi e ha visto tre stati diversi, e una
# consegna ha riportato un rosso che non esisteva. Il rischio peggiore è l'altro:
# se il giro muore a metà, il mutante resta nel sorgente e il primo che ricompila
# se lo porta nel binario che i ganci eseguono a ogni strumento.
#
# Non è una serratura — nessuno qui dentro può obbligare le altre sessioni a
# chiedere permesso — ma è la cosa che rende la domanda rispondibile: chi vede un
# rosso strano guarda se questo file esiste. Una sentinella che sopravvive al
# proprio processo è la firma del giro morto a metà, e per questo il ripristino
# la toglie **solo** quando ha verificato che i sorgenti sono tornati.
SENTINELLA="$HOME/.claude/state/mutanti-in-corso-$$"
mkdir -p "$HOME/.claude/state" 2>/dev/null

announce_mutant() {
  {
    echo "banco dei mutanti in corso — i sorgenti qui sotto NON sono quelli in servizio"
    echo "processo: $$"
    echo "sezione: $WHICH"
    echo "dalle: $(date '+%Y-%m-%d %H:%M:%S')"
    echo "mutante: ${1:-nessuno ancora applicato}"
    echo "file: ${2:-nessuno ancora toccato}"
    echo "copia di sicurezza: $BACKUP"
    echo
    echo "Se stai leggendo un rosso su uno di questi file, non è tuo: aspetta la"
    echo "fine del giro. Se il processo qui sopra non esiste più, il giro è morto"
    echo "a metà e il sorgente può essere ancora mutato: confronta con la copia."
  } > "$SENTINELLA" 2>/dev/null
}

# Le sentinelle dei giri morti: si dicono all'avvio, perché è il momento in cui
# qualcuno sta per mutare di nuovo gli stessi file e cancellerebbe la prova.
for vecchia in "$HOME"/.claude/state/mutanti-in-corso-*; do
  [ -f "$vecchia" ] || continue
  altrui="${vecchia##*-}"
  [ "$altrui" = "$$" ] && continue
  if kill -0 "$altrui" 2>/dev/null; then
    echo "ATTENZIONE: un altro banco gira sugli stessi sorgenti (processo $altrui)." >&2
    echo "  I due giri si sovrascrivono i mutanti a vicenda: vedi $vecchia" >&2
  else
    echo "ATTENZIONE: $vecchia è di un processo che non esiste più." >&2
    echo "  Quel giro è morto a metà: i sorgenti che nomina possono essere ancora" >&2
    echo "  mutati, e il binario in servizio con loro. Leggilo prima di continuare." >&2
  fi
done

announce_mutant

# Ripristinare i sorgenti non basta: **il binario resta quello mutato**, ed è
# quello che i ganci di questa macchina eseguono davvero. Il 17/08/2026 questo
# script ha lasciato in produzione il binario dell'ultimo mutante, e il confronto
# lanciato subito dopo ha dato 318 divergenze che sembravano un difetto del
# porting. Ricompilare fa parte del ripristino, non è un di più.
restore() {
  restore_touched
  # Aver *chiamato* il ripristino non prova che sia riuscito. Un giro ucciso in
  # mezzo può lasciare un mutante addormentato, e quel difetto è invisibile ai
  # verdi: il 17/08/2026 `successor.rs` è rimasto col nome del gancio cambiato in
  # `successore`, che non rompe niente e fa sparire dai conteggi ogni riga che
  # quel gancio scrive. Qui si confronta, e se non torna lo si dice per nome.
  residuo=""
  for f in $TOUCHED; do
    cmp -s "$BACKUP/$f" "$ROOT/$f" \
      || { echo "ATTENZIONE: $f non e' tornato alla copia di sicurezza — mutante residuo"; residuo="$residuo $f"; }
  done
  (cd "$ROOT" && cargo build --release >/dev/null 2>&1) \
    || { echo "ATTENZIONE: ricompilazione fallita, il binario è ancora quello mutato"; residuo="$residuo (binario)"; }
  # La sentinella si toglie solo quando c'è la prova che i sorgenti sono tornati:
  # è l'unica cosa che, il giorno dopo, distingue «il banco ha finito» da «il
  # banco è morto col mutante dentro». Con un residuo resta, e cambia nome perché
  # il giro dopo non la scambi per un banco vivo.
  if [ -n "$residuo" ]; then
    mv "$SENTINELLA" "$HOME/.claude/state/mutanti-residuo-$$" 2>/dev/null
    echo "ATTENZIONE: resta $HOME/.claude/state/mutanti-residuo-$$ — i file:$residuo"
    # La copia di sicurezza NON si butta: senza, il confronto che dice cosa è
    # rimasto mutato non è più possibile da nessuno.
    echo "  copia di sicurezza tenuta in $BACKUP"
    return
  fi
  rm -f "$SENTINELLA"
  rm -rf "$BACKUP"
}
trap restore EXIT

survivors=0
broken=0
skipped=0
fallbacks=0
# Quanti mutanti questo giro ha davvero provato: la rete dietro il controllo sul
# nome, per la sezione che esiste ma non contiene nemmeno una chiamata.
attempted=0
COMPARE=""

# L'oracolo alternativo, per le guardie che un confronto col Python ancora non ce
# l'hanno: finché l'involucro non è portato non esiste un binario da mettere di
# fronte all'originale, e l'unica cosa che può accorgersi di un mutante è la
# batteria del modulo. Vuoto = si usa `COMPARE`, che resta la strada normale.
ORACOLO=""

# Vero se il comando dell'oracolo nomina uno script Python che sul disco non
# c'e' piu'.
#
# GLI SCRIPT `tools/compare-*.py` SONO STATI CANCELLATI con la fine del porting
# (`1db79a6`), e `python3` su un file inesistente esce **diverso da zero** — che
# qui e' esattamente la firma di «ucciso». Ogni mutante che passava dal confronto
# si dichiarava cosi' visto dalla batteria senza che nessuno lo guardasse: 10
# sezioni su 13 lo facevano, comprese le due che il confronto lo chiamavano da
# dentro `ORACOLO`. Una batteria che promuove se' stessa e' peggio di una che
# tace, ed e' il difetto contro cui questo script e' stato scritto.
oracolo_orfano() {
  local tok
  for tok in $1; do
    case "$tok" in
      *.py)
        case "$tok" in tools/*) ;; *) tok="tools/$tok" ;; esac
        [ -f "$ROOT/$tok" ] || return 0
        ;;
    esac
  done
  return 1
}

mutate() {
  local name="$1" file="$2" script="$3"
  attempted=$((attempted + 1))
  restore_touched
  case " $TOUCHED " in *" $file "*) ;; *) TOUCHED="$TOUCHED $file" ;; esac
  # La sentinella si aggiorna PRIMA di mutare, non dopo: se il giro muore fra le
  # due righe, il file che nomina è quello davvero sporco.
  announce_mutant "$name" "$file"
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
  # L'oracolo di questo mutante: quello dichiarato dalla sezione, o il confronto
  # col Python, che resta la strada normale dove lo script esiste ancora.
  local oracolo="$ORACOLO" nota=""
  [ -n "$oracolo" ] || oracolo="python3 tools/$COMPARE"
  if oracolo_orfano "$oracolo"; then
    # Il ripiego e' la batteria del workspace, ed e' un oracolo vero: un mutante
    # che le sopravvive e' un punto cieco esattamente come lo era per il
    # confronto. Si dichiara a ogni riga, perche' un giro letto fra sei mesi non
    # deve dedurre da solo chi ha dato il verdetto.
    oracolo="cargo test --release"
    nota="  (oracolo: cargo test — il confronto dichiarato non esiste piu')"
    fallbacks=$((fallbacks + 1))
  fi
  # Uno scartato NON e' un ucciso, e va contato a parte: il 17/08/2026 un file
  # rotto da un'altra sessione ha fatto fallire la compilazione dal terzo
  # mutante in poi, e questo script ha stampato «tutti i mutanti uccisi» su un
  # giro in cui nove non erano nemmeno stati provati. Un verdetto che si
  # confonde col silenzio e' peggio di nessun verdetto. Si compilano anche i
  # test, perche' un oracolo che li esegue fallirebbe qui per un errore di
  # sintassi e quello si conterebbe come successo.
  if ! (cd "$ROOT" && cargo test --release --no-run >/dev/null 2>&1); then
    echo "  SCARTATO    $name  <- non compila, quindi non e' stato provato"
    skipped=$((skipped + 1))
    return
  fi
  # `eval` senza virgolette di proposito: così un oracolo può portarsi dietro i
  # propri argomenti (`compare-live-rules.py 40`). Un confronto che gira su tutto
  # il corpus per ognuno degli otto mutanti costa ore, e un giro che nessuno
  # aspetta non viene lanciato — che è il modo in cui una batteria smette di
  # esistere. I nomi dei file non hanno spazi, quindi qui non si perde niente.
  if (cd "$ROOT" && eval "$oracolo" >/dev/null 2>&1); then
    echo "  SOPRAVVIVE  $name  <- la batteria non lo vede$nota"
    survivors=$((survivors + 1))
  else
    echo "  ucciso      $name$nota"
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

  # Un involucro (`timeout 30 …`) davanti alla chiusura di una scheda era la
  # scorciatoia più corta per aggirare la versione precedente. Il comando non si
  # scrive per esteso qui: il gancio vivo non guarda dentro le virgolette e
  # rifiuterebbe l'esecuzione di questo stesso script — misurato il 17/08/2026,
  # ed è lo stesso motivo per cui in `main.rs` lo smoke test è scritto a pezzi.
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

# ── live-rules ──────────────────────────────────────────────────────────────
if [ "$WHICH" = "live-rules" ] || [ "$WHICH" = "tutte" ]; then
  echo "mutanti su live_rules.rs:"
  # 40 file veri più i 14 costruiti: il corpus intero manda ogni giro a ore,
  # perché 308 dei 356 file passano dal verificatore Node — due volte a testa.
  COMPARE="compare-live-rules.py 40"
  G="crates/guards/src/live_rules.rs"
  W="crates/claude-hooks/src/live_rules.rs"

  # Il perimetro si allarga a ogni markdown: il gancio comincia a giudicare
  # documenti e competenze, che non sono regole e non hanno `paths:` — cioè
  # rimprovererebbe ogni file che tocca.
  mutate "perimetro esteso a ogni markdown" "$W" \
    's = s.replace("if !path.contains(\".claude/rules/\") || !path.ends_with(\".md\") {", "if !path.ends_with(\".md\") {", 1)'

  # Sparisce il caso che non si vede a occhio: `paths: ["**"]` sembra uno scope
  # e invece è l'assenza di scope, e passerebbe in silenzio.
  mutate "paths universale non riconosciuto" "$G" \
    's = s.replace("    if universal_paths(&front) {", "    if false {", 1)'

  # Le righe commentate tornano a contare come configurazione: un `# paths:`
  # dentro una spiegazione basterebbe a far tacere il controllo proprio sulle
  # regole che si vogliono trovare.
  mutate "commenti contati come paths" "$G" \
    's = s.replace("    let front = without_comments(front);\n    if !front.lines()", "    if !front.lines()", 1)'

  # Il filtro sul file appena scritto sparisce: il gancio rimprovera per il
  # debito altrui, ed è il difetto per cui un controllo viene spento.
  mutate "reperti altrui non filtrati" "$G" \
    's = s.replace("MARKERS.iter().any(|m| r.contains(m)) && r.contains(relative)", "MARKERS.iter().any(|m| r.contains(m))", 1)'

  # Il glob non viene più tagliato alla prima parte letterale: `src/**` viene
  # cercato alla lettera come cartella, non esiste, e ogni regola con un glob
  # normale viene dichiarata morta.
  #
  # Il primo mutante provato qui era «togli il `continue` sul prefisso vuoto», e
  # sopravviveva sempre: senza prefisso il codice cerca `repo.join("")`, che è
  # il repo stesso e c'è per definizione. Non era un punto cieco della batteria
  # ma un mutante che non muta niente — la distinzione va fatta, altrimenti si
  # aggiungono casi per un buco che non esiste.
  mutate "glob non tagliato al primo jolly" "$G" \
    's = s.replace("        if part.contains([\x27*\x27, \x27?\x27, \x27[\x27]) {\n            break;\n        }", "", 1)'

  # La carta non si legge più: senza repo il verdetto sui glob sparisce del
  # tutto, ed è la metà del gancio che riguarda le regole globali.
  mutate "carta dei repo ignorata" "$W" \
    's = s.replace("    let Ok(raw) = std::fs::read_to_string(CARTA) else {", "    if true { return Vec::new() }\n    let Ok(raw) = std::fs::read_to_string(CARTA) else {", 1)'

  # L'estensione corta non è più richiesta: ogni cartella citata fra apici
  # diventa un reperto, e il rapporto si riempie di falsi.
  mutate "estensione non richiesta" "$G" \
    's = s.replace("        if !has_short_extension(t) {\n            continue;\n        }", "", 1)'

  # Una regola globale passa dal verificatore Node, che è il difetto del
  # 14/08/2026: misurata sulla home, tutti i suoi glob risultano morti.
  mutate "regola globale mandata al verificatore" "$W" \
    's = s.replace("    let lines = if rules::is_global(path, &home) {", "    let lines = if false {", 1)'

  # I tre che seguono chiudono le lacune trovate da una revisione indipendente il
  # 17/08/2026, quando il confronto dava «0 differenze» e tre classi di ingresso
  # non erano nel corpus. Ognuno rimette esattamente la prima stesura.

  # Un file vuoto torna indistinguibile da uno illeggibile: zero byte non ha
  # frontmatter, quindi si carica in ogni sessione, e passava in silenzio.
  mutate "file vuoto scambiato per illeggibile" "$W" \
    's = s.replace("        if let Some(why) = rules::always_loaded(text) {", "        if let Some(why) = rules::always_loaded(text).filter(|_| !text.is_empty()) {", 1)'

  # Il guasto di lettura torna muto: senza la riga nel registro, un gancio morto
  # e uno contento sono la stessa cosa vista da fuori.
  mutate "guasto di lettura inghiottito" "$W" \
    's = s.replace("log_failure(path, &format!(\"non e", "let _ = (&format!(\"non e", 1)'

  # `is_global` smette di sciogliere e confronta solo alla lettera: una regola
  # raggiunta attraverso un collegamento smette di essere globale e finisce dal
  # verificatore Node con la home come radice — il difetto del 15/08/2026.
  #
  # Il mutante provato prima — *rimettere* il confronto letterale davanti allo
  # scioglimento — sopravviveva, e non era un punto cieco: se due percorsi sono
  # uguali alla lettera lo sono anche sciolti, quindi quel controllo anticipato
  # non cambia nessun esito. Un altro «mutante che non muta».
  mutate "is_global non scioglie i collegamenti" "$G" \
    's = s.replace("    match (parent.canonicalize(), target.canonicalize()) {", "    if true { return parent == target }\n    match (parent.canonicalize(), target.canonicalize()) {", 1)'
fi

# ── relay-evaluate ──────────────────────────────────────────────────────────
if [ "$WHICH" = "relay-evaluate" ] || [ "$WHICH" = "tutte" ]; then
  echo "mutanti su handoff.rs (la decisione della staffetta):"
  # Il numero non è una linea di base congelata: è ciò che il sorgente **non
  # mutato** produce, rimisurato con lo stesso comando prima di questo giro. Il
  # porto ha divergenze vere e note (arrotondamento della percentuale, memo
  # malformato); senza dirlo qui, ogni mutante risulterebbe ucciso dalle loro.
  COMPARE="compare-relay-evaluate.py --attese 10"
  H="crates/guards/src/handoff.rs"

  # L'ordine dei controlli È il comportamento. Col raffreddamento davanti, un
  # record che punta a un pannello morto non viene più buttato: resta lì finché
  # dura la tregua, e il registro delle sessioni si riempie di fantasmi.
  mutate "raffreddamento prima del pannello morto" "$H" \
    's = s.replace("    if !live.iter().any(|h| h == f.handle) {", "    if f.in_cooldown { return (Action::Skip, \"cooldown\".into()); }\n    if !live.iter().any(|h| h == f.handle) {", 1)'

  # La distinzione che vale il registro delle sessioni: elenco illeggibile
  # trattato come «sono morti tutti», cioè 276 giri che cancellano sessioni vive.
  mutate "elenco illeggibile letto come vuoto" "$H" \
    's = s.replace("    let Some(live) = f.live_handles else {\n        return (Action::Skip, \"elenco dei terminali illeggibile\".into());\n    };", "    let live: &[String] = f.live_handles.unwrap_or(&[]);", 1)'

  # Il freno del 17/08/2026: senza, la staffetta apre un secondo successore per
  # una sessione che ne ha già uno vivo.
  mutate "successore gia armato ignorato" "$H" \
    's = s.replace("if !f.armed_successor.is_empty() && live.iter().any(|h| h == f.armed_successor) {", "if false {", 1)'

  # Un carattere: la sessione esattamente sulla soglia smette di rigenerare.
  # È il caso che nessun transcript vero contiene, e per questo va fabbricato.
  mutate "soglia con <= invece di <" "$H" \
    's = s.replace("if f.used == 0 || f.used < t.require {", "if f.used == 0 || f.used <= t.require {", 1)'

  # L'opt-out scavalcato dal pannello morto: chi ha chiesto di non essere
  # toccato si vede cancellare il record lo stesso.
  mutate "opt-out spostato dopo l elenco" "$H" \
    's = s.replace("    if f.opted_out {\n        return (Action::Skip, \"opt-out\".into());\n    }\n", "", 1); s = s.replace("    if f.in_cooldown {", "    if f.opted_out {\n        return (Action::Skip, \"opt-out\".into());\n    }\n    if f.in_cooldown {", 1)'

  # Mezzo record basta: un handle vuoto non è più «non so di chi parliamo» ma
  # «non è fra i vivi», e la risposta diventa una cancellazione.
  mutate "record incompleto giudicato sulla sola sessione" "$H" \
    's = s.replace("if f.session.is_empty() || f.handle.is_empty() || f.worktree.is_empty() {", "if f.session.is_empty() {", 1)'

  # La guardia forte: si rigenera SOLO dopo che la consegna è su disco. Senza,
  # chiudere una sessione piena perde il lavoro non consegnato.
  mutate "consegna non piu richiesta" "$H" \
    's = s.replace("if !f.handoff_done {", "if false {", 1)'

  # Stessa azione, motivo diverso: senza transcript la misura è zero e la frase
  # diventa «sotto soglia». Un confronto che guardasse solo l'azione lo perde,
  # ed è la ragione per cui azione e motivo si contano separati.
  mutate "transcript assente non nominato" "$H" \
    's = s.replace("if !f.transcript_exists {", "if false {", 1)'

  # Un carattere dentro una riga di registro. Si vede solo se il corpus usa
  # handle veri: con `term_succ` il troncamento non si esercita mai.
  mutate "handle del successore troncato a 12" "$H" \
    's = s.replace(".chars().take(13)", ".chars().take(12)", 1)'

  # ATTESO SOPRAVVISSUTO, ed è la ragione per cui sta qui. `thresholds: None` è
  # uno stato che il Python non ha: lui le soglie le calcola sempre. Il confronto
  # non può raggiungere quel ramo, e un mutante che lo tocca deve sopravvivere —
  # dichiararlo è più utile che nascondere il buco togliendo il mutante.
  mutate "ramo senza soglie, irraggiungibile dal confronto" "$H" \
    's = s.replace("soglie non calcolabili", "soglie assenti", 1)'
fi

# ── relay-handoff ───────────────────────────────────────────────────────────
if [ "$WHICH" = "relay-handoff" ] || [ "$WHICH" = "tutte" ]; then
  echo "mutanti su relay.rs (quale consegna eredita il successore):"
  # Il caso che li vede tutti e cinque e' «CAMPO: la stessa, dentro un albero di
  # lavoro», che sta solo nei casi di campo: senza `--all` questo gruppo
  # dichiarerebbe uccisi mutanti che nessuno ha guardato.
  COMPARE="compare-relay-regenerate.py --all"
  R="crates/claude-hooks/src/relay.rs"

  # Il difetto del 18/08/2026: la memoria di un albero di lavoro sta sotto il
  # repo che lo ospita, e senza questa risalita il restringimento al progetto non
  # si applica mai proprio dove serve — 5 rigenerazioni su 11.
  mutate "l albero di lavoro non risale al repo" "$R" \
    's = s.replace("    for candidate in [cwd, canonical.as_str()] {", "    for candidate in [cwd] {", 1)'

  # Il nome non dice se un documento e' una consegna: la skill scrive uno slug
  # tematico. Col criterio sul nome vince la consegna del giorno prima.
  mutate "riconosce la consegna dal solo nome" "$R" \
    's = s.replace("        if guards::successor::is_handoff_doc(path, text.as_deref()) {", "        if guards::successor::name_says_handoff(path) {", 1)'

  # Senza conferma, la memoria piu' recente qualunque diventa il mandato del
  # successore — appunti compresi.
  mutate "prende il piu recente senza confermare" "$R" \
    's = s.replace("""        if guards::successor::is_handoff_doc(path, text.as_deref()) {
            return path.clone();
        }""", "        return path.clone();", 1)'

  # Il ripiego su tutti i progetti col cwd noto e' esattamente cio' che ha dato a
  # una sessione sulla suite il mandato della configurazione.
  mutate "ripiega su tutti anche col cwd noto" "$R" \
    's = s.replace("    Vec::new()\n}", "    vec![base]\n}", 1)'

  # L ordine e il comportamento: per data crescente si confermerebbe la consegna
  # piu' vecchia, e il tetto dei candidati taglierebbe via proprio le recenti.
  mutate "scorre per data crescente" "$R" \
    's = s.replace("    found.sort_by(|a, b| b.cmp(a));", "    found.sort();", 1)'

  # Il progresso della generazione uscente: il §3 lo pretende nel registro, ed e'
  # l'unico dato che distinguerebbe una catena che lavora da una che gira a vuoto.
  mutate "conta solo le Write, non le altre scritture" "$R" \
    's = s.replace("for tool in [r#\"\"name\":\"Write\"\"#, r#\"\"name\":\"Edit\"\"#, r#\"\"name\":\"MultiEdit\"\"#] {", "for tool in [r#\"\"name\":\"Write\"\"#] {", 1)'

  mutate "il progresso non si misura" "$R" \
    's = s.replace("    let (turns, writes) = progress(&rec.transcript);", "    let (turns, writes) = (0u64, 0u64);", 1)'
fi

# ── successor ───────────────────────────────────────────────────────────────
if [ "$WHICH" = "successor" ] || [ "$WHICH" = "tutte" ]; then
  echo "mutanti su successor.rs (l'involucro che arma):"
  COMPARE="compare-successor.py"

  # La valvola che fissa l'ora, portata il 18/08/2026 perche' il freno orario sta
  # PRIMA dei tetti e di notte li copre. Se la legge un lato solo, i due porti
  # rispondono diverso a parita' di ambiente e nessuno se ne accorge.
  mutate "il porto ignora l ora fissata" "crates/claude-hooks/src/successor.rs" \
    's = s.replace("""    if let Ok(fixed) = std::env::var("CONSEGNA_ORA") {
        if let Ok(h) = fixed.parse::<u32>() {
            return h;
        }
    }
""", "", 1)'
  A="crates/claude-hooks/src/successor.rs"

  # IL DIFETTO VERO, rimesso: il porto decide bene e non registra niente. È lo
  # stato in cui il gancio è vissuto dalle 10:57 del 17/08/2026, e il confronto
  # di allora — solo `stdout`, HOME vera — restava verde.
  mutate "registro dei freni rimosso" "$A" \
    's = s.replace("    journal::record(\"consegna-arma-successore\", \"ferma\", reason, &extra);\n", "", 1)'

  # Il nome del gancio cambia: le righe ci sono ma nessuna misura le trova,
  # perché tutte le letture filtrano per quel nome.
  mutate "nome del gancio cambiato" "$A" \
    's = s.replace("journal::record(\"consegna-arma-successore\", \"ferma\", reason, &extra);", "journal::record(\"successore\", \"ferma\", reason, &extra);", 1)'

  # Le chiavi dei numeri cambiano nome: chi somma `vive` e `pannelli` legge zero
  # su righe formalmente valide.
  mutate "chiavi dei conteggi rinominate" "$A" \
    's = s.replace("                \"vive\",", "                \"sessioni\",", 1)'

  # ATTESO SOPRAVVISSUTO, il primo dei due. `origin` è il campo che distingue le
  # due strade, ma questo confronto esercita solo il gancio PostToolUse, che
  # passa sempre `scrittura`: l altro innesco è lo Stop, il cui involucro è
  # ancora il Python. Il mutante morirà da sé quando quel porto arriverà —
  # tenerlo qui è il promemoria che oggi quella metà non ha rete.
  mutate "origine costante" "$A" \
    's = s.replace("(\"origine\", Field::Text(origin.to_string())),", "(\"origine\", Field::Text(\"scrittura\".to_string())),")'

  # Il conteggio diventa testo: due righe identiche a vedersi, e una somma che
  # non si può più fare. È il difetto che `journal::Field` esiste per non rifare.
  mutate "conteggio scritto come testo" "$A" \
    's = s.replace("(\"pannelli\", Field::Number(facts.panes_here.unwrap_or(0) as i64)),", "(\"pannelli\", Field::Text(facts.panes_here.unwrap_or(0).to_string())),", 1)'

  # ATTESO SOPRAVVISSUTO. Nessun caso del confronto arriva al ramo che apre: là
  # in fondo c è una tab vera e una sessione Claude che parte trenta secondi
  # dopo, e uno strumento che genera sessioni non lo rilancia nessuno. La riga
  # `apre`/`fallisce` resta senza rete automatica, ed è meglio dirlo qui.
  mutate "registro dell apertura rimosso" "$A" \
    's = s.replace("            journal::record(\n                \"consegna-arma-successore\",\n                if ok { \"apre\" } else { \"fallisce\" },", "            #[allow(unused)] let _skip = (\n                \"consegna-arma-successore\",\n                if ok { \"apre\" } else { \"fallisce\" },", 1)'

  COMPARE=""
fi

# ── session-cap ─────────────────────────────────────────────────────────────
if [ "$WHICH" = "session-cap" ] || [ "$WHICH" = "tutte" ]; then
  echo "mutanti sul tetto alle sessioni (il giudizio puro):"
  # L'oracolo e' la batteria del modulo: il giudizio non tocca la macchina, e
  # provarlo contro il Python costerebbe un giro di confronto per ogni mutante
  # senza vedere niente di piu'.
  ORACOLO="cargo test -p guards handoff::tests"
  H="crates/guards/src/handoff.rs"

  # IL MUTANTE PIU' IMPORTANTE, ed e' l'errore che chi legge dopo fara' da se':
  # il tetto morde anche chi sostituisce. Una sessione piena che si rigenera e' a
  # saldo zero, e bloccarla la blocca proprio quando serve di piu'.
  mutate "il tetto morde anche le sostituzioni" "$H" \
    's = s.replace("    if f.delta != SessionDelta::Adds {", "    if f.delta == SessionDelta::None {", 1)'

  # Il fail-open si capovolge: chi non sa contare blocca tutto. E' il verso in
  # cui un freno smette di frenare le sessioni e comincia a frenare il lavoro.
  mutate "conteggio fallito diventa infinito" "$H" \
    's = s.replace("    let live = f.live?;", "    let live = f.live.unwrap_or(usize::MAX);", 1)'

  # Il confine slitta di uno: venti sessioni passano, e la soglia misurata non e'
  # piu' quella che il documento dichiara.
  mutate "confine spostato di uno" "$H" \
    's = s.replace("    if live < f.cap {", "    if live <= f.cap {", 1)'

  # La parola torna a essere una sottostringa: ogni comando eseguito dentro
  # `~/.claude` contiene «claude», e il freno bloccherebbe il lavoro normale.
  mutate "claude cercato come sottostringa" "$H" \
    's = s.replace("(with_agent || word_at(&c, \"claude\"))", "(with_agent || c.contains(\"claude\"))", 1)'

  # Ogni `terminal create` conta come sessione: rientrano le due shell che
  # `--setup run` lascia in ogni albero nuovo, che non sono sessioni.
  mutate "ogni scheda conta come sessione" "$H" \
    's = s.replace("(c.contains(\"terminal create\") && (with_agent || word_at(&c, \"claude\")))", "c.contains(\"terminal create\")", 1)'

  # La dichiarazione di sostituzione viene ignorata: tutto diventa un'aggiunta.
  mutate "dichiarazione di sostituzione ignorata" "$H" \
    's = s.replace("    if c.contains(REPLACEMENT_MARK) {", "    if false {", 1)'

  echo "mutanti sul tetto alle sessioni (l'involucro che conta e registra):"
  ORACOLO=""
  COMPARE="compare-successor.py"
  A="crates/claude-hooks/src/successor.rs"

  # La valvola non spegne piu' niente: un freno che non si puo' togliere si
  # aggira, e allora lo si toglie del tutto.
  mutate "valvola del tetto ignorata" "$A" \
    's = s.replace("    let mode = hook_io::Mode::from_env(\"SESSION_CAP_GUARD\");\n    if mode == hook_io::Mode::Off {\n        return 0;\n    }\n", "    let mode = hook_io::Mode::from_env(\"SESSION_CAP_GUARD\");\n", 1)'

  # Il rifiuto passa dal canale sbagliato: `Warn` scrive su stderr ed esce 0, e
  # il comando viene eseguito lo stesso. Un freno che non nega non e' un freno.
  mutate "diniego declassato ad avviso" "$A" \
    's = s.replace("        _ => hook_io::Decision::Deny(message),", "        _ => hook_io::Decision::Warn(message),", 1)'

  # Il registro del tetto sparisce: la decisione resta giusta e nessuno puo' piu'
  # sapere quante sessioni si aprono in un giorno — cioe' non si potra' ritarare
  # la soglia, che e' l'errore gia' pagato dalla regola sulla pressione.
  mutate "registro del tetto rimosso" "$A" \
    's = s.replace("    journal::record(\n        \"session-cap\",", "    #[allow(unused)] let _skip = (\n        \"session-cap\",", 1)'

  # Il conteggio torna a contare per sottostringa: righe come
  # `chrome-native-host` non lo cambiano oggi, ma `claude-hooks` si'.
  mutate "conteggio dei processi non ancorato" "$A" \
    's = s.replace(".filter(|l| l.trim() == \"claude\")", ".filter(|l| l.contains(\"claude\"))", 1)'

  COMPARE=""
fi

# ── handoff-on-stop ─────────────────────────────────────────────────────────
if [ "$WHICH" = "handoff-on-stop" ] || [ "$WHICH" = "tutte" ]; then
  echo "mutanti su handoff_on_stop.rs (la consegna a fine turno):"
  # Qui l'oracolo è la batteria del modulo, non un confronto: l'involucro è
  # ancora il Python, quindi non c'è un binario da mettere di fronte.
  ORACOLO="cargo test -p guards handoff_on_stop"
  S="crates/guards/src/handoff_on_stop.rs"

  # L'anti-loop primario sparisce: il presidio riblocca dentro la propria catena
  # e incastra la sessione per sempre — più pericoloso della compattazione che
  # evita.
  mutate "anti-loop primario rimosso" "$S" \
    's = s.replace("    if f.stop_hook_active {\n        return Decision::Pass;\n    }\n", "", 1)'

  # La resa non arriva mai: stesso stallo, per l altra strada.
  mutate "resa spostata di uno" "$S" \
    's = s.replace("if f.stop_blocks_so_far >= STOP_CAP {", "if f.stop_blocks_so_far > STOP_CAP {", 1)'

  # Il congedo torna verde e inerte: la consegna c e, ma nessuno arma il
  # successore e nessuno chiude la scheda. E il difetto del 16/08/2026.
  mutate "consegna valida che non congeda" "$S" \
    's = s.replace("        return Decision::Settle;", "        return Decision::Pass;", 1)'

  # Il secondo motivo sparisce del tutto: e la sessione e1fa3600, settima
  # ripartenza a 129k token, che non verrebbe mai fermata.
  mutate "motivo delle ripartenze rimosso" "$S" \
    's = s.replace("    if !over_context && f.restarts < f.restart_cap {", "    if !over_context {", 1)'

  # Un carattere sul tetto delle ripartenze: chi sta esattamente sulla soglia
  # smette di essere fermato.
  mutate "tetto delle ripartenze spostato di uno" "$S" \
    's = s.replace("f.restarts < f.restart_cap {", "f.restarts <= f.restart_cap {", 1)'

  # Un carattere sull obbligo di contesto, dall altra parte.
  mutate "obbligo di contesto spostato di uno" "$S" \
    's = s.replace("f.used >= f.thresholds.require;", "f.used > f.thresholds.require;", 1)'

  # Le due teste si scambiano: a chi ha compattato sette volte si dice che il
  # contesto e pieno, quando e basso proprio per quello.
  mutate "teste del messaggio invertite" "$S" \
    's = s.replace("let head = if f.restarts >= f.restart_cap {", "let head = if f.restarts < f.restart_cap {", 1)'

  # Il tetto torna scritto a mano: alzarlo dall ambiente non cambierebbe piu il
  # testo, e il messaggio direbbe un numero falso.
  mutate "tetto scritto a mano nel messaggio" "$S" \
    's = s.replace("            f.restarts, f.restart_cap\n", "            f.restarts, 5\n", 1)'

  # Senza trascrizione si giudica lo stesso, cioe su una misura che non esiste.
  mutate "trascrizione assente ignorata" "$S" \
    's = s.replace("    if !f.has_transcript {", "    if false {", 1)'

  # La misura assente torna a valere come contesto vuoto: con soglie degeneri il
  # presidio blocca una sessione di cui non sa niente.
  mutate "misura assente trattata come misura" "$S" \
    's = s.replace("let over_context = f.used > 0 && f.used >=", "let over_context = f.used >=", 1)'

  ORACOLO=""
fi

# ── handoff-on-stop, ma con l'oracolo del confronto ─────────────────────────
# Gli stessi mutanti visti dall'altra parte. Serve perché i due oracoli vedono
# cose diverse: la batteria conosce le quattro varianti della decisione, il
# confronto conosce solo ciò che l'originale espone — un codice d'uscita e il
# testo su stderr. Un mutante che muore di là e sopravvive di qua dice
# esattamente quanto il confronto **non** copre, e va saputo prima di adottare
# il porto, non dopo.
if [ "$WHICH" = "handoff-on-stop-confronto" ]; then
  echo "mutanti su handoff_on_stop.rs, oracolo = confronto col Python:"
  ORACOLO="cargo build --release >/dev/null 2>&1 && python3 tools/compare-handoff-on-stop.py"
  S="crates/guards/src/handoff_on_stop.rs"

  mutate "anti-loop primario rimosso" "$S" \
    's = s.replace("    if f.stop_hook_active {\n        return Decision::Pass;\n    }\n", "", 1)'
  mutate "resa spostata di uno" "$S" \
    's = s.replace("if f.stop_blocks_so_far >= STOP_CAP {", "if f.stop_blocks_so_far > STOP_CAP {", 1)'
  mutate "motivo delle ripartenze rimosso" "$S" \
    's = s.replace("    if !over_context && f.restarts < f.restart_cap {", "    if !over_context {", 1)'
  mutate "tetto delle ripartenze spostato di uno" "$S" \
    's = s.replace("f.restarts < f.restart_cap {", "f.restarts <= f.restart_cap {", 1)'
  mutate "obbligo di contesto spostato di uno" "$S" \
    's = s.replace("f.used >= f.thresholds.require;", "f.used > f.thresholds.require;", 1)'
  mutate "teste del messaggio invertite" "$S" \
    's = s.replace("let head = if f.restarts >= f.restart_cap {", "let head = if f.restarts < f.restart_cap {", 1)'
  mutate "tetto scritto a mano nel messaggio" "$S" \
    's = s.replace("            f.restarts, f.restart_cap\n", "            f.restarts, 5\n", 1)'
  # ATTESO SOPRAVVISSUTO, il primo dei due. Senza trascrizione i fatti che ne
  # derivano — zero token, zero ripartenze, budget di ripiego — portano a `Pass`
  # da soli: il ramo è una cintura sul contratto dei `Facts`, non un freno che
  # cambi un esito raggiungibile da qui. A distinguerlo serve una combinazione
  # che l'involucro non produrrebbe mai (nessuna trascrizione e 480k token in
  # contesto), e quella vive nella batteria del modulo.
  mutate "trascrizione assente ignorata" "$S" \
    's = s.replace("    if !f.has_transcript {", "    if false {", 1)'

  # ATTESO SOPRAVVISSUTO, il secondo, ed è la ragione per cui sta qui invece di
  # essere tolto.
  # `Settle` e `Pass` sono lo stesso codice d'uscita e lo stesso silenzio: la
  # differenza sta negli effetti — armare il successore e chiudere la propria
  # scheda — che vivono nell'involucro non ancora portato. Finché quel pezzo è il
  # Python, nessun confronto può vedere questo mutante, e la sua rete è la
  # batteria del modulo (`bash tools/mutants.sh handoff-on-stop`).
  mutate "consegna valida che non congeda" "$S" \
    's = s.replace("        return Decision::Settle;", "        return Decision::Pass;", 1)'

  ORACOLO=""
fi

# ── handoff-on-stop, l'involucro ────────────────────────────────────────────
# Il gancio intero contro il Python: uscita, testo su stderr e riga di registro.
if [ "$WHICH" = "handoff-on-stop-involucro" ]; then
  echo "mutanti su claude-hooks/handoff_on_stop.rs (l'involucro dello Stop):"
  COMPARE="compare-handoff-on-stop.py"
  I="crates/claude-hooks/src/handoff_on_stop.rs"

  # Il presidio decide e non lascia traccia: e il difetto trovato oggi nel
  # gemello, rimesso qui per vedere se questo confronto lo prende.
  mutate "registro del blocco rimosso" "$I" \
    's = s.replace("            journal::record(\n                \"consegna-allo-stop\",\n                \"blocca\",", "            #[allow(unused)] let _skip = (\n                \"consegna-allo-stop\",\n                \"blocca\",", 1)'

  # La resa smette di dirlo: da fuori, arrendersi e non essere mai arrivati li
  # diventano la stessa cosa.
  mutate "registro della resa rimosso" "$I" \
    's = s.replace("            journal::record(\n                \"consegna-allo-stop\",\n                \"passa\",", "            #[allow(unused)] let _skip = (\n                \"consegna-allo-stop\",\n                \"passa\",", 1)'

  # Il contatore non avanza: il gancio non si arrende mai, e la sessione resta
  # incastrata a forzare per sempre.
  mutate "contatore delle forzature fermo" "$I" \
    's = s.replace("            let n = stop_blocks(&session, true);\n            journal::record(\n                \"consegna-allo-stop\",\n                \"blocca\",", "            let n = stop_blocks(&session, false);\n            journal::record(\n                \"consegna-allo-stop\",\n                \"blocca\",", 1)'

  # Un numero fuori di uno nella riga del blocco: si vede solo confrontando il
  # registro, mai l uscita.
  mutate "blocchi contati senza se stesso" "$I" \
    's = s.replace("(\"blocchi\", Field::Number((n + 1) as i64)),", "(\"blocchi\", Field::Number(n as i64)),", 1)'

  # Il tetto delle ripartenze torna costante: alzarlo dall ambiente non serve
  # piu a niente, ed e la valvola che i due ganci condividono.
  mutate "tetto delle ripartenze non piu dall ambiente" "$I" \
    's = s.replace("    let restart_cap = std::env::var(\"RIPARTENZA_TETTO_COMPATT\")\n        .ok()\n        .and_then(|v| v.parse::<u32>().ok())\n        .unwrap_or(RESTART_CAP_DEFAULT);", "    let restart_cap = RESTART_CAP_DEFAULT;", 1)'

  # Il contatore si incrementa gia mentre si raccolgono i fatti: ogni fine turno
  # sotto soglia consuma una forzatura, e dopo tre il presidio e disarmato senza
  # aver mai parlato.
  mutate "contatore incrementato troppo presto" "$I" \
    's = s.replace("stop_blocks_so_far: stop_blocks(&session, false),", "stop_blocks_so_far: stop_blocks(&session, true),", 1)'

  COMPARE=""
fi

# ── worktree-deletes ────────────────────────────────────────────────────────
# Il gancio che CONCEDE permessi: qui un mutante sopravvissuto non e un
# messaggio sbagliato, e una cancellazione autorizzata che non doveva esserlo.
if [ "$WHICH" = "worktree-deletes" ] || [ "$WHICH" = "tutte" ]; then
  echo "mutanti su worktree_deletes.rs (le cancellazioni concesse):"
  COMPARE="compare-worktree-deletes.py"
  G="crates/guards/src/worktree_deletes.rs"
  I="crates/claude-hooks/src/worktree_deletes.rs"

  # La profondita minima sparisce: un worktree intero, o un repo intero, o la
  # radice di tutte le copie diventano cancellabili senza domande.
  mutate "profondita minima azzerata" "$G" \
    's = s.replace("pub const MIN_DEPTH: usize = 3;", "pub const MIN_DEPTH: usize = 0;", 1)'

  # Il confine sul prefisso cade: basta che il nome cominci allo stesso modo, e
  # `workspacesXX` passa.
  mutate "confine del prefisso allentato" "$G" \
    's = s.replace("let prefisso = format!(\"{root}/\");", "let prefisso = root.clone();", 1)'

  # I collegamenti smettono di contare: cancellare *attraverso* un collegamento
  # a `node_modules` del canonico diventa autorizzato.
  mutate "collegamenti non piu seguiti" "$G" \
    's = s.replace("        if is_link(&current) {\n            return false;\n        }\n", "", 1)'

  # La risalita torna ammessa: un comando puo scavalcare la propria copia di
  # lavoro ed entrare in quella di un altro giro.
  mutate "risalita ammessa" "$G" \
    's = s.replace("    if path.split(\x27/\x27).any(|p| p == \"..\") {\n        return false;\n    }\n", "", 1)'

  # Basta un bersaglio dentro invece di tutti: `rm -rf <dentro> /etc/passwd`
  # verrebbe concesso.
  mutate "basta un bersaglio dentro" "$G" \
    's = s.replace("        .find(|t| !inside_worktree(t, f.workspaces, f.is_link))", "        .find(|t| false && !inside_worktree(t, f.workspaces, f.is_link))", 1)'

  # ATTESO SOPRAVVISSUTO, e l originale lo dichiara come «ridondanza voluta»:
  # `$(...)` non si espande, quindi il pezzo resta non-assoluto e cade comunque
  # sul controllo del prefisso. Resta scritto perche su un gancio che concede
  # permessi la prima linea dev essere esplicita — se un domani il controllo che
  # lo copre cambia, questo tiene. Il mutante sta qui per non far sembrare quel
  # ramo provato quando non lo e.
  mutate "sostituzioni non piu rifiutate" "$G" \
    's = s.replace("    if f.command.contains(\"$(\") || f.command.contains(\x27`\x27) {", "    if false {", 1)'

  # Un `rm` illeggibile smette di fermare tutto: la riga si salta in silenzio e
  # si giudica sul resto.
  mutate "rm illeggibile ignorato" "$G" \
    's = s.replace("                if bare_rm().is_match(&line) {\n                    return Verdict::Abstain(\"un rm che non so leggere per intero\".into());\n                }\n", "", 1)'

  # Il comando non si spezza piu sui separatori: si guarda solo il primo pezzo,
  # e cio che segue `&&` passa inosservato.
  mutate "separatori non spezzano" "$G" \
    's = s.replace("    for line in split_commands(f.command) {", "    for line in [f.command.to_string()] {", 1)'

  # Una variabile non definita smette di essere un ostacolo e diventa stringa
  # vuota: il bersaglio cambia significato senza che nessuno lo dica.
  mutate "variabile ignota trattata come vuota" "$G" \
    's = s.replace("        let value = variables.get(name)?;", "        let vuoto = String::new();\n        let value = variables.get(name).unwrap_or(&vuoto);", 1)'

  # Il perimetro si allarga a ogni strumento, non solo a Bash.
  mutate "perimetro esteso a ogni strumento" "$I" \
    's = s.replace("    if data.get(\"tool_name\").and_then(|v| v.as_str()) != Some(\"Bash\") {\n        return 0;\n    }\n", "", 1)'

  # Il registro delle concessioni sparisce: da fuori, «ha concesso» e «non e
  # partito» tornano indistinguibili.
  mutate "registro delle concessioni rimosso" "$I" \
    's = s.replace("    log_grant(&data, reason, command);", "", 1)'

  COMPARE=""
fi

# ── scope-drift ─────────────────────────────────────────────────────────────
if [ "$WHICH" = "scope-drift" ] || [ "$WHICH" = "tutte" ]; then
  echo "mutanti su scope_drift.rs:"
  COMPARE="compare-scope-drift.py"
  G="crates/guards/src/scope_drift.rs"
  I="crates/claude-hooks/src/scope_drift.rs"

  # La soglia scende a due: l'avviso arriverebbe su ogni sessione che tocca un
  # codice e la sua configurazione, cioe' quasi tutte. Un avviso che parla
  # sempre non lo legge piu' nessuno, ed e' il modo in cui questo gancio
  # smetterebbe di esistere restando acceso.
  mutate "soglia a due aree" "$G" \
    's = s.replace("pub const THRESHOLD: usize = 3;", "pub const THRESHOLD: usize = 2;", 1)'

  # Il confine della soglia: con `>` la terza area non basta piu' e il gancio
  # tace fino alla quarta.
  mutate "soglia stretta a maggiore-di" "$G" \
    's = s.replace("after.len() >= THRESHOLD", "after.len() > THRESHOLD", 1)'

  # L avviso torna a ogni area nuova: e' la promessa «parla una volta sola».
  mutate "detto ignorato" "$G" \
    's = s.replace("let speak = after.len() >= THRESHOLD && !already_said;", "let speak = after.len() >= THRESHOLD;", 1)'

  # Il ramo «niente di nuovo» sparisce: si riscrive lo stato a ogni chiamata, e
  # su una sessione gia' a tre aree non ancora avvisata si ricomincia a parlare.
  mutate "ristagno non riconosciuto" "$G" \
    's = s.replace("    if after == *seen {", "    if false {", 1)'

  # Il flag non si conserva: dopo il primo avviso lo stato tornerebbe a «mai
  # detto», e l area successiva farebbe parlare di nuovo.
  mutate "flag detto non conservato" "$I" \
    's = s.replace("\"detto\": already_said || esito.speak,", "\"detto\": esito.speak,", 1)'

  # I trascritti tornano a contare come configurazione: chiunque rilegga i
  # propri registri si accende la terza area senza aver cambiato mestiere.
  mutate "trascritti contati come configurazione" "$G" \
    's = s.replace("        if !rest.starts_with(\"projects\") && !rest.starts_with(\"history\") {", "        if true {", 1)'

  # Il confine di parola sparisce: `suiteXX` diventa `suite`.
  mutate "confine di parola rimosso" "$G" \
    's = s.replace(r"/gyver/work/suite\b|workspaces/suite/", "/gyver/work/suite|workspaces/suite/", 1)'

  # Si guardano solo due campi: un percorso arrivato in `path` o in
  # `notebook_path` smette di contare.
  mutate "meta dei campi non guardati" "$G" \
    's = s.replace(r"""pub const FIELDS: [&str; 4] = ["command", "file_path", "path", "notebook_path"];""", r"""pub const FIELDS: [&str; 4] = ["command", "file_path", "command", "file_path"];""", 1)'

  # Il filtro che rende gratuito il caso normale: senza, il gancio tocca il
  # disco a ogni chiamata a strumento. E' l unico mutante che il confronto vede
  # solo grazie al campo «cartella dello stato».
  mutate "uscita anticipata senza aree rimossa" "$I" \
    's = s.replace("    if fresh.is_empty() {\n        return 0;\n    }\n", "", 1)'

  # Uno stato inutilizzabile smette di fermare il gancio: dove l originale muore
  # in silenzio senza toccare niente, il porto riscriverebbe il file.
  mutate "stato non-oggetto accettato" "$I" \
    's = s.replace("    if !v.is_object() {\n        return None;\n    }\n", "", 1)'

  # La valvola non si legge piu': `SCOPE_DRIFT=off` smette di spegnere.
  mutate "valvola rimossa" "$I" \
    's = s.replace("    if std::env::var(\"SCOPE_DRIFT\").as_deref() == Ok(\"off\") {\n        return 0;\n    }\n", "", 1)'

  # Il registro sparisce: da fuori, «ha avvisato» e «non e' partito» tornano
  # indistinguibili — che e' esattamente lo stato in cui si trova l originale.
  mutate "registro rimosso" "$I" \
    's = s.replace("    log_notice(&session, &esito.areas);", "", 1)'

  # Il registro nella forma del JavaScript invece che in quella del Python:
  # stessa informazione, byte diversi, e due formati mescolati nello stesso
  # archivio storico senza che nessuno se ne accorga. E' la trappola gia' pagata
  # una volta da `journal::record`.
  # La riga del registro perde lo spazio dopo i due punti, cioe' prende la forma
  # che scrive `journal::record`. Stessa informazione, byte diversi, e due
  # formati mescolati nello stesso archivio storico senza che nessuno se ne
  # accorga: e' la trappola gia' pagata una volta da `journal::record`, e in
  # `ganci.jsonl` le due forme convivono ancora (2093 righe contro 3517).
  mutate "registro nella forma senza spazi" "$I" \
    's = s.replace(r"""    let dir = state_dir();""", r"""    let line = line.replace("\": \"", "\":\"");
    let dir = state_dir();""", 1)'

  # Lo stato scritto con serde invece che con `json.dumps`: i separatori
  # perdono lo spazio, e il file diverge pur dicendo la stessa cosa.
  mutate "stato nella forma di serde" "$I" \
    's = s.replace("""        hook_io::python_json::dumps(&serde_json::json!({
            "aree": esito.areas.iter().cloned().collect::<Vec<_>>(),
            "detto": already_said || esito.speak,
        })),""", """        serde_json::to_string(&serde_json::json!({
            "aree": esito.areas.iter().cloned().collect::<Vec<_>>(),
            "detto": already_said || esito.speak,
        }))
        .unwrap_or_default(),""", 1)'

  # L ordine delle chiavi dello stato si inverte. Compila, dice la stessa cosa,
  # e cambia i byte: e' il mutante che prova che il confronto guarda il file
  # come TESTO. Rianalizzandolo in un oggetto, questo sopravviverebbe.
  mutate "ordine delle chiavi dello stato invertito" "$I" \
    's = s.replace("""            "aree": esito.areas.iter().cloned().collect::<Vec<_>>(),
            "detto": already_said || esito.speak,""", """            "detto": already_said || esito.speak,
            "aree": esito.areas.iter().cloned().collect::<Vec<_>>(),""", 1)'

  # La sessione non si tronca piu' a otto caratteri: il registro cambia forma.
  mutate "sessione non troncata nel registro" "$I" \
    's = s.replace("session.chars().take(8).collect::<String>()", "session.to_string()", 1)'

  COMPARE=""
fi

# ── link-worktree-rules ─────────────────────────────────────────────────────
if [ "$WHICH" = "link-worktree-rules" ] || [ "$WHICH" = "tutte" ]; then
  echo "mutanti su link_worktree_rules.rs:"
  # `--veloce` salta il caso del tempo massimo: quaranta secondi per mutante,
  # e questa sezione ne ha tredici. Il giro completo lo fa chi confronta.
  COMPARE="compare-link-worktree-rules.py --veloce"
  G="crates/guards/src/link_worktree_rules.rs"
  I="crates/claude-hooks/src/link_worktree_rules.rs"

  # Il primo dei due momenti sparisce: le copie nate mentre non c'eravamo
  # restano scoperte, ed e' meta' del caso.
  mutate "SessionStart non lancia piu" "$G" \
    's = s.replace("""    if event == "SessionStart" {
        return true;
    }
""", "", 1)'

  # La guardia sullo strumento cade: il gancio guarda il comando di qualunque
  # chiamata, e parte su un payload che non ha mai eseguito niente in una shell.
  mutate "tool_name non guardato" "$G" \
    's = s.replace("""    if payload.get("tool_name").and_then(Value::as_str) != Some("Bash") {
        return false;
    }
""", "", 1)'

  # Il confine di parola in coda: `git worktree added` tornerebbe a contare come
  # una creazione, e la cura girerebbe su un comando che non crea niente.
  mutate "confine di parola in coda rimosso" "$G" \
    's = s.replace(r"worktree\s+add)\b", r"worktree\s+add)", 1)'

  # Lo script cercato altrove: il gancio non trova piu' niente e tace per
  # sempre, che e' esattamente lo stato da cui questo gancio ci ha tirati fuori.
  mutate "percorso dello script cambiato" "$I" \
    's = s.replace(".claude/scripts/link-worktree-rules.sh", ".claude/scripts/link-worktree-rules.bash", 1)'

  # La cura non si lancia piu': si conta su un'uscita vuota, quindi si tace
  # sempre. Da fuori sembra un gancio contento, e il confronto se ne accorge
  # solo perche' guarda **i lanci**, non il solo avviso.
  mutate "lo script non si lancia" "$I" \
    's = s.replace("""    let Some(uscita) = link(&script) else {
        return 0;
    };""", "    let uscita = String::new();", 1)'

  # Si conta ogni riga invece delle sole collegate: il gancio parla anche quando
  # non c'era niente da fare, cioe' quasi sempre.
  mutate "si contano tutte le righe" "$G" \
    's = s.replace(".filter(|r| strip(r).starts_with(\"collegato\"))", ".filter(|r| !strip(r).is_empty())", 1)'

  # `lines()` invece di `splitlines()` di Python: le righe spezzate da una
  # tabulazione verticale o da un separatore di record tornano una sola.
  mutate "righe spezzate come in Rust invece che come in Python" "$G" \
    's = s.replace("    splitlines(stdout)\n        .into_iter()", "    stdout\n        .lines()", 1)'

  # `trim()` invece di `str.strip()`: i quattro separatori `\\x1c`-`\\x1f` non
  # sono spazio per Rust, e una riga che comincia con uno di quelli smette di
  # contare.
  mutate "bordi ripuliti alla maniera di Rust" "$G" \
    's = s.replace("""    s.trim_matches(|c: char| c.is_whitespace() || (\x27\\u{1c}\x27..=\x27\\u{1f}\x27).contains(&c))""", "    s.trim()", 1)'

  # L uscita non decodificabile smette di far tacere il gancio: dove il Python
  # muore su `UnicodeDecodeError`, il porto conterebbe righe con il carattere di
  # rimpiazzo dentro.
  mutate "uscita non UTF-8 accettata lo stesso" "$I" \
    's = s.replace("    let testo = String::from_utf8(raw).ok()?;", "    let testo = String::from_utf8_lossy(&raw).into_owned();", 1)'

  # Lo stderr del figlio torna a scorrere sullo stderr del gancio, cioe' sotto
  # gli occhi del modello: il porto parlerebbe dove l originale sta zitto.
  mutate "stderr del figlio ereditato" "$I" \
    's = s.replace(".stderr(Stdio::piped())", ".stderr(Stdio::inherit())", 1)'

  # Si parla anche con zero copie collegate: l avviso diventa rumore a ogni
  # apertura di sessione.
  mutate "si parla anche a zero" "$I" \
    's = s.replace("""    if quante == 0 {
        return 0;
    }
""", "", 1)'

  # L'avviso cambia parole: stessa informazione, altro testo. E' il mutante che
  # prova che il confronto guarda stderr byte per byte.
  mutate "testo dell avviso cambiato" "$G" \
    's = s.replace("worktree(s) that had none", "worktrees without rules", 1)'

  # Un ingresso illeggibile smette di essere un fatto e diventa un rimprovero:
  # su PostToolUse un'uscita 2 finisce davanti al modello a ogni comando.
  mutate "uscita 2 su stdin malformato" "$I" \
    's = s.replace("""    let Ok(payload) = serde_json::from_str::<serde_json::Value>(&grezzo) else {
        return 0;
    };""", """    let Ok(payload) = serde_json::from_str::<serde_json::Value>(&grezzo) else {
        return 2;
    };""", 1)'

  COMPARE=""
fi

# ── handoff-threshold ───────────────────────────────────────────────────────
if [ "$WHICH" = "handoff-threshold" ] || [ "$WHICH" = "tutte" ]; then
  echo "mutanti su handoff_threshold.rs:"
  COMPARE="compare-handoff-threshold.py"
  G="crates/guards/src/handoff_threshold.rs"
  I="crates/claude-hooks/src/handoff_threshold.rs"

  # Il gradino basso si sposta: il gancio comincia a parlare dove l'originale
  # tace. È la soglia intera del presidio, e senza un caso appena sotto — 69,4%
  # — nessun confronto se ne accorge.
  mutate "gradino spostato da 70 a 60" "$G" \
    's = s.replace("pub const STEPS: [u64; 2] = [70, 85];", "pub const STEPS: [u64; 2] = [60, 85];", 1)'

  # Il confine da chiuso a aperto: al 70% e all85% esatti non si parla piu.
  mutate "verso del confronto sul gradino" "$G" \
    's = s.replace("percent >= *s", "percent > *s", 1)'

  # Il freno «una volta per gradino» sparisce: il gancio riparla a ogni prompt,
  # che e esattamente il difetto che l'originale dichiara di aver curato.
  mutate "freno una-volta-per-gradino tolto" "$G" \
    's = s.replace("pub fn already_said(text: &str, step: u64) -> bool {", "pub fn already_said(text: &str, step: u64) -> bool {\n    if true { return false; }", 1)'

  # Lo stato non si scrive piu: stesso effetto del mutante di sopra, ma la
  # traccia e diversa — qui il file non compare proprio nella TMPDIR finta.
  mutate "scrittura dello stato saltata" "$I" \
    's = s.replace("let _ = fs::write(&state, step.to_string());", "let _ = &state;", 1)'

  # Il separatore delle migliaia torna quello di Python: 144,000 invece di
  # 144.000. Cambia un byte del testo che la sessione legge.
  mutate "separatore delle migliaia" "$G" \
    's = s.replace("gruppi(used).replace(\x27,\x27, \".\")", "gruppi(used)", 1)'

  # L'arrotondamento lontano da zero al posto di quello al pari: l84,5% diventa
  # 85 e con esso cambia anche QUALE dei due messaggi esce.
  mutate "arrotondamento non al pari" "$G" \
    's = s.replace("round_half_to_even(used as f64 * 100.0 / window as f64)", "(used as f64 * 100.0 / window as f64).round() as u64", 1)'

  # Il messaggio alto si sceglie con il confine aperto: all85% esatto esce
  # quello basso, che non nomina la compattazione.
  mutate "confine del messaggio alto" "$G" \
    's = s.replace("if step >= 85 {", "if step > 85 {", 1)'

  # Le due variabili si guardano nell ordine inverso: lo scavalco manuale non
  # scavalca piu niente, e il gancio non e piu provabile su una sessione vera.
  mutate "ordine delle variabili della finestra" "$I" \
    's = s.replace("[\"SOGLIA_FINESTRA\", \"CLAUDE_CODE_AUTO_COMPACT_WINDOW\"]", "[\"CLAUDE_CODE_AUTO_COMPACT_WINDOW\", \"SOGLIA_FINESTRA\"]", 1)'

  # La misura prende il ripiego dell ALTRO impianto di soglie: un usage di primo
  # livello comincia a contare. E il modo piu probabile in cui questo porto
  # sarebbe stato scritto sbagliato, riusando context_used di handoff.rs.
  mutate "ripiego sull usage di primo livello" "$I" \
    's = s.replace("let usage = msg.as_object()?.get(\"usage\");", "let usage = msg.as_object()?.get(\"usage\").or_else(|| obj.get(\"usage\"));", 1)'

  # La prima riga della coda non si scarta piu: si legge un usage che l oracolo
  # non vede, e solo su transcript sopra i 400 kB.
  mutate "prima riga della coda conservata" "$I" \
    's = s.replace("if size > TAIL_BYTES && !lines.is_empty() {", "if false && size > TAIL_BYTES && !lines.is_empty() {", 1)'

  COMPARE=""
fi

# ── long-session ────────────────────────────────────────────────────────────
if [ "$WHICH" = "long-session" ] || [ "$WHICH" = "tutte" ]; then
  echo "mutanti su long_session.rs:"
  # Nato in Rust il 23/08: non c'e' un Python da confrontare, l'oracolo e' la
  # batteria dei due moduli.
  ORACOLO="cargo test --release -p guards -p claude-hooks long_session"
  G="crates/guards/src/long_session.rs"
  I="crates/claude-hooks/src/long_session.rs"

  # Il centinaio diventa cinquantina: il gancio parla dove deve tacere.
  mutate "gradino spostato da 100 a 50" "$G" \
    's = s.replace("pub const STEP: u64 = 100;", "pub const STEP: u64 = 50;", 1)'

  # Al centesimo esatto non si parla piu: il confine da chiuso ad aperto.
  mutate "confine aperto sul gradino" "$G" \
    's = s.replace("(step >= STEP).then_some(step)", "(step > STEP).then_some(step)", 1)'

  # Il freno «una volta per centinaio» si allenta: lo stato a 100 non copre il
  # turno 150.
  mutate "freno una-volta-per-centinaio allentato" "$G" \
    's = s.replace("is_ok_and(|said| said >= step)", "is_ok_and(|said| said > step)", 1)'

  # Lo streaming torna a contare: ogni riga con usage e un turno, come nello
  # script del 18/08 che gonfiava di 1,85x.
  mutate "deduplicazione su message.id tolta" "$G" \
    's = s.replace("seen.insert(id.to_string());", "seen.insert(format!(\"{id}-{}\", seen.len()));", 1)'

  # La riga perde la via d uscita: e l unica parola che fa agire.
  mutate "la riga non nomina handoff" "$G" \
    's = s.replace("chiudi con `handoff`", "chiudi", 1)'

  # Lo stato non si scrive piu: il gancio riparla a ogni prompt.
  mutate "scrittura dello stato saltata" "$I" \
    's = s.replace("let _ = fs::write(&state, step.to_string());", "let _ = (&state, step);", 1)'

  # La valvola sparisce: LONG_SESSION=off non tace piu.
  mutate "valvola tolta" "$I" \
    's = s.replace("if Mode::from_env(\"LONG_SESSION\") == Mode::Off {", "if false {", 1)'

  ORACOLO=""
fi

# ── spotlight-marker ────────────────────────────────────────────────────────
if [ "$WHICH" = "spotlight-marker" ] || [ "$WHICH" = "tutte" ]; then
  echo "mutanti su spotlight_marker.rs:"
  COMPARE="compare-spotlight-marker.py"
  G="crates/guards/src/spotlight_marker.rs"
  I="crates/claude-hooks/src/spotlight_marker.rs"

  # La condizione si rovescia: il marcatore si rimette su ogni comando TRANNE
  # le installazioni, cioe' esattamente quando non serve.
  mutate "condizione invertita sul riconoscitore" "$I" \
    's = s.replace("if !is_an_install(text) {", "if is_an_install(text) {", 1)'

  # La guardia che tiene `find` fuori da una radice-link: senza, un cwd che e'
  # un collegamento fa scrivere marcatori in un albero che l originale non
  # tocca mai.
  mutate "guardia sulla radice-link tolta" "$I" \
    's = s.replace("""    if fs::symlink_metadata(p)\n        .map(|m| m.file_type().is_symlink())\n        .unwrap_or(false)\n    {\n        return Some(Vec::new());\n    }\n""", "", 1)'

  # La soglia di profondita: a cinque livelli si trovano cartelle che
  # l originale non vede, e si scrive dove lui non scrive.
  mutate "soglia di profondita a cinque" "$G" \
    's = s.replace("pub const MAX_DEPTH: usize = 4;", "pub const MAX_DEPTH: usize = 5;", 1)'

  # Il file non si scrive piu, ma si continua a contarlo: il messaggio resta
  # identico e l unica traccia e' l albero. E' il mutante che dimostra perche'
  # il confronto fotografa il disco e non solo stderr.
  mutate "scrittura del marcatore saltata" "$I" \
    's = s.replace("""        if fs::OpenOptions::new()\n            .create(true)\n            .append(true)\n            .open(&path)\n            .is_ok()\n        {\n            touched.push(d);\n        }""", "        touched.push(d);", 1)'

  # Il formato del messaggio: una parola sola, e chi cerca la riga nei registri
  # non la trova piu.
  mutate "formato del messaggio cambiato" "$I" \
    's = s.replace("exclusion marker(s) under", "exclusion markers under", 1)'

  # Il `-prune` sparisce: si scende dentro le cartelle gia' trovate, e ogni
  # `node_modules` annidata prende un marcatore dentro un albero che ha gia il
  # suo. E' la ragione per cui l originale usa `-prune` e non un `find` piatto.
  mutate "prune tolto" "$I" \
    's = s.replace("""            out.push(child);\n            continue;""", """            out.push(child.clone());\n            walk(&child, depth + 1, out);\n            continue;""", 1)'

  # Il controllo «c e gia»: senza, il conteggio conta anche i marcatori che
  # esistevano, e il gancio dice di aver rimesso cio che non ha toccato.
  mutate "controllo di esistenza tolto" "$I" \
    's = s.replace("""        if path.exists() {\n            continue;\n        }\n""", "", 1)'

  # Il difetto dell originale non si riproduce piu: dove Python muore con
  # uscita 1, il porto esce 0. E la tentazione naturale di chi porta — «tanto
  # e un difetto» — ed e' proprio cio che il confronto deve vietare.
  mutate "AttributeError non riprodotto" "$I" \
    's = s.replace("""        python_type(v),\n        attr\n    );\n    1\n}""", """        python_type(v),\n        attr\n    );\n    0\n}""", 1)'

  # `splitlines` di Python contro `lines` di Rust: differiscono su `\\r`
  # solitario, `\\x0b`, `\\u2028` e compagnia. Col solo `\\n` questo mutante
  # sopravviveva — `lines()` spezza anche lui — ed e' il buco che ha aggiunto al
  # confronto i tre casi coi separatori esotici.
  mutate "splitlines diventa lines di Rust" "$I" \
    's = s.replace(r"""    python_splitlines(&found.join("\n"))""", r"""    found.join("\n").lines().map(String::from).collect::<Vec<_>>()""", 1)'

  # Il filtro su `tool_name`: senza, il gancio parte su ogni strumento, e un
  # Read che nomina «pnpm install» scrive marcatori.
  mutate "filtro su tool_name tolto" "$I" \
    's = s.replace("""    if obj.get(\"tool_name\") != Some(&Value::String(\"Bash\".to_string())) {\n        return 0;\n    }\n""", "", 1)'

  # La valvola sparisce: `MARCATORE_SPOTLIGHT=off` smette di spegnere il gancio.
  mutate "valvola rimossa" "$I" \
    's = s.replace("""    if std::env::var(\"MARCATORE_SPOTLIGHT\").as_deref() == Ok(\"off\") {\n        return 0;\n    }\n""", "", 1)'

  # Lo spazio di Rust invece di quello di Python: i separatori di controllo
  # `\\x1c`-`\\x1f` smettono di contare come spazio nella normalizzazione.
  mutate "spazio di Rust invece di quello di Python" "$G" \
    "s = s.replace(\"    c.is_whitespace() || matches!(c, '\\\\u{1c}'..='\\\\u{1f}')\", '    c.is_whitespace()', 1)"

  # La radice non combacia piu con se stessa: un `pnpm install` lanciato da
  # dentro una `node_modules` smette di rimetterci il marcatore.
  mutate "la radice non combacia con se stessa" "$I" \
    's = s.replace("    if is_dependency_dir(&find_name(root)) {", "    if false {", 1)'

  # Un `cwd` intero: `os.path.isdir` lo legge come descrittore e risponde no,
  # senza errori. Trattarlo come un TypeError fa morire il porto dove
  # l originale tace.
  mutate "cwd intera trattata come errore" "$I" \
    's = s.replace("        Some(Value::Number(n)) if !n.is_f64() => Root::Opaque,", "        Some(Value::Number(_)) => Root::Refused(\"int\"),", 1)'

  # Il nome del marcatore: un file che somiglia a quello giusto non spegne
  # Spotlight, e nessun messaggio lo direbbe.
  mutate "nome del marcatore cambiato" "$G" \
    "s = s.replace('pub const MARKER: &str = \".metadata_never_index\";', 'pub const MARKER: &str = \".metadata_never_indexed\";', 1)"

  COMPARE=""
fi

# ── work-status ─────────────────────────────────────────────────────────────
# Il gancio che SCRIVE SU ORCA: qui un mutante sopravvissuto non e' un messaggio
# sbagliato, e' lo stato di una copia di lavoro riscritto quando non doveva. Il
# confronto non tocca Orca vera — `orca`, `gh` e `git` sono finti e ogni
# chiamata finisce in un registro che fa parte di cio' che si confronta.
if [ "$WHICH" = "work-status" ] || [ "$WHICH" = "tutte" ]; then
  echo "mutanti su work_status.rs (lo stato delle lavorazioni):"
  COMPARE="compare-work-status.py"
  W="crates/claude-hooks/src/work_status.rs"

  # LA GUARDIA CHE E' COSTATA DUE MARCATURE SBAGLIATE IL 17/08/2026: la copia
  # scelta deve parlare proprio col repo dove la richiesta e' nata. Senza, la
  # copia della sessione viene marcata per la richiesta di qualcun altro.
  mutate "repo della richiesta non verificato" "$W" \
    's = s.replace("    if wanted != slug_of_remote(&path_) {", "    if false != slug_of_remote(&path_).is_empty() {", 1)'

  # L URL stampato smette di essere la prova: il solo testo `gh pr create` —
  # dentro un messaggio di commit, in un grep — torna a marcare.
  mutate "URL non piu richiesto" "$W" \
    's = s.replace("    if wanted.is_empty() {\n        return Ok(None);\n    }\n", "", 1)'

  # Un comando fallito o interrotto torna a valere come richiesta nata.
  mutate "errore dello strumento ignorato" "$W" \
    's = s.replace("        if rotto {\n            return Ok(None);\n        }\n", "", 1)'

  # I cambi di cartella si leggono dal primo invece che dall ultimo: in
  # `cd /tmp && cd <copia> && gh pr create` la richiesta nasce nella seconda.
  mutate "primo cd invece dell ultimo" "$W" \
    's = s.replace("    visti.reverse();\n", "", 1)'

  # IL DIFETTO NOTO, primo verso: un repo che non ha risposto torna
  # indistinguibile da un repo senza richieste, e le sue copie in revisione
  # vengono declassate a «in-progress» a ogni avvio di sessione.
  mutate "repo muto letto come repo senza richieste" "$W" \
    's = s.replace("        if repo_noti.contains(&mine) && !answered.contains(&mine) {", "        if false && repo_noti.contains(&mine) && !answered.contains(&mine) {", 1)'

  # IL DIFETTO NOTO, secondo verso: con TUTTI i repo muti si riscrive lo stesso,
  # cioe' trentatre' stati riscritti su zero informazione.
  mutate "nessuna risposta, si riscrive lo stesso" "$W" \
    's = s.replace("    if answered.is_empty() {", "    if false {", 1)'

  # Una prova sola invece di tre: col 503 due volte su tre, il repo risulta muto
  # quasi sempre.
  mutate "un solo tentativo su gh" "$W" \
    's = s.replace("        for _ in 0..3 {", "        for _ in 0..1 {", 1)'

  # DOPO UNO SQUASH `merge-base --is-ancestor` mente sempre: il confronto giusto
  # e' sul contenuto. Senza, una copia gia' confluita resta «in-progress» per
  # sempre.
  mutate "contenuto non confrontato dopo lo squash" "$W" \
    's = s.replace("            if !f.is_empty() && f == albero {", "            if false {", 1)'

  # git muto torna a valere come albero pulito: su uno zero finto una copia
  # viene dichiarata completata.
  mutate "git muto letto come zero" "$W" \
    's = s.replace("--not\", \"--remotes\"])?;", "--not\", \"--remotes\"]).unwrap_or_default();", 1)'

  # Il residuo smette di contare: `media-link-recovery` torna «completed» con
  # sette commit scritti dopo la fusione.
  mutate "residuo ignorato nel giudizio" "$W" \
    's = s.replace("            Some(_) => \"in-progress\".into(),", "            Some(_) => \"completed\".into(),", 1)'

  # Il giro a secco scrive davvero: e' il mutante che, senza il registro delle
  # chiamate, sarebbe invisibile.
  mutate "a secco scrive lo stesso" "$W" \
    's = s.replace("        if secco {", "        if false {", 1)'

  # La tab dell agente viene chiusa insieme a quella di avvio: con `--agent`
  # quella tab ospita il lavoro.
  mutate "tab dell agente chiusa" "$W" \
    's = s.replace("    if startup.is_empty() || startup == agent {", "    if startup.is_empty() {", 1)'

  # Un checkout canonico torna a essere giudicato come una lavorazione: e' il
  # motivo per cui cinque erano «in-review» da 16-25 giorni.
  mutate "checkout canonico giudicato come lavorazione" "$W" \
    's = s.replace("    if git.get(\"isMainWorktree\").map(truthy).unwrap_or(false) {", "    if false {", 1)'

  # Il registro del giro sparisce: «quanti repo non hanno risposto?» torna una
  # domanda senza risposta, ed e' come il difetto e' rimasto invisibile per
  # giorni.
  mutate "registro del giro rimosso" "$W" \
    's = s.replace("    registra(\n        if secco", "    #[allow(unused)] let _skip = (\n        if secco", 1)'

  # La riga del registro perde lo spazio dopo i due punti, cioe prende la forma
  # del JavaScript: stessa informazione, byte diversi, due formati mescolati
  # nello stesso archivio storico.
  mutate "registro nella forma senza spazi" "$W" \
    's = s.replace("hook_io::python_json::dumps_unicode(&Value::Object(line))", "serde_json::to_string(&Value::Object(line)).unwrap_or_default()", 1)'

  # Il guasto torna muto: un gancio rotto e uno contento si leggono uguali.
  mutate "guasto non annotato" "$W" \
    's = s.replace("            Err(e) => annota_guasto(name, &e),", "            Err(_e) => {}", 1)'

  COMPARE=""
fi

# ── skill-nudge ─────────────────────────────────────────────────────────────
if [ "$WHICH" = "skill-nudge" ] || [ "$WHICH" = "tutte" ]; then
  echo "mutanti su skill_nudge.rs:"
  COMPARE="compare-skill-nudge.py"
  G="crates/guards/src/skill_nudge.rs"
  S="crates/claude-hooks/src/skill_nudge.rs"

  # La soglia di costo si sposta: la fascia che consuma l'82% della cache
  # riletta comincia altrove, e l'invito a passare il lavoro sparisce.
  mutate "soglia di costo spostata" "$G" \
    's = s.replace("pub const DEFAULT_THRESHOLD_MB: u64 = 6;", "pub const DEFAULT_THRESHOLD_MB: u64 = 30;", 1)'

  # Il taglio in lunghezza abbassato: le richieste lunghe — cioe' quelle con
  # piu' aree in una volta, dove la competenza serve di piu' — tornano mute.
  mutate "taglio in lunghezza abbassato" "$G" \
    's = s.replace("pub const CHAR_CUTOFF: usize = 12_000;", "pub const CHAR_CUTOFF: usize = 11_000;", 1)'

  # Una domanda torna a essere lavoro nuovo: «hai testato e verificato in ui?»
  # contiene un verbo di compito al participio, e senza questo filtro innescava
  # l'invito a delegare una risposta.
  mutate "la domanda torna lavoro nuovo" "$G" \
    's = s.replace("if mb >= threshold && !already_said && !is_question", "if mb >= threshold && !already_said", 1)'

  # Il freno a un colpo tolto: l'invito torna a ogni messaggio, che e'
  # esattamente il rumore per cui il freno esiste. Nella prova sui messaggi veri
  # usciva 276 volte, e dopo la prima nessuna aggiungeva nulla.
  mutate "freno a un colpo tolto" "$G" \
    's = s.replace("if mb >= threshold && !already_said", "if mb >= threshold", 1)'

  # L'ordine con cui si sceglie la competenza si inverte: con il tetto di due,
  # una frase che ne aggancia tre nomina le ultime invece delle prime.
  mutate "ordine della tavola invertito" "$G" \
    's = s.replace("for (skill, re) in SKILLS.iter().zip(skill_regexes()) {", "for (skill, re) in SKILLS.iter().rev().zip(skill_regexes().iter().rev()) {", 1)'

  # Il tetto di due alzato: un elenco lungo e rumore, e viene saltato come tutto
  # il resto.
  mutate "tetto di due competenze alzato" "$G" \
    's = s.replace("if matched.len() == 2 {", "if matched.len() == 3 {", 1)'

  # Una voce sparisce dalla tavola. Non si neutralizza lo schema: si toglie
  # proprio la riga, com'e' successo davvero due volte in agosto.
  mutate "una voce tolta dalla tavola" "$G" \
    'i = s.index("        name: \"orca-linear\",")
j = s.rindex("    Skill {", 0, i)
k = s.index("    },\n", i) + len("    },\n")
s = s[:j] + s[k:]'

  # Il testo della riga cambia. Stessa decisione, altre parole: il confronto
  # deve guardare cosa esce, non solo se esce qualcosa.
  mutate "testo della riga cambiato" "$G" \
    's = s.replace("\"Esiste `{}`: copre {}, con la procedura già scritta e provata.\",", "\"Esiste gia` `{}`: copre {}, con la procedura già scritta e provata.\",", 1)'

  # Il catalogo ignorato: si torna a nominare competenze disinstallate, che e'
  # mandare a cercare una cosa che non c'e' — peggio del silenzio.
  mutate "catalogo ignorato" "$G" \
    's = s.replace("            if !exists(skill.name)? {", "            if false && !exists(skill.name)? {", 1)'

  # `str.strip()` di Python torna `trim()` di Rust: i quattro separatori di
  # controllo restano attaccati alla richiesta.
  mutate "estremi tagliati con trim()" "$G" \
    's = s.replace("    let p = python_trim(prompt);", "    let p = prompt.trim();", 1)'

  # Lo sguardo indietro riscritto sparisce da `prisma`: i PERCORSI tornano ad
  # agganciare, che e' il difetto corretto a mano sull'originale.
  mutate "sguardo indietro di prisma tolto" "$G" \
    's = s.replace("r\"(?:^|[^/.\\w])(prisma (migrate", "r\"(prisma (migrate", 1)'

  # Lo sguardo avanti riscritto sparisce da `mastra`: `mastra.config.ts` torna a
  # essere il framework.
  mutate "sguardo avanti di mastra tolto" "$G" \
    's = s.replace("dentro) mastra(?:$|[^/.\\w]))\",", "dentro) mastra)\",", 1)'

  # Il registro non scrive piu': il suggerimento diventa non verificabile, ed e'
  # l'unico modo di sapere fra un mese se e' stato raccolto.
  mutate "registro del suggerimento tolto" "$S" \
    's = s.replace("fn log_suggestion(session: &str, reason: &str, skills: &[String], chars: usize) {", "fn log_suggestion(session: &str, reason: &str, skills: &[String], chars: usize) {\n    if true { let _ = (session, reason, skills, chars); return }", 1)'

  # Il marcatore nasce leggibile da chiunque invece che 0600.
  mutate "permessi del marcatore allargati" "$S" \
    's = s.replace("                .mode(0o600)\n", "", 1)'

  COMPARE=""
fi

# ── orca-cleanup ────────────────────────────────────────────────────────────
#
# Qui il mutante piu' informativo non cambia una riga di testo: cambia **quali
# chiamate a orca partono**. Un porto che stampa la tabella giusta e non chiude
# niente, o che chiude la scheda di chi sta lavorando, ha lo stesso stdout — la
# differenza sta solo nel registro del finto orca. Meta' dei mutanti qui sotto
# sono di quella specie, e sono l'unica ragione per cui quel registro esiste.
if [ "$WHICH" = "orca-cleanup" ] || [ "$WHICH" = "tutte" ]; then
  echo "mutanti su orca_cleanup.rs (chiude terminali e rinomina copie):"
  COMPARE="compare-orca-cleanup.py"
  O="crates/claude-hooks/src/orca_cleanup.rs"

  # La chiusura perde il passo con --tab: il pannello se ne va, la voce resta
  # nella barra. Invisibile a stdout, visibile solo nel registro.
  mutate "chiusura senza --tab" "$O" \
    's = s.replace("    orca(&[\"terminal\", \"close\", \"--terminal\", handle, \"--tab\"]);\n", "", 1)'

  # I due passi in ordine invertito: e lo stesso insieme di chiamate, quindi lo
  # vede solo la vista per manico. E il mutante che dice se l ordinamento del
  # blocco delle chiusure ha nascosto qualcosa.
  mutate "chiusura in ordine invertito" "$O" \
    's = s.replace("    orca(&[\"terminal\", \"close\", \"--terminal\", handle, \"--tab\"]);\n    orca(&[\"terminal\", \"close\", \"--terminal\", handle]);", "    orca(&[\"terminal\", \"close\", \"--terminal\", handle]);\n    orca(&[\"terminal\", \"close\", \"--terminal\", handle, \"--tab\"]);", 1)'

  # La scheda da cui sto girando smette di essere protetta: il gancio di
  # SessionEnd chiuderebbe se stesso e le schede della stessa cartella.
  mutate "la mia scheda non e protetta" "$O" \
    's = s.replace("fn is_mine(term: &Value) -> bool {", "fn is_mine(term: &Value) -> bool {\n    if true { return false }", 1)'

  # `waiting` finisce fra gli stati che liberano: si chiude una sessione che
  # sta aspettando una risposta, buttando via cio che aspettava.
  mutate "waiting vale come done" "$O" \
    's = s.replace("if state == \"working\" || state == \"waiting\" {", "if state == \"working\" {", 1)'

  # La chiave dello stato perde il pannello: lo stato di un pannello decide per
  # l altro pannello della stessa scheda.
  mutate "lo stato si cerca per sola scheda" "$O" \
    's = s.replace("let state = states.get(&key).and_then(|v| v.as_str()).unwrap_or(\"\");", "let pre = format!(\"{}:\", py_str(term.get(\"tabId\")));\n    let state = states.iter().find(|(k, _)| k.starts_with(&pre)).map(|(_, v)| v).and_then(|v| v.as_str()).unwrap_or(\"\");", 1)'

  # Un agente che ha appena finito si chiude subito, senza i venti minuti di
  # grazia: puo essere ancora dentro a scrivere.
  mutate "nessuna grazia per un agente appena finito" "$O" \
    's = s.replace("        if idle < DONE_IDLE_MIN {", "        if false {", 1)'

  # Le consegne in attesa di INVIO non sono piu protette da --handoffs.
  mutate "le consegne non sono piu protette" "$O" \
    's = s.replace("        if !with_handoffs {", "        if false {", 1)'

  # Senza lastOutputAt il terminale risulta appena attivo invece che vecchio
  # all infinito: chi non ha mai scritto niente smette di essere chiudibile.
  mutate "senza lastOutputAt il terminale e nuovo" "$O" \
    's = s.replace("        _ => return f64::INFINITY,", "        _ => return 0.0,", 1)'

  # L esito del processo torna a contare: l originale passa check=False, e un
  # orca che esce non-zero stampando JSON buono va preso per buono.
  mutate "l esito di orca decide" "$O" \
    's = s.replace("    let _ = child.wait();\n    Some(String::from_utf8_lossy(&buf).into_owned())", "    if !child.wait().map(|e| e.success()).unwrap_or(false) { return None }\n    Some(String::from_utf8_lossy(&buf).into_owned())", 1)'

  # «Orca non risponde» diventa «non c e nessun terminale»: due frasi diverse
  # per due situazioni diverse, e il porto le confonde.
  mutate "orca muto vale elenco vuoto" "$O" \
    's = s.replace("    let Some(terminals) = collect(args.worktree.as_deref()) else {", "    let Some(terminals) = collect(args.worktree.as_deref()).or(Some(Vec::new())) else {", 1)'

  # Un titolo generico torna a poter rinominare una tab: «consegna raccolta»
  # diventerebbe «Claude Code».
  mutate "un titolo generico rinomina la tab" "$O" \
    's = s.replace("    if wanted.is_empty() || is_anonymous_title(&wanted) {", "    if wanted.is_empty() {", 1)'

  # La rinomina si conta invece di verificarla rileggendo: dice sempre «fatte
  # tutte», anche quando Orca non ha cambiato niente.
  mutate "rinomina data per fatta senza rileggere" "$O" \
    's = s.replace("        .filter(|(w, _)| after.get(&realpath(&text(w.get(\"path\")))) == Some(&expected_name(w)))\n", "", 1)'

  # La copia principale prende il nome del ramo invece che del repo: il nome
  # ballerebbe a ogni checkout.
  mutate "la copia principale prende il ramo" "$O" \
    's = s.replace("    if truthy(w.get(\"isMainWorktree\")) {", "    if false {", 1)'

  # La copia si indica col nome invece che col percorso. Stesso stdout, altra
  # chiamata: senza il registro non lo vedrebbe nessuno.
  mutate "la copia si indica col nome" "$O" \
    's = s.replace("let target = format!(\"path:{}\", py_str(w.get(\"path\")));", "let target = format!(\"name:{}\", py_str(w.get(\"path\")));", 1)'

  # Con --align si rinominano anche le tab: nello stesso comando si
  # sovrascriverebbero nomi scritti a mano con titoli di agente.
  mutate "con --align si toccano anche le tab" "$O" \
    's = s.replace("        if args.align {\n            return outcome;\n        }\n", "", 1)'

  COMPARE=""
fi

# ── phase-router ────────────────────────────────────────────────────────────
if [ "$WHICH" = "phase-router" ] || [ "$WHICH" = "tutte" ]; then
  echo "mutanti su phase_router.rs (il router di fase, gradino 5):"
  # Nato in Rust il 23/08, senza un Python da confrontare: l'oracolo e' la
  # batteria dei due moduli, come per long-session.
  ORACOLO="cargo test --release -p guards -p claude-hooks phase_router"
  G="crates/guards/src/phase_router.rs"
  I="crates/claude-hooks/src/phase_router.rs"

  # Il mutante piu' pericoloso dei tre: chi lancia un subagent con un modello
  # esplicito se lo vede sovrascritto lo stesso.
  mutate "riscrive anche con model gia' scritto" "$G" \
    's = s.replace("    if model.is_some_and(|m| !m.trim().is_empty()) {\n        return Routing {\n            model: None,\n            trade,\n            reason: \"chi lancia ha deciso\",\n        };\n    }\n", "", 1)'

  # La valvola smette di spegnere: PHASE_ROUTER=off non lascia piu' passare
  # tutto com'e'. E' nell'involucro, non nel giudizio puro: la traduzione
  # dall'ambiente e' il punto che un test sul solo `guards::route` non vede.
  mutate "PHASE_ROUTER=off non spegne" "$I" \
    's = s.replace("let valve_off = hook_io::Mode::from_env(\"PHASE_ROUTER\") == hook_io::Mode::Off;", "let valve_off = false;", 1)'

  # La tabella candidata cambia riga: measurer va su sonnet invece di haiku.
  # E' la forma che il router deve saper applicare, provata sulla tabella
  # delle batterie perche' quella in servizio puo' essere vuota.
  mutate "measurer va su sonnet invece che haiku" "$G" \
    's = s.replace("\"measurer\",\n        \"haiku\",", "\"measurer\",\n        \"sonnet\",", 1)'

  # La tabella in servizio si riempie senza che la prova a campione l'abbia
  # promossa: e' la riga che il 23/08 e' stata svuotata per misura.
  mutate "la tabella in servizio torna piena senza prova" "$G" \
    's = s.replace("pub const TRADE_MODEL: &[Row] = &[];", "pub const TRADE_MODEL: &[Row] = CANDIDATE_TABLE;", 1)'

  # La forma del mandato smette di fermare un measurer a cui si chiede di
  # scrivere: torna a girare su un modello piccolo mentre modifica sorgente.
  mutate "il verbo di scrittura non ferma piu' il measurer" "$G" \
    's = s.replace("        if asks_to_write_code(prompt) {\n            return Routing {\n                model: None,\n                trade,\n                reason: \"il mandato chiede di scrivere codice\",\n            };\n        }\n", "", 1)'

  ORACOLO=""
fi

# ── instincts ───────────────────────────────────────────────────────────────
if [ "$WHICH" = "instincts" ] || [ "$WHICH" = "tutte" ]; then
  echo "mutanti su instincts.rs (i due tetti del blocco iniettato):"
  # I dodici del correttivo `6319f7c`, applicati a mano quel giorno e qui resi
  # ripetibili. L'oracolo e' la batteria del modulo: `select()` e' un giudizio
  # puro, non c'e' nessun Python da mettergli di fronte.
  ORACOLO="cargo test --release -p guards instincts"
  I="crates/guards/src/instincts.rs"

  # IL MUTANTE PER CUI IL TETTO ERA STATO RESPINTO: senza accumulatore ogni
  # corpo si misura da solo, nessuno sfonda, e tutti entrano. Il caso che lo
  # vede sono corpi piccoli che sfondano solo insieme.
  mutate "accumulatore dei byte rimosso" "$I" \
    's = s.replace("let candidate_bytes = bytes_so_far + separator + body.len();", "let candidate_bytes = separator + body.len();", 1)'

  # Stesso buco per l altra strada: l accumulatore c e ma riparte da zero a ogni
  # giro.
  mutate "accumulatore azzerato a ogni corpo" "$I" \
    's = s.replace("        bytes_so_far = candidate_bytes;", "        bytes_so_far = 0;", 1)'

  # I due byte fra un corpo e l altro escono dal conto: cinque corpi da 700
  # fanno esattamente il tetto, e sono gli otto byte dei separatori a decidere.
  mutate "separatore fisso a zero" "$I" \
    's = s.replace("let separator = if included.is_empty() { 0 } else { 2 };", "let separator = 0;", 1)'

  # Un carattere sul confine: il corpo lungo quanto il tetto smette di entrare.
  mutate "confine del tetto con >= invece di >" "$I" \
    's = s.replace("if candidate_bytes > live_budget {", "if candidate_bytes >= live_budget {", 1)'

  # La testa conta gli iniettati invece dei vivi: «Vivi: 8» a fronte di nove
  # istinti vivi, cioe' il numero che serve a sapere quanto e' stato tagliato.
  mutate "testa scritta su included.len()" "$I" \
    's = s.replace("\"Vivi: {live_count} · scaduti: {expired_count} · senza misura: {undated_count}\"", "\"Vivi: {} · scaduti: {expired_count} · senza misura: {undated_count}\", included.len()", 1)'

  # Il distacco davanti alla coda torna a dipendere dai vivi invece che da cio'
  # che e' entrato: con niente iniettato si stampa una riga vuota di troppo.
  mutate "distacco deciso su live_count" "$I" \
    's = s.replace("if !included.is_empty() {", "if live_count > 0 {", 1)'

  # La causa del taglio si stampa sempre: a taglio zero il blocco dichiara
  # «tagliati dal tetto: 0 ()», che e' un allarme falso a ogni sessione.
  mutate "causa del taglio stampata anche a zero" "$I" \
    's = s.replace("if excluded_by_cap > 0 {", "if true {", 1)'

  # Il difetto che la revisione aveva trovato: il primo corpo troppo grosso
  # chiude l elenco per tutti quelli dietro, che nel budget rimasto ci stavano.
  mutate "break invece di continue sul corpo troppo grosso" "$I" \
    's = s.replace("            cut_by_bytes += 1;\n            continue;", "            cut_by_bytes += 1;\n            break;", 1)'

  # La coda «Da rimisurare» torna fuori da ogni limite: 40 istinti scaduti fanno
  # 10.053 byte, il triplo del blocco intero.
  mutate "coda resa intera" "$I" \
    's = s.replace("        if out.len() + separator + line.len() + OVERFLOW_LINE_MAX > MAX_REMEASURE_BYTES {\n            break;\n        }\n", "", 1)'

  # La coda si prende un budget suo invece di toglierlo ai corpi vivi: il tetto
  # smette di valere sul blocco emesso e vale solo su meta'.
  mutate "budget della coda fuori dal conto" "$I" \
    's = s.replace("MAX_LIVE_BYTES.saturating_sub(remeasure_text.len() + joint)", "MAX_LIVE_BYTES.saturating_sub(joint)", 1)'

  # `confidence: 0,95` torna a cadere a 0.0 in silenzio: l istinto si inietta
  # ultimo in classifica e la testa gli attribuisce una causa che non e' la sua.
  mutate "confidence illeggibile riportata a 0.0" "$I" \
    's = s.replace("                fm.confidence = match value.parse::<f64>() {\n                    Ok(v) if v.is_finite() => Confidence::Value(v),\n                    _ => Confidence::Unreadable(value.to_string()),\n                }", "                fm.confidence = Confidence::Value(value.parse::<f64>().unwrap_or(0.0));", 1)'

  # La soglia torna scritta a mano nel messaggio: cambiare la costante non
  # cambierebbe piu' il testo, e il blocco direbbe un numero falso.
  mutate "soglia del conteggio scritta a mano" "$I" \
    's = s.replace("{cut_by_count} oltre le prime {MAX_LIVE_COUNT} per confidence", "{cut_by_count} oltre le prime 5 per confidence", 1)'

  ORACOLO=""
fi

echo
# Zero provati non e' zero sopravvissuti: qui il giro non sa niente di niente, e
# il verde di sotto sarebbe una dichiarazione senza misura.
if [ "$attempted" -eq 0 ]; then
  echo "nessun mutante eseguito su '$WHICH': la sezione non ne contiene nemmeno uno."
  echo "  Questo giro non dice niente sulla batteria: aggiungi i mutanti, poi rilancia."
  exit 2
fi
if [ "$broken" -gt 0 ]; then
  echo "$broken mutanti non applicati: correggi lo script prima di leggere il resto"
fi
if [ "$skipped" -gt 0 ]; then
  echo "$skipped mutanti scartati perche' l'albero non compila: NON sono stati provati,"
  echo "  e questo giro non dice niente su di loro. Ripara la compilazione e rilancia."
fi
if [ "$fallbacks" -gt 0 ]; then
  echo "$fallbacks mutanti giudicati da 'cargo test' e non dal confronto dichiarato:"
  echo "  gli script tools/compare-*.py non esistono piu'. Il verdetto vale, ma"
  echo "  l'elenco COMPARE delle sezioni e' da riscrivere sulla batteria Rust."
fi
if [ "$survivors" -eq 0 ] && [ "$broken" -eq 0 ] && [ "$skipped" -eq 0 ]; then
  echo "tutti i mutanti uccisi: la batteria vede ciò che dichiara di vedere"
elif [ "$survivors" -gt 0 ]; then
  echo "$survivors mutanti sopravvissuti: la batteria ha un punto cieco"
fi
exit $((survivors + broken + skipped))
