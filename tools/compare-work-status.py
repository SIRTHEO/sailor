#!/usr/bin/env python3
"""Confronta il gancio che scrive lo stato delle lavorazioni col suo porto in Rust.

L'ORACOLO È IL PYTHON. Questo gancio non concede e non blocca: ciò che può
divergere è **cosa dice a Orca**. Si confrontano byte per byte cinque cose:

    l'uscita        deve restare 0 in ogni caso, anche sui payload storti
    stdout          il messaggio nel contesto, e le righe del giro a secco
    stderr          vuoto da entrambe le parti
    le chiamate     ogni invocazione di `orca`, `gh` e `git`, in ordine
    i file scritti  `state/ganci.jsonl` e `state/work-status-failures.log`

LE CHIAMATE SONO METÀ DELL'ORACOLO, e senza di loro questo confronto non
dimostrerebbe niente. Un porto che non parla con Orca affatto produce lo stesso
stdout di uno che la marca bene: la differenza sta solo in *quale* comando è
partito e con quali argomenti. Lo stesso vale al contrario — il caso «lo stato
è già quello giusto» si distingue da «l'ha riscritto lo stesso» solo perché nel
registro non compare nessun `worktree set`.

NESSUNA PROVA PUÒ TOCCARE ORCA VERA, e non è una precauzione teorica: il modo
`--riconcilia` scrive lo `workspaceStatus` di ogni copia della macchina, e
alcune stanno lavorando adesso. Tre difese, tutte verificate **prima** di
eseguire il primo caso (`porte_chiuse`):

  1. `orca`, `gh` e `git` sono finti e stanno in una cartella davanti al `PATH`,
     quindi anche una risoluzione per nome finisce sul finto;
  2. il porto in Rust legge `WORK_STATUS_ORCA`, e il lato Python riceve lo stesso
     percorso perché il conduttore gli riscrive la costante `ORCA` del modulo —
     l'originale la tiene assoluta (`/usr/local/bin/orca`), quindi il solo `PATH`
     non basterebbe a deviarlo. L'originale **non si modifica**: si carica come
     modulo e gli si cambia un attributo, che è lo stesso gesto delle sue prove
     interne;
  3. una sonda in sola lettura verifica che entrambe le esecuzioni arrivino
     davvero al finto. Se una delle due non lo raggiungesse, lo strumento si
     ferma qui invece di scoprirlo su una copia vera.

DUE HOME FINTE E DUE REGISTRI SEPARATI, uno per parte: il gancio scrive stato e
il finto registra le chiamate, e con una cartella sola la seconda esecuzione
leggerebbe (o allungherebbe) il lavoro della prima.

IL TRACEBACK NON SI CONFRONTA. `annota_guasto` scrive `--- <passo>` seguito dal
traceback di Python, che in Rust non esiste. Si confrontano le righe `--- `, cioè
l'informazione — *quale* passo si è rotto — e non il testo dell'interprete.

    python3 tools/compare-work-status.py
"""
import json
import os
import re
import shutil
import subprocess
import sys
from pathlib import Path

ROOT = Path.home() / ".claude"
HOOK = ROOT / "skills" / "hooks" / "work-status.py"
BINARY = ROOT / "rust" / "target" / "release" / "claude-hooks"
FAKE = ROOT / "rust" / "tools" / "fake-cli.py"
SCRATCH = Path(
    os.environ.get(
        "RELAY_SCRATCH",
        "/private/tmp/claude-501/-Users-theo-orca-general/scratchpad",
    )
) / "work-status"

# Il conduttore del lato Python: carica l'originale come modulo, gli devia la
# costante `ORCA` e chiama `main()`. Nessuna riga dell'originale viene toccata.
DRIVER = """import importlib.util, os, sys
spec = importlib.util.spec_from_file_location("work_status_orig", os.environ["WORK_STATUS_ORIGINAL"])
mod = importlib.util.module_from_spec(spec)
spec.loader.exec_module(mod)
mod.ORCA = os.environ["WORK_STATUS_ORCA"]
sys.exit(mod.main())
"""

SHIM = """#!/bin/sh
exec /usr/bin/env python3 "{fake}" {name} "$@"
"""


# ── il mondo finto ──────────────────────────────────────────────────────────

def carta(*repos):
    return {"repo": [{"nome": r} for r in repos]}


def elenco(*worktrees):
    return json.dumps({"ok": True, "result": {"worktrees": list(worktrees)}})


def copia(id_, path, name, status, branch, main=False):
    return {
        "id": id_,
        "path": path,
        "displayName": name,
        "workspaceStatus": status,
        "git": {"isMainWorktree": main, "branch": f"refs/heads/{branch}"},
    }


OK = {"stdout": json.dumps({"ok": True})}
NO = {"stdout": json.dumps({"ok": False})}

# Le due copie del riconciliatore: una di `packages`, una di `suite`.
PACK = copia("1", "/a/packages/x", "x", "in-review", "x")
SUITE = copia("2", "/a/suite/y", "y", "in-progress", "y")

GIT_REMOTI = [
    {"match": ["/a/packages/x", "remote"], "stdout": "git@github.com:Other-repo-Work/packages.git\n"},
    {"match": ["/a/suite/y", "remote"], "stdout": "https://github.com/Other-repo-Work/suite\n"},
    {"match": ["/a/packages", "remote"], "stdout": "git@github.com:Other-repo-Work/packages.git\n"},
]


