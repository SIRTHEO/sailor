import { useEffect, useMemo, useRef, useState } from "react";
import {
  Background,
  Controls,
  MiniMap,
  ReactFlow,
  applyEdgeChanges,
  applyNodeChanges,
  type Connection,
  type Edge,
  type EdgeChange,
  type Node,
  type NodeChange,
  type ReactFlowInstance,
} from "@xyflow/react";
import "@xyflow/react/dist/style.css";

import {
  FlowBandNode,
  StepNode,
  STATE_COLOR,
  StepRunContext,
  StepUsageContext,
  type StepNodeData,
} from "./StepNode";
import { stepUsageOfRun, type StepUsage } from "./stepusage";
import { stepStatesOfCanvas, stepStatesOfRun } from "./runstate";
import { BlankCanvas } from "./BlankCanvas";
import { Now } from "./Now";
import { History, lastedOf, outcomeOf, whenOf } from "./History";
import { useAsk, useClock } from "./ask";
import { Installed } from "./Installed";
import { Manual } from "./Manual";
import { Terminals } from "./Terminals";
import { StepEditor } from "./StepEditor";
import { Toolbar } from "./Toolbar";
import { RunContext, TriggerNode, triggerNodeId, type RunControls, type TriggerState } from "./TriggerNode";
import { RunConsole, type ConsoleMode } from "./RunConsole";
import { StepHistory } from "./StepHistory";
import { StepLive } from "./StepLive";
import { buildUnifiedLayout, nodeId, splitNodeId, wouldCycle } from "./layout";
import { SAMPLE, SAMPLE_RUN } from "./sample";
import {
  deleteFlow,
  discoverTools,
  executionHistory,
  flowTrigger,
  insideTheWindow,
  knownRuns,
  listenToRuns,
  loadFlows,
  runSnapshot,
  runUsage,
  saveFlow,
  startRun,
  type Execution,
  type RunEvent,
  type RunSnapshot,
} from "./engine";
import {
  DEFAULT_ACTION_FOR_KIND,
  KNOWN_ACTIONS,
  type BrokenFlow,
  type FlowEntry,
  type FlowFile,
  type RunUsage,
  type Step,
  type StepKind,
  type StepRun,
} from "./flow";
import { MODEL_KEY, type Tool, type ToolDiscovery } from "./tools";

const nodeTypes = { step: StepNode, flowBand: FlowBandNode, trigger: TriggerNode };

/** Quanto spazio prende l'innesco a sinistra della sua corsia. */
const TRIGGER_WIDTH = 240;
const TRIGGER_GAP = 48;
const TRIGGER_TOP = 54;

/**
 * Dentro il guscio o in un browser: si decide una volta sola, all'avvio.
 * Cambia cosa la tela mostra prima ancora del primo giro di caricamento, e
 * chiederlo a ogni render darebbe la stessa risposta.
 */
const NATIVE = insideTheWindow();

/** Da dove vengono i flussi che si stanno guardando. */
type Source = "loading" | "sample" | "engine" | "failed";

/**
 * I posti della finestra.
 *
 * **LA FINESTRA SI APRE SU «ADESSO», NON SULLA TELA**, ed è un cambio di
 * scuola, non di disposizione. Aprire sull'inventario dei flussi — come fanno
 * n8n, Zapier e Dify — risponde alla domanda «cosa potrei far girare»; chi
 * riapre la finestra ne ha un'altra in testa, «cosa sta succedendo», e fino a
 * stasera per rispondere doveva andarsela a cercare. La tela non sparisce:
 * diventa il posto dove si va per guardare dentro.
 */
type Place = "now" | "history" | "flows" | "installed" | "manual" | "terminals";

/**
 * Three views of ONE flow, not three places of the window. The graph is where
 * it is composed, the code is what lands on the disk, the runs are what came
 * out of it: the same thing looked at from three sides.
 */
/* Only the graph: "Code" was a data file dressed as source, and "Runs" is
   already the «Adesso» and «Cronologia» places. `FlowCode` and `FlowRuns` stay
   below, unmounted, with what they measured about the engine. */
type FlowView = "graph";

/**
 * Un flusso in modifica: quello che si vede e quello che è già sul disco.
 * `saved: null` è un flusso appena creato qui e mai scritto — non è «uguale al
 * disco», è «il disco non lo conosce», e le due cose portano a gesti diversi
 * (si salva sempre, si cancella senza chiedere niente al motore).
 */
interface WorkingFlow {
  flow: FlowFile;
  saved: FlowFile | null;
}

function isDirty(working: WorkingFlow): boolean {
  if (working.saved === null) return true;
  return JSON.stringify(working.flow) !== JSON.stringify(working.saved);
}

/** Divide gli ingressi in flussi modificabili (mappa per nome) e flussi rotti (lista). */
function splitEntries(entries: FlowEntry[]): { flows: Map<string, WorkingFlow>; broken: BrokenFlow[] } {
  const flows = new Map<string, WorkingFlow>();
  const broken: BrokenFlow[] = [];
  for (const entry of entries) {
    if (entry.state === "loaded") {
      flows.set(entry.flow.id, { flow: entry.flow, saved: entry.flow });
    } else {
      broken.push(entry.broken);
    }
  }
  return { flows, broken };
}

/**
 * Unisce i fatti di una corsa per numero d'ordine.
 *
 * Un fatto può arrivare due volte — una dall'elenco di quello che è già
 * successo, una dall'ascolto — e i due canali si sovrappongono proprio nel
 * momento in cui la vista si apre. Unire per `seq` è l'unico modo di non
 * mostrare due volte lo stesso passo senza dipendere da quale dei due canali
 * arriva prima. Se non c'è niente di nuovo torna l'array di partenza, così chi
 * confronta le identità non ridisegna per un fatto già noto.
 */
function mergeEvents(existing: RunEvent[], incoming: RunEvent[]): RunEvent[] {
  const fresh = incoming.filter(
    (event) => !existing.some((known) => known.seq === event.seq),
  );
  if (fresh.length === 0) return existing;
  return [...existing, ...fresh].sort((a, b) => a.seq - b.seq);
}

/** Un nome libero per un flusso nuovo: numerato, mai in collisione. */
function freeFlowName(taken: Map<string, WorkingFlow>): string {
  let n = 1;
  while (taken.has(`flusso-${n}`)) n += 1;
  return `flusso-${n}`;
}

