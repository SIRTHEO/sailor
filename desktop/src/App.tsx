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
  KIND_LABEL,
  StepNode,
  STATE_COLOR,
  StepRunContext,
  StepUsageContext,
  type StepNodeData,
} from "./StepNode";
import { stepUsageOfRun, type StepUsage } from "./stepusage";
import { stepStatesOfCanvas } from "./runstate";
import { BlankCanvas } from "./BlankCanvas";
import { Now } from "./Now";
import { StepEditor } from "./StepEditor";
import { RunContext, TriggerNode, triggerNodeId, type RunControls, type TriggerState } from "./TriggerNode";
import { RunConsole, type ConsoleMode } from "./RunConsole";
import { StepHistory } from "./StepHistory";
import { buildUnifiedLayout, nodeId, splitNodeId, wouldCycle } from "./layout";
import { SAMPLE, SAMPLE_RUN } from "./sample";
import {
  deleteFlow,
  discoverTools,
  flowTrigger,
  insideTheWindow,
  knownRuns,
  listenToRuns,
  loadFlows,
  runSnapshot,
  runUsage,
  saveFlow,
  startRun,
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
const PALETTE_KINDS = Object.keys(DEFAULT_ACTION_FOR_KIND) as StepKind[];

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
type Place = "now" | "flows";

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

  return (
    <div className="app">
      <header className="bar">
        <span className="bar__name">Sailor</span>
        <span className="bar__sep" />
        <span className="bar__flow">
          {flows.size === 0 ? "nessun flusso, ancora" : `${flows.size} flussi, un sistema solo`}
        </span>
        {/* Chi guarda deve sapere se sta vedendo il disco o un esempio, senza
            chiedere e senza aprire il codice. */}
        <span className="bar__source" data-source={source}>
          {source === "engine"
            ? `${flows.size + broken.length} flussi dal disco`
            : source === "failed"
              ? `il motore non risponde: ${failure}`
              : source === "loading"
                ? "chiedo i flussi al motore…"
                : "dati di esempio"}
        </span>
        {anyDirty && <span className="bar__dirty">modifiche non salvate</span>}
        <div className="bar__spacer" />
        {/* IL GESTO DI PARTENZA STA SUL NODO DI INNESCO, non qui: su una tela
            dove tutti i flussi stanno insieme, un pulsante «Esegui» in cima non
            dice quale flusso farebbe partire. Questo apre la vista di quello
            che sta girando, che è la domanda che si fa dalla barra. */}
        {executionList.length > 0 && (
          <button
            type="button"
            className="is-primary"
            onClick={() => setWatching(watched?.run_id ?? executionList[0].run_id)}
            disabled={watched !== undefined}
          >
            {anyRunning ? "Guarda l'esecuzione ●" : "Vedi l'ultima esecuzione"}
          </button>
        )}
      </header>

      {/* I POSTI, NOMINATI. Una finestra che cambia contenuto senza dire dove
          si è costringe a ricostruirlo dall'aspetto della pagina. Due posti
          adesso, e nessuno finto: quelli che mancano — spazi, profili,
          strumenti, terminali — si aggiungono quando esiste il motore che li
          risponde, non prima. */}
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
          data-here={place === "flows" || undefined}
          onClick={() => setPlace("flows")}
        >
          Flussi
          <span className="places__count">{flows.size}</span>
        </button>
      </nav>

      {place === "flows" && focusName && focusedWorking && focusedBand && (
        <FocusBar
          key={focusName}
          name={focusName}
          color={focusedBand.color}
          flow={focusedWorking.flow}
          neverSaved={focusedWorking.saved === null}
          dirty={focusedDirty}
          busy={saving.has(focusName)}
          error={saveErrors[focusName]}
          onRename={(next) => renameFlow(focusName, next)}
          onDescription={(text) => updateFlow(focusName, (flow) => ({ ...flow, description: text }))}
          onSave={() => void handleSave(focusName)}
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
      <div className="body" hidden={place !== "flows"}>
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

          <div className="rail__palette">
            <div className="rail__title">Cassetta dei passi</div>
            {focusName === null && <p className="rail__hint">scegli un flusso per aggiungere un passo</p>}
            <div className="palette__grid">
              {PALETTE_KINDS.map((kind) => (
                <button
                  key={kind}
                  type="button"
                  className="palette__item"
                  disabled={focusName === null}
                  title={focusName ? `aggiungi un passo di tipo «${KIND_LABEL[kind]}»` : "scegli prima un flusso"}
                  onClick={() => focusName && addStep(focusName, kind)}
                >
                  {KIND_LABEL[kind]}
                </button>
              ))}
            </div>
          </div>
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
  dirty: boolean;
  busy: boolean;
  error?: string;
  onRename: (next: string) => void;
  onDescription: (text: string) => void;
  onSave: () => void;
  onDelete: () => void;
}

/**
 * La barra del flusso a fuoco: nome, descrizione, salva ed elimina. Il nome si
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
  dirty,
  busy,
  error,
  onRename,
  onDescription,
  onSave,
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
      {dirty && <span className="focusbar__dirty">{neverSaved ? "mai salvato" : "non salvato"}</span>}
      <div className="focusbar__spacer" />
      {error && <span className="focusbar__error">{error}</span>}
      <button type="button" onClick={onSave} disabled={!dirty || busy}>
        {busy ? "salvo…" : "Salva flusso"}
      </button>
      <button type="button" className="is-danger" onClick={onDelete} disabled={busy}>
        {neverSaved ? "Scarta flusso" : "Elimina flusso"}
      </button>
    </div>
  );
}

