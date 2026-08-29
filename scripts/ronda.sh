#!/bin/sh
# Un flusso a ronda: lo stesso flusso corto, eseguito molte volte.
#
# PERCHÉ UNO SCRIPT E NON UN FLUSSO. Il motore rifiuta i cicli nel grafo, di
# proposito: un grafo con un ciclo non ha un fronte e non si sa quando è finito.
# Il ciclo di un flusso a ronda sta nel tempo, non nel disegno — e questo file è
# il gradino prima di lasciarlo fare a una macchina. Quando l'orologio di Sailor
# eseguirà ciò che `sailor flow due` già calcola, questo script si cancella.
#
# LE TRE CONDIZIONI DI USCITA, e la prima è quella che conta:
#   1. il flusso dichiara che non è rimasto niente da fare;
#   2. due giri di fila sono falliti — insistere su un guasto che si ripete
#      brucia tempo e produce commit peggiori del niente;
#   3. il tetto di giri è stato raggiunto.
# Un ciclo senza un'uscita che non sia il tetto è un ciclo che qualcuno dovrà
# fermare a mano, e chi lo lascia girando di notte non c'è.
#
#   uso:  scripts/ronda.sh [giri] [indicazione per la scelta]

set -u
GIRI="${1:-5}"
INDICAZIONE="${2:-}"
FLUSSO="sviluppa-sailor"
CASA="$(cd "$(dirname "$0")/.." && pwd)"
REGISTRO="$CASA/.ronda.log"

cd "$CASA" || exit 1

if [ -n "$INDICAZIONE" ]; then
  # L'indicazione entra dall'innesco, dove il flusso la legge come «viene prima
  # delle tue regole di scelta». Si scrive nel file perché è lì che l'innesco
  # manuale prende il proprio testo, e il flusso resta quello che è.
  python3 - "$INDICAZIONE" <<'PY'
import json, sys
from pathlib import Path
p = Path('flows/sviluppa-sailor.flow.json')
d = json.loads(p.read_text())
d['inputs']['trigger']['text'] = sys.argv[1]
p.write_text(json.dumps(d, indent=2, ensure_ascii=False) + "\n")
PY
fi

falliti_di_fila=0
giro=1
while [ "$giro" -le "$GIRI" ]; do
  printf '%s  giro %s di %s\n' "$(date '+%H:%M:%S')" "$giro" "$GIRI" | tee -a "$REGISTRO"

  cargo run -q -p sailor -- flow run "$FLUSSO" >> "$REGISTRO" 2>&1
  esito=$?

  # La corsa appena chiusa la racconta il deposito, non lo schermo: è lo stesso
  # posto da cui un flusso può leggerla, quindi qui non nasce una seconda verità.
  ultima=$(sqlite3 "${SAILOR_LEDGER:-$HOME/.claude/state/flussi}/state.db" \
           "SELECT run_id FROM steps WHERE run_id LIKE '$FLUSSO%' ORDER BY started_at DESC LIMIT 1;" 2>/dev/null)

  niente=$(sqlite3 "${SAILOR_LEDGER:-$HOME/.claude/state/flussi}/state.db" \
    "SELECT json_extract(output,'\$.answer.nothing_left') FROM steps
     WHERE run_id='$ultima' AND step_id='scegli';" 2>/dev/null)

  if [ -n "$niente" ] && [ "$niente" != "null" ] && [ "$niente" != "" ]; then
    printf 'FERMO: il flusso dice che non è rimasto niente da fare — %s\n' "$niente" | tee -a "$REGISTRO"
    exit 0
  fi

  if [ "$esito" -eq 0 ]; then
    falliti_di_fila=0
    printf '  giro %s: passato\n' "$giro" | tee -a "$REGISTRO"
  else
    falliti_di_fila=$((falliti_di_fila + 1))
    printf '  giro %s: fallito (%s di fila)\n' "$giro" "$falliti_di_fila" | tee -a "$REGISTRO"
    if [ "$falliti_di_fila" -ge 2 ]; then
      printf 'FERMO: due giri di fila falliti. Il lavoro respinto è nel working tree, da guardare.\n' | tee -a "$REGISTRO"
      exit 1
    fi
  fi

  giro=$((giro + 1))
done

printf 'FERMO: tetto di %s giri raggiunto.\n' "$GIRI" | tee -a "$REGISTRO"