def gh_dice(repo, richieste):
    return {
        "match": [f"Other-repo-Work/{repo}"],
        "stdout": json.dumps(richieste),
    }


def gh_muto(repo):
    return {"match": [f"Other-repo-Work/{repo}"], "rc": 1}


APERTA = [{"headRefName": "y", "state": "OPEN", "mergedAt": None}]
FUSA = [{"headRefName": "y", "state": "MERGED", "mergedAt": "2026-08-16T10:00:00Z"}]

# Il gesto della fusione interroga GitHub sul ramo della copia, non sul repo:
# `gh pr list --head <ramo>`. Le regole si distinguono per il `--head`, che e'
# l'unico argomento che cambia fra le due domande.
def gh_ramo(ramo, richieste):
    return {"match": ["--head", ramo], "stdout": json.dumps(richieste)}


RAMO_FUSO = [{"state": "MERGED", "mergedAt": "2026-08-18T09:00:00Z"}]
RAMO_APERTO = [{"state": "OPEN", "mergedAt": None}]


# Quando il comando nomina il numero, la domanda cambia forma: `gh pr view
# <numero>` invece di `gh pr list --head <ramo>`. E' il gesto reale — la
# richiesta #530 di `parole-side-table` e' stata fusa cosi', da una cartella che
# non era la copia — e la risposta porta il ramo, che e' come si trova la copia.
def gh_view(numero, ramo, stato):
    return {
        "match": ["view", str(numero)],
        "stdout": json.dumps({
            "headRefName": ramo,
            "state": stato,
            "mergedAt": "2026-08-18T09:34:05Z" if stato == "MERGED" else None,
        }),
    }

# `unlanded_commits`: quanti commit non vivono altrove, e — se ce ne sono — se
# fondere cambierebbe un byte. Due forme, «pulita» e «con lavoro dentro».
GIT_PULITO = GIT_REMOTI + [
    {"match": ["rev-list", "--count"], "stdout": "0\n"},
]
GIT_CON_RESIDUO = GIT_REMOTI + [
    {"match": ["rev-list", "--count"], "stdout": "7\n"},
    {"match": ["rev-parse"], "stdout": "aaaa\n"},
    {"match": ["merge-tree"], "stdout": "bbbb\n"},
]
# git muto: «non letto» non e' «zero», e su uno zero finto una copia verrebbe
# dichiarata completata.
GIT_MUTO_SUL_CONTO = GIT_REMOTI + [
    {"match": ["rev-list", "--count"], "rc": 1},
]

URL = "https://github.com/Other-repo-work/packages/pull/32"
CREATA = json.dumps(
    {"ok": True, "result": {"startupTerminal": {"handle": "term_setup"},
                            "agentTerminalHandle": "term_agent"}}
)


def bash(command, response=None):
    payload = {"tool_name": "Bash", "tool_input": {"command": command}}
    if response is not None:
        payload["tool_response"] = response
    return json.dumps(payload)


LISTA_DUE = elenco(
    copia("g", "/a/general", "GENERAL", "in-progress", "g"),
    copia("p", "/a/packages", "PACKAGES", "in-progress", "p"),
)
LISTA_GIA_IN_REVIEW = elenco(
    copia("g", "/a/general", "GENERAL", "in-progress", "g"),
    copia("p", "/a/packages", "PACKAGES", "in-review", "p"),
)


def orca_normale(listing, set_reply=OK):
    return [
        {"match": ["worktree", "list"], "stdout": listing},
        {"match": ["worktree", "set"], **set_reply},
        {"match": ["terminal", "close"], **OK},
    ]


# ── i casi ──────────────────────────────────────────────────────────────────