export default function App() {
  // LA TELA NASCE COME IL DISCO, NON COME UN ESEMPIO. Dentro il guscio si parte
  // vuoti e si aspetta la risposta del motore: mostrare l'esempio per un
  // istante e poi toglierlo fa vedere flussi che sul disco non ci sono. Fuori
  // dal guscio (`npm run dev` in un browser) il motore non esiste e l'esempio è
  // l'unica cosa da mostrare — la barra dichiara sempre quale dei due si sta
  // guardando.
  const [flows, setFlows] = useState<Map<string, WorkingFlow>>(() =>
    NATIVE ? new Map<string, WorkingFlow>() : splitEntries(SAMPLE).flows,
  );
  const [broken, setBroken] = useState<BrokenFlow[]>(() => (NATIVE ? [] : splitEntries(SAMPLE).broken));
  const [source, setSource] = useState<Source>(NATIVE ? "loading" : "sample");
  const [failure, setFailure] = useState<string | null>(null);

  const [place, setPlace] = useState<Place>("now");
  const [flowView] = useState<FlowView>("graph");

  // Il fuoco è del ramo, non della tela: la colonna indica un percorso dentro
  // il grafo unico, non sceglie più quale grafo mostrare.
  const [focusName, setFocusName] = useState<string | null>(null);
  const [selectedNode, setSelectedNode] = useState<string | null>(null);

  const [saving, setSaving] = useState<Set<string>>(new Set());
  const [saveErrors, setSaveErrors] = useState<Record<string, string>>({});

  const [discovery, setDiscovery] = useState<ToolDiscovery>(() =>
    NATIVE ? { state: "asking" } : { state: "mute", why: "fuori dal guscio: gli strumenti li conosce il motore" },
  );

  useEffect(() => {
    if (!NATIVE) return;
    let dropped = false;
    loadFlows()
      .then((loaded) => {
        if (dropped) return;
        const split = splitEntries(loaded);
        setFlows(split.flows);
        setBroken(split.broken);
        setSource("engine");
        setFocusName(null);
        setSelectedNode(null);
      })
      .catch((error: unknown) => {
        if (dropped) return;
        // Un motore muto non si maschera con l'esempio: chi guarda vedrebbe
        // flussi che sul disco non ci sono.
        setSource("failed");
        setFailure(String(error));
      });
    return () => {
      dropped = true;
    };
  }, []);

  // GLI STRUMENTI SI CHIEDONO, NON SI SANNO. `discover_tools` nasce in un altro
  // cantiere: se non risponde ancora, il pannello lo dice e lascia scrivere
  // l'identificativo a mano — nessuna schermata bianca, e nessun elenco finto
  // per far sembrare che funzioni.
  useEffect(() => {
    if (!NATIVE) return;
    let dropped = false;
    discoverTools()
      .then((found) => {
        if (dropped) return;
        setDiscovery({ state: "ready", tools: found });
      })
      .catch((error: unknown) => {
        if (dropped) return;
        setDiscovery({ state: "mute", why: String(error) });
      });
    return () => {
      dropped = true;
    };
  }, []);

  const tools = discovery.state === "ready" ? discovery.tools : EMPTY_TOOLS;


  const anyDirty = useMemo(() => Array.from(flows.values()).some(isDirty), [flows]);

  // Chi chiude la finestra non deve scoprire dopo di aver perso il lavoro.
  // `beforeunload` è un ripiego onesto: nel guscio nativo la chiusura è un
  // evento della finestra, non una navigazione di pagina, e non è detto che
  // questo listener venga interpellato — non l'ho potuto provare su una
  // chiusura vera. La difesa che regge comunque è il bollino «non salvato»,
  // sempre visibile, nella barra e accanto a ogni flusso.
  useEffect(() => {
    function handleBeforeUnload(event: BeforeUnloadEvent) {
      if (!anyDirty) return;
      event.preventDefault();
      event.returnValue = "";
    }
    window.addEventListener("beforeunload", handleBeforeUnload);
    return () => window.removeEventListener("beforeunload", handleBeforeUnload);
  }, [anyDirty]);

  const flowList = useMemo(
    () => Array.from(flows.entries()).map(([name, working]) => ({ name, flow: working.flow })),
    [flows],
  );

  // I modelli già scritti negli altri passi: il suggerimento più onesto che
  // esista, perché viene dai flussi veri invece che da un elenco inventato.
  const usedModels = useMemo(() => {
    const seen = new Set<string>();
    for (const { flow } of flowList) {
      for (const step of flow.graph.steps) {
        const model = step.with?.[MODEL_KEY];
        if (typeof model === "string" && model !== "") seen.add(model);
      }
    }
    return Array.from(seen);
  }, [flowList]);

  // ── le corse: farne partire una, e guardarla ──────────────────────────
  //
  // LE CORSE NON VIVONO QUI. Vivono nel guscio, che le esegue su un thread
  // proprio; questa mappa è la copia che la finestra tiene per disegnarle. È la
  // ragione per cui chiudere la vista o ricaricare la pagina non ferma niente —
  // e per cui, al montaggio, la prima cosa da fare è chiedere al guscio cosa
  // sta già girando invece di ripartire da un elenco vuoto.
  //
  // Si chiamano «esecuzioni» e non «corse» per non confondersi con `runs` qui
  // sotto, che è un'altra cosa: lo stato dei passi con cui la tela li colora.
  const [executions, setExecutions] = useState<Map<string, RunSnapshot>>(new Map());
  const [triggers, setTriggers] = useState<Map<string, TriggerState>>(new Map());
  const [mandates, setMandates] = useState<Record<string, string>>({});
  const [starting, setStarting] = useState<Set<string>>(new Set());
  const [runErrors, setRunErrors] = useState<Record<string, string>>({});
  const [watching, setWatching] = useState<string | null>(null);
  const [consoleMode, setConsoleMode] = useState<ConsoleMode>("inline");

  function absorb(snapshot: RunSnapshot) {
    setExecutions((prev) => {
      const current = prev.get(snapshot.run_id);
      const next = new Map(prev);
      next.set(snapshot.run_id, {
        ...snapshot,
        events: mergeEvents(current?.events ?? [], snapshot.events),
      });
      return next;
    });
  }

  // Cosa sta già girando: si chiede una volta, all'apertura. Una corsa avviata
  // prima di un ricaricamento della pagina è viva nel guscio, e senza questa
  // domanda diventerebbe invisibile pur continuando a lavorare.
  useEffect(() => {
    if (!NATIVE) return;
    let dropped = false;
    knownRuns()
      .then((found) => {
        if (dropped) return;
        for (const snapshot of found) absorb(snapshot);
      })
      // Nessuna corsa da mostrare non è un guasto da sbattere in faccia: il
      // pulsante di partenza funziona lo stesso, e un errore qui parlerebbe di
      // una vista che nessuno ha ancora chiesto.
      .catch(() => {});
    return () => {
      dropped = true;
    };
  }, []);

  // Perché la vista non si aggiorna da sola, quando non lo fa. Vuoto significa
  // che l'ascolto è attaccato.
  const [listenFailure, setListenFailure] = useState<string | null>(null);

  // L'ascolto di quello che succede. Si attacca una volta e resta: staccarlo e
  // riattaccarlo a ogni fatto perderebbe proprio i fatti che arrivano nel mezzo.
  useEffect(() => {
    if (!NATIVE) return;
    let unlisten: (() => void) | null = null;
    let dropped = false;
    void listenToRuns((event) => {
      setExecutions((prev) => {
        const current = prev.get(event.run_id);
        if (!current) return prev;
        const events = mergeEvents(current.events, [event]);
        if (events === current.events) return prev;
        const payload = event.payload as { status?: unknown } | null;
        const status =
          event.kind === "run_ended" && typeof payload?.status === "string"
            ? payload.status
            : current.status;
        const next = new Map(prev);
        next.set(event.run_id, { ...current, events, status });
        return next;
      });
    }).then((result) => {
      if ("why" in result) {
        setListenFailure(result.why);
        return;
      }
      if (dropped) result.stop();
      else unlisten = result.stop;
    });
    return () => {
      dropped = true;
      unlisten?.();
    };
  }, []);


  // Come si innesca ciascun flusso lo dice il guscio, non questa pagina: la
  // regola su dove va a finire una consegna è una sola, e sta di là.
  const known = useMemo(() => flowList.map(({ name }) => name).join("\u0000"), [flowList]);
  useEffect(() => {
    if (!NATIVE) return;
    let dropped = false;
    for (const name of known.split("\u0000").filter((entry) => entry !== "")) {
      setTriggers((prev) => {
        if (prev.has(name)) return prev;
        flowTrigger(name)
          .then((trigger) => {
            if (!dropped) setTriggers((now) => new Map(now).set(name, { state: "ready", trigger }));
          })
          .catch((error: unknown) => {
            if (!dropped) setTriggers((now) => new Map(now).set(name, { state: "mute", why: String(error) }));
          });
        return new Map(prev).set(name, { state: "asking" });
      });
    }
    return () => {
      dropped = true;
    };
  }, [known]);

  const anyRunning = useMemo(
    () => Array.from(executions.values()).some((run) => run.status === "running"),
    [executions],
  );

  // L'orologio dei contatori. BATTE SOLO MENTRE QUALCOSA GIRA: un secondo fisso
  // che ridisegna la tela per sempre è un costo che nessuno guarda, e su questa
  // tela un ridisegno di troppo è già bastato una volta a mandarla in ciclo.
  const [now, setNow] = useState(() => Math.floor(Date.now() / 1000));
  useEffect(() => {
    if (!anyRunning) return;
    const tick = window.setInterval(() => setNow(Math.floor(Date.now() / 1000)), 1000);
    return () => window.clearInterval(tick);
  }, [anyRunning]);

  // LA RETE DI SICUREZZA. Un canale di eventi può non attaccarsi, o perdere un
  // fatto: in tutti e due i casi la finestra continuerebbe a disegnare l'ultimo
  // stato ricevuto — «in corso da 00:30» su una corsa finita — che è
  // esattamente la bugia che questa vista non deve raccontare. Finché qualcosa
  // gira si richiede lo stato al guscio, e i fatti si uniscono per numero
  // d'ordine: quelli già arrivati dall'ascolto non si contano due volte.
  //
  // Interroga **solo mentre una corsa è viva**, e non a tela ferma.
  useEffect(() => {
    if (!NATIVE || !anyRunning) return;
    const live = Array.from(executions.values())
      .filter((run) => run.status === "running")
      .map((run) => run.run_id);
    const tick = window.setInterval(() => {
      for (const runId of live) void runSnapshot(runId).then(absorb).catch(() => {});
    }, 1000);
    return () => window.clearInterval(tick);
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [anyRunning, executions.size]);

  // QUANTO STA COSTANDO LA CORSA CHE SI GUARDA.
  //
  // **SI CHIEDE AL MOTORE, NON SI SOMMA QUI.** I totali li calcola
  // `ui::dashboard`, lo stesso codice che serve la pagina di `sailor ui`: una
  // seconda somma scritta in TypeScript darebbe due cifre per la stessa spesa, e
  // il 28/08 abbiamo già pagato il prezzo di una verità tenuta in due posti.
  //
  // **OGNI TRE SECONDI MENTRE GIRA, E UNA VOLTA QUANDO FINISCE.** Leggere la
  // spesa vuol dire aprire il deposito e scorrere le corse: è troppo per il
  // battito di un secondo con cui si aggiornano i passi, ed è poco per una cifra
  // che cambia solo quando un passo chiama un motore. `null` finché il deposito
  // non ha ancora proiettato la corsa — e allora non si mostra niente, invece di
  // mostrare uno zero che sembrerebbe una misura.
  const [usage, setUsage] = useState<RunUsage | null>(null);
  useEffect(() => {
    if (!NATIVE || !watching) {
      setUsage(null);
      return;
    }
    let alive = true;
    const ask = () => {
      void runUsage(watching)
        .then((found) => {
          if (alive) setUsage(found);
        })
        .catch(() => {});
    };
    ask();
    const watchedRun = executions.get(watching);
    if (watchedRun?.status !== "running") return () => {
      alive = false;
    };
    const tick = window.setInterval(ask, 3000);
    return () => {
      alive = false;
      window.clearInterval(tick);
    };
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [watching, executions.get(watching ?? "")?.status, executions.size]);

  /**
   * La spesa per passo della corsa guardata, pronta per i nodi.
   *
   * Le chiamate portano lo `step_id` ma non il flusso: lo mette qui chi sa
   * quale corsa si sta guardando, perché la chiave dei nodi è `flusso::passo` e
   * `verifica` esiste in più flussi.
   */
  const stepUsage = useMemo(() => {
    const watchedRun = watching ? executions.get(watching) : undefined;
    if (!watchedRun) return new Map<string, StepUsage>();
    return stepUsageOfRun(usage, watchedRun.flow);
  }, [usage, watching, executions]);

  /** La corsa più recente di un flusso: è quella che chi guarda intende. */
  const latestByFlow = useMemo(() => {
    const latest = new Map<string, RunSnapshot>();
    for (const run of executions.values()) {
      const current = latest.get(run.flow);
      if (!current || run.started_at >= current.started_at) latest.set(run.flow, run);
    }
    return latest;
  }, [executions]);

  const executionList = useMemo(
    () => Array.from(executions.values()).sort((a, b) => b.started_at - a.started_at),
    [executions],
  );

  async function handleRun(flowName: string) {
    const working = flows.get(flowName);
    if (!working) return;
    // UN FLUSSO SI ESEGUE DAL DISCO, NON DALLO SCHERMO. Il motore legge il
    // file: se ci sono modifiche non salvate, quello che parte non è quello che
    // si sta guardando — e chi preme lo deve decidere prima, non scoprirlo dai
    // risultati.
    if (isDirty(working)) {
      const question =
        working.saved === null
          ? `«${flowName}» non è mai stato salvato: il motore non lo troverebbe sul disco. Salvarlo e poi eseguirlo?`
          : `«${flowName}» ha modifiche non salvate: il motore eseguirebbe la versione sul disco. Salvarlo prima?`;
      if (!window.confirm(question)) return;
      await handleSave(flowName);
      if (saveErrors[flowName]) return;
    }

    setStarting((prev) => new Set(prev).add(flowName));
    setRunErrors((prev) => {
      if (!(flowName in prev)) return prev;
      const next = { ...prev };
      delete next[flowName];
      return next;
    });
    try {
      const text = mandates[flowName] ?? "";
      const started = await startRun(flowName, text.trim() === "" ? null : text);
      absorb({
        run_id: started.run_id,
        flow: started.flow,
        started_at: started.started_at,
        status: "running",
        events: [],
      });
      setWatching(started.run_id);
      // I fatti nati fra la partenza e l'inizio dell'ascolto stanno solo nel
      // guscio: si chiedono subito, e si uniscono per numero d'ordine.
      void runSnapshot(started.run_id)
        .then(absorb)
        .catch((error: unknown) => setRunErrors((prev) => ({ ...prev, [flowName]: String(error) })));
    } catch (error) {
      setRunErrors((prev) => ({ ...prev, [flowName]: String(error) }));
    } finally {
      setStarting((prev) => {
        const next = new Set(prev);
        next.delete(flowName);
        return next;
      });
    }
  }

  const controls: RunControls = useMemo(
    () => ({
      native: NATIVE,
      triggerOf: (name) =>
        triggers.get(name) ??
        (NATIVE
          ? { state: "asking" }
          : { state: "mute", why: "fuori dal guscio: nessun motore da innescare" }),
      runOf: (name) => latestByFlow.get(name),
      mandateOf: (name) => mandates[name] ?? "",
      onMandate: (name, text) => setMandates((prev) => ({ ...prev, [name]: text })),
      onRun: (name) => void handleRun(name),
      starting: (name) => starting.has(name),
      errorOf: (name) => runErrors[name],
      onWatch: (name) => {
        const run = latestByFlow.get(name);
        if (run) setWatching(run.run_id);
      },
    }),
    // eslint-disable-next-line react-hooks/exhaustive-deps
    [triggers, latestByFlow, mandates, starting, runErrors, flows, saveErrors],
  );

  const watched = watching ? executions.get(watching) : undefined;

  // GLI STATI D'ESEMPIO RESTANO ALL'ESEMPIO. Sui flussi veri la corsa non è
  // ancora letta dal deposito: passare qui `SAMPLE_RUN` colorerebbe di
  // «andato» e «rotto» dei passi che nessuno ha mai eseguito, ed è la specie
  // di bugia che una tela racconta senza che nessuno la smentisca.
  // `new Map()` scritto qui dentro sarebbe un oggetto nuovo a ogni render, e
  // basta quello: il disegno dipende da `runs`, il disegno cambia identità, un
  // effetto riscrive i nodi, che fa ripartire il render. La tela non si ferma
  // mai abbastanza per misurare i propri nodi, e chi guarda vede una tela vuota
  // con la minimappa piena — che è esattamente come si è presentata.
  const runs = useMemo(
    () => (source === "sample" ? SAMPLE_RUN : new Map<string, StepRun>()),
    [source],
  );

  // LO STATO VERO DEI NODI, e non passa di qui sopra apposta.
  //
  // `runs` entra nel disegno, quindi ogni suo cambiamento ricostruisce l'elenco
  // dei nodi — che è la cosa che questa tela non sopporta. Gli stati veri vanno
  // invece nel valore di un contesto: i nodi restano gli stessi oggetti, e a
  // ridisegnarsi è solo chi legge il contesto. Chiave `flusso::passo`, perché
  // fra i flussi veri tre identificativi sono già ripetuti.
  const stepStates = useMemo(
    () => stepStatesOfCanvas(executions.values()),
    [executions],
  );

  const layout = useMemo(() => buildUnifiedLayout(flowList, runs, focusName), [flowList, runs, focusName]);

  /**
   * I nodi di innesco, uno per corsia, a sinistra di dove comincia il grafo.
   *
   * Si aggiungono qui e non in `buildUnifiedLayout` per una ragione precisa:
   * quella funzione disegna quello che sta nel file del flusso, e l'innesco nel
   * file non c'è. Tenerlo fuori lascia visibile il confine fra ciò che il
   * motore conosce e ciò che questa finestra aggiunge.
   *
   * I `data` portano due stringhe stabili e nient'altro: lo stato della corsa
   * passa dal contesto. Se passasse di qui, ogni fatto in arrivo ricostruirebbe
   * l'intero elenco dei nodi — e su questa tela un elenco di nodi ricostruito
   * dentro un effetto è già bastato una volta a mandarla in ciclo infinito.
   */
  const canvas = useMemo(() => {
    const nodes: Node[] = [...layout.nodes];
    const edges: Edge[] = [...layout.edges];
    const byName = new Map(flowList.map(({ name, flow }) => [name, flow]));

    for (const [name, band] of layout.bands) {
      const id = triggerNodeId(name);
      nodes.push({
        id,
        type: "trigger",
        position: { x: band.x - TRIGGER_WIDTH - TRIGGER_GAP, y: band.y + TRIGGER_TOP },
        draggable: false,
        data: { flowName: name, color: band.color },
      });

      // L'innesco punta ai passi da cui il grafo comincia. Tratteggiato,
      // perché non è una dipendenza dichiarata nel file: è il gesto che mette
      // in moto quelli che non aspettano nessun altro.
      const flow = byName.get(name);
      for (const step of flow?.graph.steps ?? []) {
        if (step.deps.length > 0) continue;
        edges.push({
          id: `${id}->${nodeId(name, step.id)}`,
          source: id,
          target: nodeId(name, step.id),
          animated: false,
          selectable: false,
          style: {
            stroke: band.color,
            strokeDasharray: "2 5",
            opacity: focusName !== null && focusName !== name ? 0.2 : 0.7,
          },
        });
      }
    }

    return { nodes, edges };
  }, [layout, flowList, focusName]);

  const [nodes, setNodes] = useState<Node[]>([]);
  const [edges, setEdges] = useState<Edge[]>([]);
  useEffect(() => {
    setNodes(canvas.nodes);
    setEdges(canvas.edges);
  }, [canvas]);

  // Il fuoco muove la vista, non ricrea la disposizione: leggo l'ultimo
  // riquadro noto da un ref, così un'ingresso non ri-centra la tela mentre si
  // sta modificando un campo altrove.
  const bandsRef = useRef(layout.bands);
  bandsRef.current = layout.bands;
  const flowInstance = useRef<ReactFlowInstance | null>(null);

  useEffect(() => {
    const instance = flowInstance.current;
    if (!instance) return;
    if (focusName === null) {
      instance.fitView({ padding: 0.15, duration: 300 });
      return;
    }
    const band = bandsRef.current.get(focusName);
    if (!band) return;
    void instance.fitBounds(
      { x: band.x, y: band.y, width: band.width, height: band.height },
      { padding: 0.2, duration: 300 },
    );
  }, [focusName]);

  useEffect(() => {
    flowInstance.current?.fitView({ padding: 0.15 });
  }, [source]);

  /**
   * **LA PRIMA INQUADRATURA ASPETTA CHE CI SIA QUALCOSA DA INQUADRARE.**
   *
   * `fitView` come proprietà di `<ReactFlow>` scatta al primo disegno. I nodi
   * però non ci sono ancora: arrivano da un effetto su `canvas`, cioè **un
   * giro dopo**. Così la tela si inquadrava sul vuoto e non ci tornava più.
   *
   * Misurato il 01/09/2026 su un browser vero, appena aperta la vista dei
   * flussi: **otto nodi nel documento, zero dentro la vista**, a 375, 760,
   * 1100 e 1440 pixel. Chi apriva «FLUSSI» vedeva una tela vuota e i propri
   * nodi solo come macchie nella minimappa — e nessuna delle 149 prove di
   * questo albero diventava rossa, perché tutte guardano il documento e questa
   * è una proprietà della geometria. Adesso la guarda
   * `npm run check:canvas`.
   *
   * Si inquadra UNA volta sola: dopo, la vista è di chi la sta usando, e
   * ricentrarla mentre qualcuno trascina sarebbe peggio del difetto. Se c'è
   * già un fuoco non si fa niente: ci pensa `fitBounds` qui sopra, che sa
   * anche dove.
   */
  const framedOnce = useRef(false);
  useEffect(() => {
    if (framedOnce.current || focusName !== null) return;
    const instance = flowInstance.current;
    // Finché non c'è l'istanza o non c'è un nodo non si va da nessuna parte, e
    // l'attesa finisce da sé al disegno successivo.
    if (!instance || nodes.length === 0) return;
    framedOnce.current = true;
    instance.fitView({ padding: 0.15 });
  }, [nodes, focusName]);

  /**
   * **IL PASSO NUOVO NASCE NELLA SUA CORSIA, E LA VISTA CI VA.**
   *
   * Dove esattamente lo decide `buildUnifiedLayout`, e la risposta è «su una
   * riga nuova della corsia, in parallelo all'inizio»: un passo appena creato
   * non dipende da nessuno, e un passo senza dipendenze parte insieme agli
   * altri che non ne hanno. Non è in coda alla catena, ed è giusto così —
   * metterlo in coda vorrebbe dire inventargli una dipendenza che chi lo crea
   * non ha chiesto.
   *
   * Un passo non porta una posizione: il file di un flusso non ne ha una, e a
   * decidere dove sta ogni nodo è `buildUnifiedLayout` a ogni disegno. Quindi
   * «nasce sotto il puntatore» qui è **impossibile senza mentire**: si potrebbe
   * lasciarlo cadere dove si vuole e vederlo saltare altrove al primo ricalcolo.
   * Per lo stesso motivo la cassetta è una fila di attrezzi da premere e non da
   * trascinare — un trascinamento prometterebbe un punto d'arrivo che non
   * esiste.
   *
   * Resta il difetto vero, che è l'altra metà: il nodo compariva dove decideva
   * il programma, e chi guardava restava dov'era. Qui la vista lo raggiunge,
   * **senza cambiare la scala**: `maxZoom` è lo zoom corrente, quindi
   * l'inquadratura scorre e non si stringe. Chi stava guardando da lontano
   * continua a guardare da lontano.
   */
  const [addedNode, setAddedNode] = useState<string | null>(null);

  useEffect(() => {
    if (addedNode === null) return;
    const instance = flowInstance.current;
    // Il nodo entra nell'elenco un giro dopo che il flusso è cambiato: finché
    // non c'è non si va da nessuna parte, e l'attesa finisce da sé al disegno
    // successivo.
    if (!instance || !nodes.some((node) => node.id === addedNode)) return;
    void instance.fitView({
      nodes: [{ id: addedNode }],
      padding: 0.1,
      maxZoom: instance.getZoom(),
      duration: 320,
    });
    setAddedNode(null);
  }, [nodes, addedNode]);

  function updateFlow(name: string, updater: (flow: FlowFile) => FlowFile) {
    setFlows((prev) => {
      const current = prev.get(name);
      if (!current) return prev;
      const next = new Map(prev);
      next.set(name, { ...current, flow: updater(current.flow) });
      return next;
    });
  }

  /**
   * Un flusso nuovo nasce qui, vuoto, e resta nella finestra finché non lo si
   * salva: senza questo gesto una cartella vuota è un vicolo cieco, perché la
   * cassetta dei passi chiede un flusso a cui appartenere.
   */
  function addFlow() {
    const name = freeFlowName(flows);
    const flow: FlowFile = {
      id: name,
      description: "",
      graph: { steps: [], skippable_dependencies: [] },
      inputs: {},
    };
    setFlows((prev) => new Map(prev).set(name, { flow, saved: null }));
    setFocusName(name);
    setSelectedNode(null);
  }

  /**
   * Rinominare un flusso significa rinominare il suo file: finché non è mai
   * stato scritto è un gesto senza conseguenze, e dopo non lo è più — sul disco
   * resterebbe il vecchio file accanto al nuovo. Il pannello lo permette solo
   * prima del primo salvataggio, e lo dice.
   */
  function renameFlow(oldName: string, raw: string) {
    const name = raw.trim();
    if (name === "" || name === oldName) return;
    setFlows((prev) => {
      const current = prev.get(oldName);
      if (!current || current.saved !== null || prev.has(name)) return prev;
      const next = new Map<string, WorkingFlow>();
      for (const [key, value] of prev) {
        if (key === oldName) next.set(name, { ...value, flow: { ...value.flow, id: name } });
        else next.set(key, value);
      }
      return next;
    });
    setFocusName((current) => (current === oldName ? name : current));
    setSelectedNode((current) => {
      if (!current) return current;
      const { flowName, stepId } = splitNodeId(current);
      return flowName === oldName ? nodeId(name, stepId) : current;
    });
  }

  function addStep(flowName: string, kind: StepKind) {
    const action = DEFAULT_ACTION_FOR_KIND[kind];
    if (!action) return;
    const working = flows.get(flowName);
    if (!working) return;
    const ids = new Set(working.flow.graph.steps.map((step) => step.id));
    let n = 1;
    let id = `${kind}-${n}`;
    while (ids.has(id)) {
      n += 1;
      id = `${kind}-${n}`;
    }
    const step: Step = {
      id,
      deps: [],
      input_schema: { type: "any" },
      output_schema: { type: "any" },
      with: null,
      when: null,
      action,
      max_attempts: 1,
    };
    updateFlow(flowName, (flow) => ({
      ...flow,
      graph: { ...flow.graph, steps: [...flow.graph.steps, step] },
    }));
    setSelectedNode(nodeId(flowName, id));
    setAddedNode(nodeId(flowName, id));
  }

  function deleteStep(flowName: string, stepId: string) {
    updateFlow(flowName, (flow) => ({
      ...flow,
      graph: {
        ...flow.graph,
        steps: flow.graph.steps
          .filter((step) => step.id !== stepId)
          .map((step) =>
            step.deps.includes(stepId) ? { ...step, deps: step.deps.filter((dep) => dep !== stepId) } : step,
          ),
        skippable_dependencies: (flow.graph.skippable_dependencies ?? []).filter(
          (edge) => edge.step !== stepId && edge.dependency !== stepId,
        ),
      },
    }));
    setSelectedNode((current) => (current === nodeId(flowName, stepId) ? null : current));
  }

  function renameStep(flowName: string, oldId: string, newId: string) {
    updateFlow(flowName, (flow) => {
      if (flow.graph.steps.some((step) => step.id === newId)) return flow;
      return {
        ...flow,
        graph: {
          steps: flow.graph.steps.map((step) => {
            const deps = step.deps.includes(oldId) ? step.deps.map((dep) => (dep === oldId ? newId : dep)) : step.deps;
            if (step.id === oldId) return { ...step, id: newId, deps };
            return deps === step.deps ? step : { ...step, deps };
          }),
          skippable_dependencies: (flow.graph.skippable_dependencies ?? []).map((edge) => ({
            step: edge.step === oldId ? newId : edge.step,
            dependency: edge.dependency === oldId ? newId : edge.dependency,
          })),
        },
      };
    });
    setSelectedNode(nodeId(flowName, newId));
  }

  function updateStepField(flowName: string, stepId: string, patch: Partial<Step>) {
    updateFlow(flowName, (flow) => ({
      ...flow,
      graph: {
        ...flow.graph,
        steps: flow.graph.steps.map((step) => (step.id === stepId ? { ...step, ...patch } : step)),
      },
    }));
  }

  // Collegare e scollegare due passi: lo stesso gesto nasca da un trascino
  // sulla tela o da una casella nel pannello, passa sempre di qui.
  function connectSteps(flowName: string, from: string, to: string) {
    if (from === to) return;
    const working = flows.get(flowName);
    if (!working) return;
    if (wouldCycle(working.flow.graph, from, to)) return;
    const target = working.flow.graph.steps.find((step) => step.id === to);
    if (!target || target.deps.includes(from)) return;
    updateFlow(flowName, (flow) => ({
      ...flow,
      graph: {
        ...flow.graph,
        steps: flow.graph.steps.map((step) => (step.id === to ? { ...step, deps: [...step.deps, from] } : step)),
      },
    }));
  }

  function disconnectSteps(flowName: string, from: string, to: string) {
    updateFlow(flowName, (flow) => ({
      ...flow,
      graph: {
        ...flow.graph,
        steps: flow.graph.steps.map((step) =>
          step.id === to ? { ...step, deps: step.deps.filter((dep) => dep !== from) } : step,
        ),
      },
    }));
  }

  async function handleSave(name: string) {
    const working = flows.get(name);
    if (!working) return;
    setSaving((prev) => new Set(prev).add(name));
    setSaveErrors((prev) => {
      if (!(name in prev)) return prev;
      const next = { ...prev };
      delete next[name];
      return next;
    });
    try {
      await saveFlow(working.flow);
      setFlows((prev) => {
        const current = prev.get(name);
        if (!current) return prev;
        const next = new Map(prev);
        next.set(name, { flow: current.flow, saved: current.flow });
        return next;
      });
    } catch (error) {
      setSaveErrors((prev) => ({ ...prev, [name]: String(error) }));
    } finally {
      setSaving((prev) => {
        const next = new Set(prev);
        next.delete(name);
        return next;
      });
    }
  }

  async function handleDeleteFlow(name: string) {
    const working = flows.get(name);
    if (!working) return;
    // Un flusso mai scritto si scarta qui: chiedere al motore di cancellare un
    // file che non ha mai visto darebbe un errore al posto di un gesto riuscito.
    const neverSaved = working.saved === null;
    const question = neverSaved
      ? `Scartare il flusso "${name}"? Non è mai stato scritto sul disco.`
      : `Cancellare il flusso "${name}"? Non si torna indietro da qui.`;
    if (!window.confirm(question)) return;

    function forget() {
      setFlows((prev) => {
        const next = new Map(prev);
        next.delete(name);
        return next;
      });
      setFocusName((current) => (current === name ? null : current));
      setSelectedNode((current) => (current && splitNodeId(current).flowName === name ? null : current));
    }

    if (neverSaved) {
      forget();
      return;
    }

    setSaving((prev) => new Set(prev).add(name));
    try {
      await deleteFlow(name);
      forget();
    } catch (error) {
      setSaveErrors((prev) => ({ ...prev, [name]: String(error) }));
    } finally {
      setSaving((prev) => {
        const next = new Set(prev);
        next.delete(name);
        return next;
      });
    }
  }

  function onNodesChange(changes: NodeChange[]) {
    setNodes((current) => applyNodeChanges(changes, current));
  }

  function onEdgesChange(changes: EdgeChange[]) {
    setEdges((current) => applyEdgeChanges(changes, current));
  }

  function onConnect(connection: Connection) {
    if (!connection.source || !connection.target) return;
    const from = splitNodeId(connection.source);
    const to = splitNodeId(connection.target);
    if (from.flowName !== to.flowName) return;
    connectSteps(from.flowName, from.stepId, to.stepId);
  }

  function isValidConnection(connection: Connection | Edge): boolean {
    const { source, target } = connection;
    if (!source || !target) return false;
    const from = splitNodeId(source);
    const to = splitNodeId(target);
    return from.flowName === to.flowName && from.stepId !== to.stepId;
  }

  function onNodesDelete(deleted: Node[]) {
    for (const node of deleted) {
      if (node.type !== "step") continue;
      const { flowName, stepId } = splitNodeId(node.id);
      deleteStep(flowName, stepId);
    }
  }

  function onEdgesDelete(deleted: Edge[]) {
    for (const edge of deleted) {
      const from = splitNodeId(edge.source);
      const to = splitNodeId(edge.target);
      if (from.flowName !== to.flowName) continue;
      disconnectSteps(to.flowName, from.stepId, to.stepId);
    }
  }

  function onNodeClick(_event: unknown, node: Node) {
    if (node.type !== "step") return;
    setSelectedNode(node.id);
    setFocusName(splitNodeId(node.id).flowName);
  }

  const selected = selectedNode ? nodes.find((node) => node.id === selectedNode) : undefined;
  const selectedData = selected?.data as StepNodeData | undefined;
  const selectedFlow = selectedData ? flows.get(selectedData.flowName) : undefined;

  const focusedBand = focusName ? layout.bands.get(focusName) : undefined;
  const focusedWorking = focusName ? flows.get(focusName) : undefined;
  const focusedDirty = focusedWorking ? isDirty(focusedWorking) : false;

  /**
   * WHAT THE RIGHT-HAND SIDE OF THE BAR IS ALLOWED TO SAY.
   *
   * The mockup writes «one run in progress · step 7 of 9» on the graph, and the
   * verdict of a check on the code. Only the first of the two exists: the count
   * is folded here from the run's own events and the number of steps the flow
   * declares. The second is `sailor flow check`, which lives in the command line
   * and has no door into this window — so the code tab says what the engine
   * really did (it refuses a graph it cannot load, on load and on save) instead
   * of borrowing a verdict nobody gave.
   */
  const focusedRun = focusName ? latestByFlow.get(focusName) : undefined;
  const barStatus = useMemo<BarStatus | null>(() => {
    if (focusName === null || focusedWorking === undefined) return null;
    const total = focusedWorking.flow.graph.steps.length;


    if (focusedRun === undefined) return { live: false, word: "no run of this flow yet" };
    const { done, running } = runProgress(focusedRun);
    if (focusedRun.status === "running") {
      const at = Math.min(total, done + (running > 0 ? 1 : 0));
      return { live: true, word: `a run in progress · step ${at} of ${total}` };
    }
    return { live: false, word: `last run ${focusedRun.status} · ${done} of ${total} steps closed` };
  }, [focusName, focusedWorking, focusedDirty, focusedRun, flowView]);

  return (
    <div className="app">
      {/* THE BAR NAMES THE FLOW IT IS ABOUT.
          Until now it said «6 flows, one system» — true, and about nothing you
          could act on: neither which flow was on screen, nor whether it was
          saved, nor whether it was running, nor a way to run it. Save and Run
          need a subject, and the subject is the flow the rail has in focus. */}
      <TopBar
        view={flowView}
        onView={() => setPlace("flows")}
        flowName={focusName}
        steps={focusedWorking ? focusedWorking.flow.graph.steps.length : 0}
        dirty={focusedDirty}
        busy={focusName !== null && saving.has(focusName)}
        starting={focusName !== null && starting.has(focusName)}
        source={source}
        sourceWord={
          source === "engine"
            ? `${flows.size + broken.length} flows from the disk`
            : source === "failed"
              ? `the engine is not answering: ${failure}`
              : source === "loading"
                ? "asking the engine for the flows…"
                : "sample data"
        }
        status={barStatus}
        onWatch={focusedRun ? () => setWatching(focusedRun.run_id) : undefined}
        onSave={() => {
          if (focusName) void handleSave(focusName);
        }}
        onRun={() => {
          if (focusName) void handleRun(focusName);
        }}
      />

      {/* I POSTI, NOMINATI E DIVISI IN DUE INTENZIONI. Una finestra che cambia
          contenuto senza dire dove si è costringe a ricostruirlo dall'aspetto
          della pagina.

          La divisione — guardare a sinistra, amministrare a destra — è il
          criterio che Inngest ha adottato riprogettando la propria
          navigazione, e lo misura in clic: «i run falliti a un clic dalla barra
          laterale» invece che scavando dentro la pagina delle funzioni. È
          l'unico documento della ricognizione che nomina un criterio invece di
          descrivere una disposizione.

          Quelli che mancano — spazi, profili, server MCP — si aggiungono
          quando esiste il motore che li risponde, non prima: una voce che apre
          una schermata vuota è una promessa non mantenuta a ogni clic.

          «Terminali» stava in quell'elenco fino al 01/09/2026 e adesso è una
          voce, e la differenza non è che il motore risponda — il ponte nasce in
          un altro cantiere mentre questa riga si scrive. È che quella schermata
          **non si apre vuota**: senza motore scrive quale domanda non ha potuto
          fare, ed è la distinzione fra «non c'è niente» e «non posso vedere» che
          la promessa non mantenuta faceva sparire. */}
      <nav className="places">
        <button
          type="button"
          className="places__item"
          data-here={place === "now" || undefined}
          onClick={() => setPlace("now")}
        >
          Adesso
        </button>
        <button
          type="button"
          className="places__item"
          data-here={place === "terminals" || undefined}
          onClick={() => setPlace("terminals")}
        >
          Terminali
        </button>
        <button
          type="button"
          className="places__item"
          data-here={place === "history" || undefined}
          onClick={() => setPlace("history")}
        >
          Storia
        </button>
        <span className="places__gap" />
        <button
          type="button"
          className="places__item"
          data-here={place === "flows" || undefined}
          onClick={() => setPlace("flows")}
        >
          Flussi
          <span className="places__count">{flows.size}</span>
        </button>
        <button
          type="button"
          className="places__item"
          data-here={place === "installed" || undefined}
          onClick={() => setPlace("installed")}
        >
          Installato
        </button>
        <button
          type="button"
          className="places__item"
          data-here={place === "manual" || undefined}
          onClick={() => setPlace("manual")}
        >
          Comandi
        </button>
      </nav>

      {place === "flows" && focusName && focusedWorking && focusedBand && (
        <FocusBar
          key={focusName}
          name={focusName}
          color={focusedBand.color}
          flow={focusedWorking.flow}
          neverSaved={focusedWorking.saved === null}
          busy={saving.has(focusName)}
          error={saveErrors[focusName]}
          onRename={(next) => renameFlow(focusName, next)}
          onDescription={(text) => updateFlow(focusName, (flow) => ({ ...flow, description: text }))}
          onDelete={() => void handleDeleteFlow(focusName)}
        />
      )}

      <RunContext.Provider value={controls}>
      <StepRunContext.Provider value={stepStates}>
      <StepUsageContext.Provider value={stepUsage}>
      {place === "now" && (
        <Now
          native={NATIVE}
          onOpen={(runId) => {
            setWatching(runId);
          }}
        />
      )}
      {place === "history" && <History native={NATIVE} />}
      {place === "installed" && <Installed native={NATIVE} />}
      {place === "manual" && <Manual native={NATIVE} />}
      {place === "terminals" && <Terminals native={NATIVE} />}
      {/* THE CANVAS STAYS MOUNTED BEHIND THE OTHER TWO TABS. React Flow measures
          its own frame once: unmounting it to change tab would give back a
          canvas that has to find its nodes again every time. */}
      <div className="body" hidden={place !== "flows" || flowView !== "graph"}>

        {/* LA COLONNA HA UN MESTIERE SOLO: SCEGLIERE COSA GUARDARE. La cassetta
            dei passi se n'è andata dentro la tela, dove si compone. Quello che
            resta — l'elenco dei flussi, «tutti i flussi», il flusso nuovo —
            risponde a una domanda sola, e la larghezza resta quella: a fissarla
            era il nome di un flusso accanto al suo conteggio di passi, e
            stringere la colonna li troncherebbe senza guadagnare niente sulla
            tela, che il pannello a destra delimita comunque. */}
        <aside className="rail">
          <div className="rail__title">Flussi registrati</div>
          <button
            type="button"
            className="rail__all"
            data-active={focusName === null || undefined}
            onClick={() => setFocusName(null)}
          >
            Tutti i flussi
          </button>
          {flowList.map(({ name, flow }) => {
            const working = flows.get(name);
            const dirty = working ? isDirty(working) : false;
            const color = layout.bands.get(name)?.color;
            return (
              <button
                type="button"
                key={name}
                className="rail__item"
                data-open={name === focusName || undefined}
                onClick={() => {
                  setFocusName((current) => (current === name ? null : name));
                  setSelectedNode(null);
                }}
              >
                <span className="rail__dot" style={{ background: color }} />
                <span className="rail__label">
                  {name}
                  {dirty && <span className="rail__dirty-dot" title="non salvato" />}
                </span>
                <span className="rail__note">{flow.graph.steps.length} passi</span>
              </button>
            );
          })}
          {broken.map((entry) => (
            // Un flusso rotto non sparisce dall'elenco: si vede, marcato, col
            // motivo. Non entra nella tela perché non ha un grafo da disegnare.
            <div className="rail__item" key={entry.name} data-broken>
              <span className="rail__label">{entry.name}</span>
              <span className="rail__note">{entry.reason}</span>
            </div>
          ))}
          <button type="button" className="rail__new" onClick={addFlow}>
            + Nuovo flusso
          </button>

        </aside>

        <main className="canvas">
          <ReactFlow
            nodes={nodes}
            edges={edges}
            nodeTypes={nodeTypes}
            onNodesChange={onNodesChange}
            onEdgesChange={onEdgesChange}
            onConnect={onConnect}
            isValidConnection={isValidConnection}
            onNodesDelete={onNodesDelete}
            onEdgesDelete={onEdgesDelete}
            deleteKeyCode={["Backspace", "Delete"]}
            onNodeClick={onNodeClick}
            onPaneClick={() => setSelectedNode(null)}
            onInit={(instance) => {
              flowInstance.current = instance;
            }}
            fitView
            proOptions={{ hideAttribution: false }}
          >
            <Background gap={20} />
            <Controls />
            {/* LA MINIMAPPA DICE DOVE GUARDARE, non «c'è della roba». Era un
                blocco grigio uniforme: adesso ogni passo ci sta con la tinta
                del proprio stato, così un guasto in fondo a un flusso fuori
                schermo si vede senza scorrere. */}
            <MiniMap
              pannable
              zoomable
              nodeColor={(node) => {
                if (node.type !== "step") return "#d8d7ce";
                const data = node.data as StepNodeData;
                const state = stepStates.get(nodeId(data.flowName, data.step.id))?.state ?? data.run?.state;
                return STATE_COLOR[state ?? "waiting"];
              }}
            />

            {/* LA CASSETTA STA QUI DENTRO, ed è tutto il punto del lavoro: chi
                compone non esce più dalla tela per prendere un attrezzo.
                Sta dentro `ReactFlow` e non accanto perché `Panel` la disegna
                fuori dal riquadro che pan e zoom trasformano — dentro la tela,
                ferma rispetto ad essa.

                CON ZERO FLUSSI NON C'È: quel momento è della tela vuota, che
                insegna il primo gesto. Due inviti nello stesso schermo si
                annullano — è la stessa regola per cui una vista ha un solo
                elemento isolato. */}
            {flows.size > 0 && (
              <Toolbar
                flowName={focusName}
                onAdd={(kind) => focusName && addStep(focusName, kind)}
                onNewFlow={addFlow}
              />
            )}
          </ReactFlow>

          {/* Una tela senza flussi non resta muta: dice cos'è un flusso e offre
              il gesto per farne uno. Il messaggio sparisce da solo appena ce n'è
              uno, e non intercetta il puntatore sulla tela sotto. */}
          {flows.size === 0 && (
            <BlankCanvas
              state={source === "failed" ? "failed" : source === "loading" ? "loading" : "empty"}
              failure={failure}
              brokenCount={broken.length}
              onCreate={addFlow}
            />
          )}
        </main>

        <aside className="panel">
          <datalist id="known-actions">
            {KNOWN_ACTIONS.map((action) => (
              <option key={action} value={action} />
            ))}
          </datalist>

          {selectedData && selectedFlow ? (
            <>
            {/* The step as the run has it, above the step as the file declares
                it: what is happening explains what is written, not the other
                way round. It mounts only while a run holds this step. */}
            {watched && (
              <StepLive
                key={`${selectedData.flowName}::${selectedData.step.id}::live`}
                step={selectedData.step}
                graph={selectedFlow.flow.graph}
                run={watched}
                now={now}
              />
            )}
            <StepEditor
              key={selectedNode}
              flowName={selectedData.flowName}
              color={selectedData.color}
              step={selectedData.step}
              siblingIds={selectedFlow.flow.graph.steps.map((step) => step.id).filter((id) => id !== selectedData.step.id)}
              tools={tools}
              discovery={discovery}
              usedModels={usedModels}
              onRename={(newId) => renameStep(selectedData.flowName, selectedData.step.id, newId)}
              onField={(patch) => updateStepField(selectedData.flowName, selectedData.step.id, patch)}
              onToggleDep={(depId, on) =>
                on
                  ? connectSteps(selectedData.flowName, depId, selectedData.step.id)
                  : disconnectSteps(selectedData.flowName, depId, selectedData.step.id)
              }
              onDelete={() => deleteStep(selectedData.flowName, selectedData.step.id)}
            />
            {/* Cosa è passato di qui, nel tempo. Sta sotto i parametri e non in
                un pannello a parte: chi clicca un nodo chiede tutte e due le
                cose — com'è fatto, e cosa ci è entrato. Fuori dal guscio non
                c'è deposito, e il pannello lo dice da sé. */}
            {NATIVE && (
              <StepHistory
                key={`${selectedData.flowName}::${selectedData.step.id}`}
                flowName={selectedData.flowName}
                stepId={selectedData.step.id}
              />
            )}
            </>
          ) : (
            <div className="panel__empty">
              {flows.size === 0
                ? "Nessun flusso da mostrare: creane uno dalla tela o dalla colonna a sinistra."
                : "Scegli un passo per vederne e modificarne i parametri, o un flusso a sinistra per metterlo a fuoco."}
            </div>
          )}
        </aside>
      </div>

      {/* LA VISTA SI CHIUDE, LA CORSA NO. Chiudere questo pannello non ferma
          niente: il flusso gira nel guscio, e riaprendo si ritrova tutto quello
          che ha detto mentre nessuno guardava. */}
      {watched && (
        <RunConsole
          run={watched}
          runs={executionList}
          mode={consoleMode}
          now={now}
          listenFailure={listenFailure}
          usage={usage}
          onMode={setConsoleMode}
          onPick={setWatching}
          onClose={() => setWatching(null)}
        />
      )}
      </StepUsageContext.Provider>
      </StepRunContext.Provider>
      </RunContext.Provider>
    </div>
  );
}

/** Un elenco vuoto stabile: un `[]` nuovo a ogni render rifarebbe i calcoli a valle. */
const EMPTY_TOOLS: Tool[] = [];

interface FocusBarProps {
  name: string;
  color: string;
  flow: FlowFile;
  neverSaved: boolean;
  busy: boolean;
  error?: string;
  onRename: (next: string) => void;
  onDescription: (text: string) => void;
  onDelete: () => void;
}

/**
 * The strip of the flow in focus: name, description and delete. Save left this
 * strip when the top bar took it: two Save buttons a hand apart, on the same
 * flow, is a question about which one saves what.
 *
 * Il nome si
 * scrive solo prima del primo salvataggio (è il nome del file); la descrizione
 * si scrive sempre, perché un flusso senza descrizione è un nome e basta per
 * chiunque lo apra dopo.
 *
 * Le bozze restano qui dentro e si depositano al `blur`: rinominare a ogni
 * tasto premuto significherebbe rinominare il flusso sette volte mentre lo si
 * chiama «notte».
 */
function FocusBar({
  name,
  color,
  flow,
  neverSaved,
  busy,
  error,
  onRename,
  onDescription,
  onDelete,
}: FocusBarProps) {
  const [nameDraft, setNameDraft] = useState(name);
  const [descDraft, setDescDraft] = useState(flow.description);

  return (
    <div className="focusbar">
      <span className="focusbar__dot" style={{ background: color }} />
      {neverSaved ? (
        <input
          className="focusbar__name-input"
          value={nameDraft}
          aria-label="nome del flusso"
          onChange={(event) => setNameDraft(event.target.value)}
          onBlur={() => onRename(nameDraft)}
          onKeyDown={(event) => {
            if (event.key === "Enter") event.currentTarget.blur();
          }}
        />
      ) : (
        <span className="focusbar__name" title="il nome è quello del file: si sceglie prima di salvare">
          {name}
        </span>
      )}
      <input
        className="focusbar__desc-input"
        value={descDraft}
        aria-label="descrizione del flusso"
        placeholder="a cosa serve questo flusso"
        onChange={(event) => setDescDraft(event.target.value)}
        onBlur={() => onDescription(descDraft)}
        onKeyDown={(event) => {
          if (event.key === "Enter") event.currentTarget.blur();
        }}
      />
      <div className="focusbar__spacer" />
      {error && <span className="focusbar__error">{error}</span>}
      <button type="button" className="is-danger" onClick={onDelete} disabled={busy}>
        {neverSaved ? "Scarta flusso" : "Elimina flusso"}
      </button>
    </div>
  );
}


// ── the top bar, and the three views of one flow ─────────────────────────

/** What the right-hand side of the bar says, and whether something is alive. */
interface BarStatus {
  live: boolean;
  word: string;
}

/**
 * How far a run has got, folded from its own facts.
 *
 * THE SNAPSHOT CARRIES NO COUNTERS. `RunSnapshot` has `status` and a list of
 * events and nothing else; the ledger has `steps_went`/`steps_total`, but it
 * only counts steps it has already recorded, so mid-run it undercounts, and it
 * is read on demand rather than at every beat. The denominator therefore comes
 * from the flow on screen, and the numerator from the events — the same fold
 * the canvas already uses to colour its nodes.
 */
function runProgress(run: RunSnapshot): { done: number; running: number } {
  let done = 0;
  let running = 0;
  for (const step of stepStatesOfRun(run.events).values()) {
    if (step.state === "running") running += 1;
    else if (step.state !== "waiting") done += 1;
  }
  return { done, running };
}

const VIEW_WORD: Record<FlowView, string> = { graph: "Graph" };

interface TopBarProps {
  view: FlowView;
  onView: (view: FlowView) => void;
  flowName: string | null;
  steps: number;
  dirty: boolean;
  busy: boolean;
  starting: boolean;
  source: Source;
  sourceWord: string;
  status: BarStatus | null;
  onWatch?: () => void;
  onSave: () => void;
  onRun: () => void;
}

/**
 * The bar of the program: the mark, the flow in focus, the three views of it,
 * and the two gestures that act on it.
 *
 * NO VERSION SITS NEXT TO THE NAME. The mockup draws a `v7` chip there; a flow
 * has no version — not in `flow::FlowFile`, not in the `.flow.json` on disk,
 * not in this window — and the Rust type refuses unknown fields, so one cannot
 * be added without changing the engine. The chip counts steps instead, which is
 * a number the flow really carries.
 */
function TopBar({
  view,
  onView,
  flowName,
  steps,
  dirty,
  busy,
  starting,
  source,
  sourceWord,
  status,
  onWatch,
  onSave,
  onRun,
}: TopBarProps) {
  const statusBody = status && (
    <>
      <span className="topbar__live" data-idle={status.live ? undefined : true} />
      <span className="topbar__status-word">{status.word}</span>
    </>
  );

  return (
    <header className="topbar">
      <span className="topbar__brand">
        <svg
          className="topbar__mark"
          width="18"
          height="18"
          viewBox="0 0 24 24"
          fill="none"
          stroke="currentColor"
          strokeWidth="1.8"
          strokeLinecap="round"
          strokeLinejoin="round"
          aria-hidden="true"
        >
          <path d="M3 17l9-13 9 13" />
          <path d="M3 17c2.5 2 5 2 7.5 0S16 15 18.5 17" />
        </svg>
        Sailor
      </span>
      <span className="topbar__rule" />

      {flowName === null ? (
        <span className="topbar__none">no flow in focus — pick one in the rail</span>
      ) : (
        <span className="topbar__flow">
          <span className="topbar__flow-name">{flowName}</span>
          <span className="topbar__steps">{steps} steps</span>
          {dirty && (
            <span className="topbar__dirty">
              <span className="topbar__dot" />
              unsaved changes
            </span>
          )}
        </span>
      )}

      {/* THREE VIEWS OF THE SAME FLOW, not three places of the window. */}
      <nav className="topbar__tabs" aria-label="views of the flow in focus">
        {(["graph"] as FlowView[]).map((name) => (
          <button
            key={name}
            type="button"
            className="topbar__tab"
            data-here={view === name || undefined}
            aria-pressed={view === name}
            onClick={() => onView(name)}
          >
            {VIEW_WORD[name]}
          </button>
        ))}
      </nav>

      <span className="topbar__spacer" />

      {/* Whoever is looking must know whether these flows come from the disk or
          from a sample, without asking and without opening the code. */}
      <span className="topbar__source" data-source={source}>
        {sourceWord}
      </span>

      {status !== null &&
        (onWatch ? (
          <button type="button" className="topbar__status" onClick={onWatch}>
            {statusBody}
          </button>
        ) : (
          <span className="topbar__status">{statusBody}</span>
        ))}

      <button
        type="button"
        className="topbar__save"
        onClick={onSave}
        disabled={flowName === null || !dirty || busy}
      >
        {busy ? "Saving…" : "Save"}
      </button>
      {/* A FILLED BUTTON HAS NO TINT. Prohibition 4 reserves colour for the
          state of the machine, so «Run» is a plate — ink on ground — and not a
          green. `.is-primary` already carried that answer. */}
      <button
        type="button"
        className="topbar__run is-primary"
        onClick={onRun}
        disabled={flowName === null || starting}
      >
        <span className="topbar__glyph" aria-hidden="true">
          ▶
        </span>
        {starting ? "Starting…" : "Run"}
      </button>
    </header>
  );
}

/**
 * THE CODE TAB SHOWS WHAT IS ACTUALLY WRITTEN, which is JSON.
 *
 * The mockup draws TypeScript — `createStep`, `createFlow`, imports from
 * `@sailor/core`. Nothing of the sort exists: there is no package, no generator,
 * no engine command that emits source, and the project decided the opposite on
 * purpose — a flow is a data file and a step is an action registered in Rust,
 * with no interpreter inside Sailor. So this tab does not draw an imaginary
 * language: it shows the file the engine reads, and says why it is that file.
 */
/* Kept, unmounted: what it measured is worth more than the view — there is no
   Tauri command for `sailor flow check`, so the window only knows the graph
   loaded. See the top bar's status line. */
function FlowCode({ name, flow }: { name: string | null; flow: FlowFile | undefined }) {
  if (name === null || flow === undefined) {
    return (
      <div className="codeview">
        <p className="codeview__empty">Pick a flow in the rail to read what is written for it.</p>
      </div>
    );
  }

  return (
    <div className="codeview">
      <div className="label">{name}.flow.json</div>
      <p className="codeview__note">
        This is the flow as it is stored, and it is the whole of it: a flow is a data file, and every
        step names an action the engine has registered — never code that runs from here. The mockup
        for this tab drew generated TypeScript; no such generator exists, and none is planned, so
        what you read below is the file and not a rendering of it.
      </p>
      <pre className="codeview__source">{JSON.stringify(flow, null, 2)}</pre>
    </div>
  );
}

/**
 * THE RUNS TAB IS THE LEDGER, NARROWED BY HAND.
 *
 * The engine keeps every run in a ledger and hands the window the lot: there is
 * no per-flow query behind `execution_history`, though the SQL for one already
 * exists unexposed in `crates/ledger`. Filtering here is what the command line
 * does too — but it means the window carries the whole history to show a slice
 * of it, and that is worth knowing before this list gets long.
 */
/* Kept, unmounted: the ledger has flow-scoped queries (`runs_in_window`,
   `last_finished_run`) that no Tauri command exposes, so this filtered in the
   window. The places «Adesso» and «Cronologia» serve the need. */
function FlowRuns({ name }: { name: string | null }) {
  const now = useClock();
  const { asked } = useAsk<Execution[]>(
    NATIVE,
    executionHistory,
    15000,
    "outside the shell: the ledger is the engine's to read",
  );

  if (name === null) {
    return (
      <div className="now">
        <p className="now__empty">Pick a flow in the rail to see the runs it has had.</p>
      </div>
    );
  }
  if (asked.state === "mute") {
    return (
      <div className="now">
        <p className="now__mute">I cannot read the runs of «{name}»: {asked.why}</p>
      </div>
    );
  }
  if (asked.state === "asking") {
    return (
      <div className="now">
        <p className="now__mute">Reading the ledger…</p>
      </div>
    );
  }

  const mine = asked.value.filter((run) => run.entity === name);
  if (mine.length === 0) {
    return (
      <div className="now">
        <p className="now__empty">
          The ledger remembers no run of «{name}». It remembers {asked.value.length} of other flows.
        </p>
      </div>
    );
  }

  return (
    <div className="now">
      <header className="now__head">
        <h2 className="now__title">Runs of {name}</h2>
        <span className="now__count">{mine.length}</span>
        <span className="now__note">out of {asked.value.length} the ledger remembers</span>
      </header>
      <table className="now__table">
        <thead>
          <tr>
            <th>run</th>
            <th>how it ended</th>
            <th>when</th>
            <th className="now__num">lasted</th>
            <th className="now__num">steps</th>
            <th className="now__num">retried</th>
          </tr>
        </thead>
        <tbody>
          {mine.map((run) => (
            <tr key={run.run_id}>
              <td className="now__entity">
                {run.run_id}
                {run.error !== null && <span className="now__why">{run.error}</span>}
              </td>
              <td className="now__state" data-outcome={outcomeOf(run)}>
                {outcomeOf(run)}
              </td>
              <td className="now__when">{whenOf(run.started_at, now)}</td>
              <td className="now__num">{lastedOf(run.duration_secs)}</td>
              <td className="now__num">
                {run.steps_went}/{run.steps_total}
              </td>
              <td className="now__num">{run.steps_retried === 0 ? "—" : run.steps_retried}</td>
            </tr>
          ))}
        </tbody>
      </table>
    </div>
  );
}

/* Not mounted today, and not dead: see the notes above each one. */
void FlowCode;
void FlowRuns;
