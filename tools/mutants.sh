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
  "crates/guards/src/live_rules.rs"
  "crates/claude-hooks/src/live_rules.rs"
  "crates/guards/src/handoff.rs"
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
skipped=0
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
    # Uno scartato NON e' un ucciso, e va contato a parte: il 17/08/2026 un file
    # rotto da un'altra sessione ha fatto fallire la compilazione dal terzo
    # mutante in poi, e questo script ha stampato «tutti i mutanti uccisi» su un
    # giro in cui nove non erano nemmeno stati provati. Un verdetto che si
    # confonde col silenzio e' peggio di nessun verdetto.
    echo "  SCARTATO    $name  <- non compila, quindi non e' stato provato"
    skipped=$((skipped + 1))
    return
  fi
  # Senza virgolette di proposito: così `COMPARE` può portarsi dietro un
  # argomento (`compare-live-rules.py 40`). Un confronto che gira su tutto il
  # corpus per ognuno degli otto mutanti costa ore, e un giro che nessuno
  # aspetta non viene lanciato — che è il modo in cui una batteria smette di
  # esistere. I nomi dei file non hanno spazi, quindi qui non si perde niente.
  # shellcheck disable=SC2086
  if (cd "$ROOT" && python3 tools/$COMPARE >/dev/null 2>&1); then
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

echo
if [ "$broken" -gt 0 ]; then
  echo "$broken mutanti non applicati: correggi lo script prima di leggere il resto"
fi
if [ "$skipped" -gt 0 ]; then
  echo "$skipped mutanti scartati perche' l'albero non compila: NON sono stati provati,"
  echo "  e questo giro non dice niente su di loro. Ripara la compilazione e rilancia."
fi
if [ "$survivors" -eq 0 ] && [ "$broken" -eq 0 ] && [ "$skipped" -eq 0 ]; then
  echo "tutti i mutanti uccisi: la batteria vede ciò che dichiara di vedere"
elif [ "$survivors" -gt 0 ]; then
  echo "$survivors mutanti sopravvissuti: la batteria ha un punto cieco"
fi
exit $((survivors + broken + skipped))