def casi():
    dentro = "cd /a/packages && gh pr create"
    return [
        # ── modo gancio: la richiesta appena fusa ─────────────────────────
        #
        # Il gesto gemello, aggiunto il 18/08/2026. Il riconciliatore gira a
        # SessionStart e fra un avvio e l'altro passano ore: quel giorno **7
        # copie su 13 dichiarate in-review avevano gia' la richiesta fusa**.
        ("un comando che non fonde niente",
         {"stdin": bash("gh pr merged"),
          "spec": {"orca": orca_normale(LISTA_GIA_IN_REVIEW), "git": GIT_PULITO}}),
        ("la fusione e' fallita: nessuno stato cambia",
         {"stdin": bash("cd /a/packages && gh pr merge", {"is_error": True}),
          "spec": {"orca": orca_normale(LISTA_GIA_IN_REVIEW), "git": GIT_PULITO,
                   "gh": [gh_ramo("p", RAMO_FUSO)]}}),
        ("CAMBIA STATO: la copia il cui lavoro e' atterrato passa a completata",
         {"stdin": bash("cd /a/packages && gh pr merge --squash", {"stdout": "merged."}),
          "spec": {"orca": orca_normale(LISTA_GIA_IN_REVIEW), "git": GIT_PULITO,
                   "gh": [gh_ramo("p", RAMO_FUSO)]}}),
        # ── e quando il comando nomina la richiesta, si segue la richiesta ──
        #
        # IL DIFETTO DEL 18/08/2026, con l'esemplare in piedi: `gh -R <repo> pr
        # merge <numero> --squash` da una cartella che non e' nessuna copia. La
        # forma non era nemmeno riconosciuta, e `parole-side-table` e' rimasta
        # «in revisione» con la #530 fusa e zero commit residui. Il `cwd` di
        # questi casi e' la HOME finta, che nessuna copia contiene: se si
        # tornasse a dedurre dalla cartella, resterebbero tutti a bocca asciutta.
        ("CAMBIA STATO: la richiesta numerata trova la copia dal suo ramo",
         {"stdin": bash("gh -R Other-repo-Work/packages pr merge 530 --squash",
                        {"stdout": "merged."}),
          "spec": {"orca": orca_normale(LISTA_GIA_IN_REVIEW), "git": GIT_PULITO,
                   "gh": [gh_view(530, "p", "MERGED")]}}),
        # IL MUTANTE CHE CONTA: il comando parte da `/a/general` e fonde la
        # richiesta di `/a/packages`. Marcare la cartella marcherebbe GENERAL.
        ("e non la copia da cui si e' digitato il comando",
         {"stdin": bash("cd /a/general && gh -R Other-repo-Work/packages pr merge 468",
                        {"stdout": "merged."}),
          "spec": {"orca": orca_normale(LISTA_GIA_IN_REVIEW), "git": GIT_PULITO,
                   "gh": [gh_view(468, "p", "MERGED")]}}),
        ("una richiesta che GitHub dice aperta non marca niente",
         {"stdin": bash("gh -R Other-repo-Work/packages pr merge 530",
                        {"stdout": "merged."}),
          "spec": {"orca": orca_normale(LISTA_GIA_IN_REVIEW), "git": GIT_PULITO,
                   "gh": [gh_view(530, "p", "OPEN")]}}),
        ("ne' una il cui ramo nessuna copia porta",
         {"stdin": bash("gh -R Other-repo-Work/packages pr merge 530",
                        {"stdout": "merged."}),
          "spec": {"orca": orca_normale(LISTA_GIA_IN_REVIEW), "git": GIT_PULITO,
                   "gh": [gh_view(530, "un-altro-ramo", "MERGED")]}}),
        ("ne' una che non dichiara un ramo affatto",
         {"stdin": bash("gh -R Other-repo-Work/packages pr merge 530",
                        {"stdout": "merged."}),
          "spec": {"orca": orca_normale(LISTA_GIA_IN_REVIEW), "git": GIT_PULITO,
                   "gh": [gh_view(530, "", "MERGED")]}}),
        ("GitHub muto sulla richiesta numerata: non si marca al buio",
         {"stdin": bash("gh -R Other-repo-Work/packages pr merge 530",
                        {"stdout": "merged."}),
          "spec": {"orca": orca_normale(LISTA_GIA_IN_REVIEW), "git": GIT_PULITO,
                   "gh": [{"match": ["view"], "rc": 1}]}}),
        # Due copie dello stesso repo sullo stesso ramo: qui non si indovina.
        ("due copie sullo stesso ramo dello stesso repo non si marcano",
         {"stdin": bash("gh -R Other-repo-Work/packages pr merge 530",
                        {"stdout": "merged."}),
          "spec": {"orca": orca_normale(elenco(
              copia("p", "/a/packages", "PACKAGES", "in-review", "p"),
              copia("q", "/a/packages-bis", "BIS", "in-review", "p"))),
              "git": GIT_PULITO + [{"match": ["/a/packages-bis", "remote"],
                                    "stdout": "git@github.com:Other-repo-Work/packages.git\n"}],
              "gh": [gh_view(530, "p", "MERGED")]}}),
        # Le guardie vecchie valgono anche qui.
        ("il ramo del checkout canonico non e' una lavorazione",
         {"stdin": bash("gh -R Other-repo-Work/packages pr merge 530",
                        {"stdout": "merged."}),
          "spec": {"orca": orca_normale(elenco(copia("t", "/a/tronco", "TRONCO",
                                                     "in-review", "develop", main=True))),
                   "git": GIT_PULITO, "gh": [gh_view(530, "develop", "MERGED")]}}),
        ("fusa per numero, ma il lavoro vive solo qui: resta in lavorazione",
         {"stdin": bash("gh -R Other-repo-Work/packages pr merge 530",
                        {"stdout": "merged."}),
          "spec": {"orca": orca_normale(LISTA_GIA_IN_REVIEW), "git": GIT_CON_RESIDUO,
                   "gh": [gh_view(530, "p", "MERGED")]}}),
        # Senza `-R` il repo lo dice la cartella: e' l'unico modo di saperlo per
        # un `gh pr merge <numero>` digitato dentro una copia.
        ("senza -R il repo lo dice la cartella del comando",
         {"stdin": bash("cd /a/packages && gh pr merge 530 --squash",
                        {"stdout": "merged."}),
          "spec": {"orca": orca_normale(LISTA_GIA_IN_REVIEW), "git": GIT_PULITO,
                   "gh": [gh_view(530, "p", "MERGED")]}}),
        ("senza -R e senza una cartella nota non si chiede nemmeno a GitHub",
         {"stdin": bash("gh pr merge 530 --squash", {"stdout": "merged."}),
          "spec": {"orca": orca_normale(LISTA_GIA_IN_REVIEW), "git": GIT_PULITO,
                   "gh": [gh_view(530, "p", "MERGED")]}}),
        ("l'URL della richiesta dice sia il repo sia il numero",
         {"stdin": bash("gh pr merge https://github.com/Other-repo-Work/packages/pull/530",
                        {"stdout": "merged."}),
          "spec": {"orca": orca_normale(LISTA_GIA_IN_REVIEW), "git": GIT_PULITO,
                   "gh": [gh_view(530, "p", "MERGED")]}}),
        # L'ALIAS SSH, che e' la forma vera di 27 copie su 28: prima lo slug
        # restava vuoto e nessuna copia poteva essere confermata.
        ("un remoto che passa da un host di comodo e' comunque GitHub",
         {"stdin": bash("gh -R Other-repo-Work/packages pr merge 530",
                        {"stdout": "merged."}),
          "spec": {"orca": orca_normale(LISTA_GIA_IN_REVIEW),
                   "git": [{"match": ["/a/packages", "remote"],
                            "stdout": "git@github.com-work:Other-repo-work/packages.git\n"},
                           {"match": ["rev-list", "--count"], "stdout": "0\n"}],
                   "gh": [gh_view(530, "p", "MERGED")]}}),
        ("GitHub non risponde: non si marca al buio",
         {"stdin": bash("cd /a/packages && gh pr merge", {"stdout": "merged."}),
          "spec": {"orca": orca_normale(LISTA_GIA_IN_REVIEW), "git": GIT_PULITO,
                   "gh": [{"match": ["--head"], "rc": 1}]}}),
        # Fusa non basta a dire finita: sette commit scritti dopo la fusione
        # erano il caso vero del 16/08/2026.
        ("fusa, ma il lavoro vive solo qui: resta in lavorazione",
         {"stdin": bash("cd /a/packages && gh pr merge", {"stdout": "merged."}),
          "spec": {"orca": orca_normale(LISTA_GIA_IN_REVIEW), "git": GIT_CON_RESIDUO,
                   "gh": [gh_ramo("p", RAMO_FUSO)]}}),
        ("e git che tace conta come residuo, non come pulito",
         {"stdin": bash("cd /a/packages && gh pr merge", {"stdout": "merged."}),
          "spec": {"orca": orca_normale(LISTA_GIA_IN_REVIEW), "git": GIT_MUTO_SUL_CONTO,
                   "gh": [gh_ramo("p", RAMO_FUSO)]}}),
        ("fondere dal checkout canonico non marca niente",
         {"stdin": bash("gh pr merge", {"stdout": "merged."}),
          "spec": {"orca": orca_normale(elenco(copia("t", "/a/tronco", "TRONCO",
                                                     "in-review", "develop", main=True))),
                   "git": GIT_PULITO, "gh": [gh_ramo("develop", RAMO_FUSO)]}}),
        # ── modo gancio: la richiesta appena aperta ────────────────────────
        ("uno strumento che non e' Bash: nessuna chiamata",
         {"stdin": json.dumps({"tool_name": "Write"}),
          "spec": {"orca": orca_normale(LISTA_DUE)}}),
        ("un comando che non apre niente",
         {"stdin": bash("gh pr list"),
          "spec": {"orca": orca_normale(LISTA_DUE)}}),
        ("il comando e' fallito: nessuna richiesta e' nata",
         {"stdin": bash(dentro, {"is_error": True, "stdout": URL}),
          "spec": {"orca": orca_normale(LISTA_DUE)}}),
        ("il comando e' stato interrotto",
         {"stdin": bash(dentro, {"interrupted": True, "stdout": URL}),
          "spec": {"orca": orca_normale(LISTA_DUE)}}),
        ("la frase citata in un messaggio di commit non marca niente",
         {"stdin": bash("git commit -m 'explain why gh pr create is watched'",
                        {"stdout": "[main d38275a] explain why"}),
          "spec": {"orca": orca_normale(LISTA_DUE), "git": GIT_REMOTI}}),
        ("gh pr create-draft e' un altro gesto",
         {"stdin": bash("cd /a/packages && gh pr create-draft", {"stdout": URL}),
          "spec": {"orca": orca_normale(LISTA_DUE), "git": GIT_REMOTI}}),
        ("CAMBIA STATO: la copia dove la richiesta e' nata passa in revisione",
         {"stdin": bash(dentro, {"stdout": URL}),
          "spec": {"orca": orca_normale(LISTA_DUE), "git": GIT_REMOTI}}),
        # IL CASO CHE CHIUDE UN BUCO TROVATO DAI MUTANTI. Il primo `cd` deve
        # essere una copia VERA, altrimenti invertire l'ordine non cambia
        # l'esito e il mutante sopravvive: con un `cd` relativo — che si risolve
        # sul cwd del gancio e non sulla cartella precedente — entrambi gli
        # ordini finiscono su `/a/general`, e il confronto resta verde.
        ("l'ultimo cd e' quello che conta",
         {"stdin": bash("cd /a/general && cd /a/packages && gh pr create", {"stdout": URL}),
          "spec": {"orca": orca_normale(LISTA_DUE), "git": GIT_REMOTI}}),
        ("un percorso da normalizzare arriva alla copia giusta",
         {"stdin": bash("cd /a/general/../packages && gh pr create", {"stdout": URL}),
          "spec": {"orca": orca_normale(LISTA_DUE), "git": GIT_REMOTI}}),
        ("un cd relativo si risolve sul cwd del gancio, non sul cd precedente",
         {"stdin": bash("cd /a/general && cd ../packages && gh pr create", {"stdout": URL}),
          "spec": {"orca": orca_normale(LISTA_DUE), "git": GIT_REMOTI}}),
        ("NESSUN CAMBIAMENTO: la copia e' gia' in revisione",
         {"stdin": bash(dentro, {"stdout": URL}),
          "spec": {"orca": orca_normale(LISTA_GIA_IN_REVIEW), "git": GIT_REMOTI}}),
        ("Orca non conferma: nessun messaggio, ma la chiamata c'e' stata",
         {"stdin": bash(dentro, {"stdout": URL}),
          "spec": {"orca": orca_normale(LISTA_DUE, NO), "git": GIT_REMOTI}}),
        ("una copia senza remoto non e' mai quella della richiesta",
         {"stdin": bash("gh pr create", {"stdout": URL}),
          "spec": {"orca": orca_normale(elenco(copia("g", "/a/general", "GENERAL",
                                                     "in-progress", "g"))),
                   "git": []}}),
        ("un remoto che parla di un altro repo non basta",
         {"stdin": bash(dentro, {"stdout": URL}),
          "spec": {"orca": orca_normale(LISTA_DUE),
                   "git": [{"match": ["/a/packages", "remote"],
                            "stdout": "git@github.com:Other-repo-Work/suite.git\n"}]}}),
        ("l'uscita non e' stata catturata: niente URL, niente marcatura",
         {"stdin": bash(dentro, {"stdout": "created."}),
          "spec": {"orca": orca_normale(LISTA_DUE), "git": GIT_REMOTI}}),
        ("Orca non risponde all'elenco: nessuna copia da marcare",
         {"stdin": bash(dentro, {"stdout": URL}),
          "spec": {"orca": [{"match": ["worktree", "list"], "rc": 1}], "git": GIT_REMOTI}}),
        ("l'URL arriva nel campo output invece che in stdout",
         {"stdin": bash(dentro, {"output": URL}),
          "spec": {"orca": orca_normale(LISTA_DUE), "git": GIT_REMOTI}}),
        ("la risposta dello strumento e' una stringa nuda",
         {"stdin": bash(dentro, URL),
          "spec": {"orca": orca_normale(LISTA_DUE), "git": GIT_REMOTI}}),

        # ── modo gancio: la tab di avvio lasciata indietro ──────────────────
        ("CHIUDE la tab di avvio che non e' quella dell'agente",
         {"stdin": bash("orca worktree create --name x --json",
                        {"stdout": json.dumps({"ok": True, "result": {
                            "startupTerminal": {"handle": "term_setup"}}})}),
          "spec": {"orca": orca_normale(LISTA_DUE)}}),
        ("la tab dell'agente non si chiude mai",
         {"stdin": bash("orca worktree create --json",
                        {"stdout": json.dumps({"ok": True, "result": {
                            "startupTerminal": {"handle": "term_a"},
                            "agentTerminalHandle": "term_a"}})}),
          "spec": {"orca": orca_normale(LISTA_DUE)}}),
        ("con l'agente su un'altra tab, quella di avvio si chiude",
         {"stdin": bash("orca worktree create --json", {"stdout": CREATA}),
          "spec": {"orca": orca_normale(LISTA_DUE)}}),
        ("senza handle non si chiude niente",
         {"stdin": bash("orca worktree create --json",
                        {"stdout": json.dumps({"ok": True, "result": {}})}),
          "spec": {"orca": orca_normale(LISTA_DUE)}}),
        ("un'uscita che non e' JSON non si legge",
         {"stdin": bash("orca worktree create", {"stdout": "created."}),
          "spec": {"orca": orca_normale(LISTA_DUE)}}),
        ("una creazione fallita non chiude niente",
         {"stdin": bash("orca worktree create", {"is_error": True, "stdout": CREATA}),
          "spec": {"orca": orca_normale(LISTA_DUE)}}),
        ("se Orca non conferma la chiusura, non lo si dice",
         {"stdin": bash("orca worktree create --json", {"stdout": CREATA}),
          "spec": {"orca": [{"match": ["terminal", "close"], **NO}]}}),
        ("orca worktree created non e' orca worktree create",
         {"stdin": bash("orca worktree created --json", {"stdout": CREATA}),
          "spec": {"orca": orca_normale(LISTA_DUE)}}),
        ("i due gesti nello stesso comando parlano tutti e due",
         {"stdin": bash("cd /a/packages && orca worktree create --json && gh pr create",
                        {"stdout": URL + "\n" + CREATA}),
          "spec": {"orca": orca_normale(LISTA_DUE), "git": GIT_REMOTI}}),

        # ── ingressi storti: nessuno deve uscire diverso da 0 ───────────────
        ("stdin vuoto", {"stdin": ""}),
        ("stdin che non e' JSON", {"stdin": "non sono json"}),
        ("un JSON che non e' un oggetto: due guasti annotati",
         {"stdin": "[1, 2, 3]"}),
        ("una stringa JSON al posto del payload", {"stdin": '"ciao"'}),
        ("tool_input che non e' un dizionario",
         {"stdin": json.dumps({"tool_name": "Bash", "tool_input": "ciao"})}),
        ("un comando che non e' testo",
         {"stdin": json.dumps({"tool_name": "Bash", "tool_input": {"command": 42}})}),
        ("senza tool_input non c'e' comando",
         {"stdin": json.dumps({"tool_name": "Bash"})}),

        # ── Orca che risponde storto ────────────────────────────────────────
        # Sono i punti in cui l'originale fa `.get` su qualcosa che potrebbe non
        # essere un dizionario: li' muore in silenzio e annota il guasto. Un
        # porto che invece prosegue non sbaglia il verdetto — sbaglia la traccia,
        # e domani nessuno saprebbe che quel giro si e' rotto.
        ("Orca risponde con un elenco invece che con un dizionario",
         {"stdin": bash(dentro, {"stdout": URL}),
          "spec": {"orca": [{"match": ["worktree", "list"], "stdout": "[1, 2]"}],
                   "git": GIT_REMOTI}}),
        ("Orca risponde con un elenco vuoto: vale come niente",
         {"stdin": bash(dentro, {"stdout": URL}),
          "spec": {"orca": [{"match": ["worktree", "list"], "stdout": "[]"}],
                   "git": GIT_REMOTI}}),
        ("il campo result non e' un dizionario",
         {"stdin": bash(dentro, {"stdout": URL}),
          "spec": {"orca": [{"match": ["worktree", "list"],
                             "stdout": json.dumps({"result": [1, 2]})}],
                   "git": GIT_REMOTI}}),
        ("worktrees non e' un elenco",
         {"stdin": bash(dentro, {"stdout": URL}),
          "spec": {"orca": [{"match": ["worktree", "list"],
                             "stdout": json.dumps({"result": {"worktrees": {"a": 1}}})}],
                   "git": GIT_REMOTI}}),
        ("una copia dell'elenco non e' un dizionario",
         {"stdin": bash(dentro, {"stdout": URL}),
          "spec": {"orca": [{"match": ["worktree", "list"],
                             "stdout": json.dumps({"result": {"worktrees": ["ciao"]}})}],
                   "git": GIT_REMOTI}}),
        ("riconcilia: una copia storta dopo una buona ferma il giro a meta'",
         {"argv": ["--riconcilia"], "carta": carta("suite"),
          "spec": {"orca": orca_normale(json.dumps({"ok": True, "result": {
              "worktrees": [SUITE, "ciao"]}})),
              "gh": [gh_dice("suite", APERTA)], "git": GIT_REMOTI}}),

        # ── la valvola ─────────────────────────────────────────────────────
        ("valvola STATO_LAVORAZIONE=off: non parte niente",
         {"stdin": bash(dentro, {"stdout": URL}),
          "env": {"STATO_LAVORAZIONE": "off"},
          "spec": {"orca": orca_normale(LISTA_DUE), "git": GIT_REMOTI}}),
        ("valvola con un altro valore: resta accesa",
         {"stdin": bash(dentro, {"stdout": URL}),
          "env": {"STATO_LAVORAZIONE": "acceso"},
          "spec": {"orca": orca_normale(LISTA_DUE), "git": GIT_REMOTI}}),

        # ── riconciliazione ────────────────────────────────────────────────
        ("riconcilia: senza la carta dei repo non parte niente",
         {"argv": ["--riconcilia"], "carta": None,
          "spec": {"orca": orca_normale(elenco(PACK, SUITE))}}),
        ("riconcilia: Orca non elenca niente",
         {"argv": ["--riconcilia"], "carta": carta("packages", "suite"),
          "spec": {"orca": [{"match": ["worktree", "list"], "stdout": elenco()}]}}),
        ("IL DIFETTO NOTO: nessun repo risponde, non si riscrive niente",
         {"argv": ["--riconcilia"], "carta": carta("packages", "suite"),
          "spec": {"orca": orca_normale(elenco(PACK, SUITE)),
                   "gh": [gh_muto("packages"), gh_muto("suite")],
                   "git": GIT_REMOTI}}),
        ("IL DIFETTO NOTO: un repo muto lascia stare la sua copia",
         {"argv": ["--riconcilia"], "carta": carta("packages", "suite"),
          "spec": {"orca": orca_normale(elenco(PACK, SUITE)),
                   "gh": [gh_muto("packages"), gh_dice("suite", APERTA)],
                   "git": GIT_REMOTI}}),
        ("riconcilia: un 503 alla prima non perde la risposta",
         {"argv": ["--riconcilia"], "carta": carta("suite"),
          "spec": {"orca": orca_normale(elenco(SUITE)),
                   "gh": [{"match": ["Other-repo-Work/suite"],
                           "seq": [{"rc": 1}, {"rc": 1},
                                   {"rc": 0, "stdout": json.dumps(APERTA)}]}],
                   "git": GIT_REMOTI}}),
        ("riconcilia: un repo che risponde vuoto ha comunque risposto",
         {"argv": ["--riconcilia"], "carta": carta("suite"),
          "spec": {"orca": orca_normale(elenco(
              copia("2", "/a/suite/y", "y", "in-review", "y"))),
              "gh": [gh_dice("suite", [])], "git": GIT_REMOTI}}),
        ("riconcilia: richiesta unita e niente rimasto qui -> completata",
         {"argv": ["--riconcilia"], "carta": carta("suite"),
          "spec": {"orca": orca_normale(elenco(SUITE)),
                   "gh": [gh_dice("suite", FUSA)],
                   "git": GIT_REMOTI + [
                       {"match": ["rev-list"], "stdout": "0\n"}]}}),
        ("DOPO UNO SQUASH: i commit sono qui ma il contenuto e' confluito",
         {"argv": ["--riconcilia"], "carta": carta("suite"),
          "spec": {"orca": orca_normale(elenco(SUITE)),
                   "gh": [gh_dice("suite", FUSA)],
                   "git": GIT_REMOTI + [
                       {"match": ["rev-list"], "stdout": "6\n"},
                       {"match": ["rev-parse"], "stdout": "T1\n"},
                       {"match": ["merge-tree"], "stdout": "T1\n"}]}}),
        ("riconcilia: commit che vivono solo qui -> resta in lavorazione",
         {"argv": ["--riconcilia"], "carta": carta("suite"),
          "spec": {"orca": orca_normale(elenco(
              copia("2", "/a/suite/y", "y", "completed", "y"))),
              "gh": [gh_dice("suite", FUSA)],
              "git": GIT_REMOTI + [
                  {"match": ["rev-list"], "stdout": "3\n"},
                  {"match": ["rev-parse"], "stdout": "T1\n"},
                  {"match": ["merge-tree"], "stdout": "T2\n"}]}}),
        ("riconcilia: git muto sul conteggio non e' un albero pulito",
         {"argv": ["--riconcilia"], "carta": carta("suite"),
          "spec": {"orca": orca_normale(elenco(
              copia("2", "/a/suite/y", "y", "completed", "y"))),
              "gh": [gh_dice("suite", FUSA)], "git": GIT_REMOTI}}),
        ("riconcilia: un checkout canonico non e' mai in revisione",
         {"argv": ["--riconcilia"], "carta": carta("suite"),
          "spec": {"orca": orca_normale(elenco(
              copia("3", "/a/suite", "suite", "in-review", "main", main=True))),
              "gh": [gh_dice("suite", APERTA)],
              "git": [{"match": ["/a/suite", "remote"],
                       "stdout": "git@github.com:Other-repo-Work/suite.git\n"}]}}),
        ("riconcilia: una copia senza remoto si giudica come le altre",
         {"argv": ["--riconcilia"], "carta": carta("suite"),
          "spec": {"orca": orca_normale(elenco(
              copia("4", "/a/altrove/z", "z", "completed", "z"))),
              "gh": [gh_dice("suite", APERTA)], "git": []}}),
        ("riconcilia: Orca non conferma la scrittura",
         {"argv": ["--riconcilia"], "carta": carta("suite"),
          "spec": {"orca": orca_normale(elenco(SUITE), NO),
                   "gh": [gh_dice("suite", APERTA)], "git": GIT_REMOTI}}),
        ("A SECCO: dice cosa farebbe e non chiama Orca",
         {"argv": ["--riconcilia", "--secco"], "carta": carta("packages", "suite"),
          "spec": {"orca": orca_normale(elenco(PACK, SUITE)),
                   "gh": [gh_dice("packages", []), gh_dice("suite", APERTA)],
                   "git": GIT_REMOTI}}),
        ("riconcilia: niente da cambiare, ma il giro lascia la sua riga",
         {"argv": ["--riconcilia"], "carta": carta("suite"),
          "spec": {"orca": orca_normale(elenco(
              copia("2", "/a/suite/y", "y", "in-review", "y"))),
              "gh": [gh_dice("suite", APERTA)], "git": GIT_REMOTI}}),
        ("riconcilia con la valvola spenta",
         {"argv": ["--riconcilia"], "carta": carta("suite"),
          "env": {"STATO_LAVORAZIONE": "off"},
          "spec": {"orca": orca_normale(elenco(SUITE)),
                   "gh": [gh_dice("suite", APERTA)], "git": GIT_REMOTI}}),
        ("riconcilia: la carta e' illeggibile",
         {"argv": ["--riconcilia"], "carta": "{non sono json",
          "spec": {"orca": orca_normale(elenco(PACK, SUITE))}}),
    ]


# ── esecuzione ──────────────────────────────────────────────────────────────

def prepara(home: Path, case: dict) -> dict:
    shutil.rmtree(home, ignore_errors=True)
    (home / ".claude" / "state").mkdir(parents=True)
    (home / ".claude" / "scripts").mkdir(parents=True)
    # Senza `hook_log` il gancio Python cade nel suo `except ImportError` e non
    # registra niente: il confronto sul registro diventerebbe vuoto contro vuoto.
    shutil.copy(ROOT / "scripts" / "hook_log.py", home / ".claude" / "scripts")

    c = case.get("carta", "assente")
    if c not in (None, "assente"):
        d = home / "other-repo" / "work" / ".claude"
        d.mkdir(parents=True)
        testo = c if isinstance(c, str) else json.dumps(c)
        (d / "repo-in-carico.json").write_text(testo)

    (home / "spec.json").write_text(json.dumps(case.get("spec", {})))
    finta = home / "bin"
    finta.mkdir()
    for name in ("orca", "gh", "git"):
        p = finta / name
        p.write_text(SHIM.format(fake=FAKE, name=name))
        p.chmod(0o755)
    (home / "driver.py").write_text(DRIVER)

    env = dict(os.environ)
    env.pop("STATO_LAVORAZIONE", None)
    env.update({
        "HOME": str(home),
        "PATH": f"{finta}:{env.get('PATH', '')}",
        "WORK_STATUS_ORCA": str(finta / "orca"),
        "WORK_STATUS_ORIGINAL": str(HOOK),
        "FAKE_LOG": str(home / "calls.jsonl"),
        "FAKE_SPEC": str(home / "spec.json"),
        "FAKE_STATE": str(home / "fake-state.json"),
    })
    env.update(case.get("env", {}))
    return env


def chiamate(home: Path):
    try:
        righe = (home / "calls.jsonl").read_text().splitlines()
    except OSError:
        return []
    return [r.replace(str(home), "<HOME>") for r in righe if r.strip()]


def registro(home: Path):
    """Le righe di `ganci.jsonl`, senza l'istante ma con la sua chiave."""
    try:
        righe = (home / ".claude" / "state" / "ganci.jsonl").read_text().splitlines()
    except OSError:
        return []
    fuori = []
    for r in righe:
        if not r.strip():
            continue
        testa, _, coda = r.partition(', "gancio"')
        fuori.append(("<t>" if testa.startswith('{"t": ') else testa) + ', "gancio"' + coda)
    return fuori


def guasti(home: Path):
    """Solo le righe che dicono QUALE passo si e' rotto: il traceback no."""
    try:
        testo = (home / ".claude" / "state" / "work-status-failures.log").read_text()
    except OSError:
        return []
    return [r for r in testo.splitlines() if r.startswith("--- ")]


def esegui(side: str, case: dict, home: Path) -> dict:
    env = prepara(home, case)
    argv = case.get("argv", [])
    cmd = ([sys.executable, str(home / "driver.py"), *argv] if side == "python"
           else [str(BINARY), "work-status", *argv])
    p = subprocess.run(cmd, input=case.get("stdin", ""), capture_output=True,
                       text=True, timeout=120, cwd=str(home), env=env)
    return {
        "exit": p.returncode,
        "stdout": p.stdout.replace(str(home), "<HOME>"),
        "stderr": p.stderr.replace(str(home), "<HOME>"),
        "chiamate": chiamate(home),
        "registro": registro(home),
        "guasti": guasti(home),
    }


def porte_chiuse(py: Path, rs: Path) -> bool:
    """Nessuna delle due esecuzioni puo' raggiungere Orca vera. Si verifica.

    La sonda usa il modo gancio, che con Orca vera arriverebbe al massimo a un
    `worktree list` — una lettura. Il `worktree set`, che e' l'unica scrittura di
    questo gancio, resta irraggiungibile perche' la copia scelta si cerca sotto
    la HOME finta, che non e' registrata da nessuna parte.
    """
    sonda = {
        "stdin": bash("cd /a/packages && gh pr create", {"stdout": URL}),
        "spec": {"orca": [{"match": ["worktree", "list"], "stdout": LISTA_DUE}]},
        "git": [],
    }
    ok = True
    for side, home in (("python", py), ("rust", rs)):
        env = prepara(home, sonda)
        trovato = shutil.which("orca", path=env["PATH"])
        if trovato != str(home / "bin" / "orca"):
            print(f"  FERMO: dal {side} `orca` si risolve in {trovato}, non nel finto")
            ok = False
        esito = esegui(side, sonda, home)
        if not any(c.startswith('["orca"') for c in esito["chiamate"]):
            print(f"  FERMO: il lato {side} non ha chiamato il finto `orca`: "
                  "una prova su Orca vera riscriverebbe lo stato di copie che lavorano")
            ok = False
    return ok


def main() -> int:
    if not BINARY.exists():
        print(f"manca il binario: {BINARY}\n  cargo build --release", file=sys.stderr)
        return 2
    if not HOOK.exists():
        print(f"manca l'originale: {HOOK}", file=sys.stderr)
        return 2
    SCRATCH.mkdir(parents=True, exist_ok=True)
    py, rs = SCRATCH / "home-python", SCRATCH / "home-rust"

    if not porte_chiuse(py, rs):
        return 2

    divergenze = 0
    prove = casi()
    for nome, case in prove:
        try:
            attesa = esegui("python", case, py)
            ottenuta = esegui("rust", case, rs)
        except Exception as exc:                       # pragma: no cover
            print(f"  ERRORE      {nome}: {exc}")
            divergenze += 1
            continue
        diffs = [k for k in attesa if attesa[k] != ottenuta[k]]
        if not diffs:
            n = len(attesa["chiamate"])
            print(f"  uguale      {nome}  ({n} chiamate)")
            continue
        divergenze += 1
        print(f"  DIVERGE     {nome}  -> {', '.join(diffs)}")
        for k in diffs:
            print(f"      python {k}: {str(attesa[k])[:400]!r}")
            print(f"      rust   {k}: {str(ottenuta[k])[:400]!r}")

    print()
    if divergenze:
        print(f"{divergenze} divergenze su {len(prove)} casi")
    else:
        print(f"{len(prove)} casi, nessuna divergenza")
    return 1 if divergenze else 0


if __name__ == "__main__":
    sys.exit(main())
