// Il pannello di un passo scelto: come si chiama, cosa fa, CHI LO ESEGUE,
// quante volte ci riprova, da chi dipende, con quali parametri.
//
// IL SELETTORE DELLO STRUMENTO NON CONOSCE NESSUNO STRUMENTO. L'elenco arriva
// dal motore (`discover_tools`); qui si sa solo disegnarlo. Quando la scoperta
// non risponde — perché il comando non c'è ancora, o perché la finestra gira
// fuori dal guscio — il pannello lo dice e lascia scrivere l'identificativo a
// mano: un nodo si compone comunque, e nessuno resta davanti a un elenco vuoto
// senza sapere se è la macchina a essere spoglia o il motore a tacere.

import { useRef, useState } from "react";
import { kindOf, type Step } from "./flow";
import {
  chainIn,
  groupByKind,
  joinToolParams,
  modelSuggestions,
  optionsPreview,
  rivalBinary,
  schemaRejectsToolKeys,
  splitToolParams,
  toolKindLabel,
  type OptionSpec,
  type OptionValue,
  type Tool,
  type ToolChoice,
  type ToolDiscovery,
} from "./tools";
import { ToolMark } from "./ToolMark";

export interface StepEditorProps {
  flowName: string;
  color: string;
  step: Step;
  siblingIds: string[];
  tools: Tool[];
  discovery: ToolDiscovery;
  /** I modelli già scritti negli altri passi: suggerimenti presi dal vero. */
  usedModels: string[];
  onRename: (newId: string) => void;
  onField: (patch: Partial<Step>) => void;
  onToggleDep: (depId: string, on: boolean) => void;
  onDelete: () => void;
}

/**
 * Monta una volta per passo selezionato (la chiave è `selectedNode` in `App`),
 * così le bozze locali (id, JSON) ripartono pulite a ogni cambio di selezione
 * senza un effetto dedicato a resettarle.
 */
export function StepEditor({
  flowName,
  color,
  step,
  siblingIds,
  tools,
  discovery,
  usedModels,
  onRename,
  onField,
  onToggleDep,
  onDelete,
}: StepEditorProps) {
  const [idDraft, setIdDraft] = useState(step.id);
  const idTaken = idDraft !== step.id && siblingIds.includes(idDraft);

  const { choice, rest } = splitToolParams(step.with);
  // La catena che il pannello non sa comporre e non deve cancellare: resta fra
  // gli altri parametri, e da qui si legge per dirlo a chi guarda.
  const chain = chainIn(rest);
  // Un passo «agente» usa uno strumento per definizione; e un passo che ne
  // dichiara già uno lo mostra comunque, qualunque azione porti — l'alternativa
  // sarebbe nascondere un dato che sta nel file. Una catena conta: un passo che
  // nomina tre motori ne usa uno.
  const usesTool = kindOf(step.action) === "engine" || choice.tool !== "" || chain.length > 0;

  // Il riquadro JSON mostra solo i parametri che il pannello non gestisce: i
  // tre campi hanno già i loro, e vederli in due posti che si scrivono a
  // vicenda è il modo più rapido per perdere quello scritto per ultimo.
  const [withDraft, setWithDraft] = useState(() => {
    const shown = usesTool ? rest : (step.with ?? {});
    return Object.keys(shown).length === 0 ? "" : JSON.stringify(shown, null, 2);
  });
  const [withError, setWithError] = useState<string | null>(null);

  const chosen = tools.find((tool) => tool.id === choice.tool);
  const unknownChoice = choice.tool !== "" && chosen === undefined;
  const models = modelSuggestions(chosen, usedModels);
  const rival = rivalBinary(rest);
  const rejectedBySchema = schemaRejectsToolKeys(step.input_schema, choice);

  function commitId() {
    const trimmed = idDraft.trim();
    if (!trimmed || trimmed === step.id || siblingIds.includes(trimmed)) {
      setIdDraft(step.id);
      return;
    }
    onRename(trimmed);
  }

  function setChoice(patch: Partial<ToolChoice>) {
    onField({ with: joinToolParams(rest, { ...choice, ...patch }) });
  }

  function commitWith() {
    const text = withDraft.trim();
    if (text === "") {
      setWithError(null);
      onField({ with: usesTool ? joinToolParams({}, choice) : null });
      return;
    }
    try {
      const parsed = JSON.parse(text) as Record<string, unknown>;
      setWithError(null);
      onField({ with: usesTool ? joinToolParams(parsed, choice) : parsed });
    } catch (error) {
      setWithError(String(error));
    }
  }

  return (
    <>
      <div className="panel__flow" style={{ color }}>
        {flowName}
      </div>
      <div className="panel__title">Passo</div>
      <input
        className="panel__id-input"
        value={idDraft}
        onChange={(event) => setIdDraft(event.target.value)}
        onBlur={commitId}
      />
      {idTaken && <p className="panel__error">un altro passo del flusso si chiama già così</p>}

      <label className="panel__field">
        <span>Azione</span>
        <input list="known-actions" value={step.action} onChange={(event) => onField({ action: event.target.value })} />
      </label>

      {usesTool && (
        <div className="panel__tool">
          <div className="panel__title">Chi lo esegue</div>

          {/* UNA CATENA SI DICE ANCHE QUI. Il selettore su «— nessuno —» sopra
              un passo che nomina tre motori è la stessa bugia che il nodo
              raccontava, spostata di una finestra: il campo c'è, il pannello
              non lo sa comporre, e il silenzio farebbe credere che non ci sia.
              Resta scritto fra i parametri, e ci torna identico. */}
          {chain.length > 0 && (
            <p className="panel__note">
              questo passo dichiara una catena di motori — {chain.join(" › ")} — in ordine di
              preferenza. Il pannello non la sa ancora comporre: resta com'è, e si legge fra i
              parametri qui sotto. Scegliere uno strumento qui la sostituisce.
            </p>
          )}

          {tools.length > 0 ? (
            <label className="panel__field">
              <span>Strumento</span>
              {/* Il segno sta accanto alla scelta, non dentro: un `<option>` non
                  ammette disegni, e nessun trucco per infilarceli sopravvive a
                  un elenco lungo o a chi naviga con la tastiera. */}
              <div className="panel__tool-pick">
                {choice.tool !== "" && (
                  <ToolMark id={choice.tool} size={20} off={chosen ? !chosen.available : true} />
                )}
              <select value={choice.tool} onChange={(event) => setChoice({ tool: event.target.value })}>
                <option value="">— nessuno —</option>
                {groupByKind(tools).map((group) => (
                  <optgroup key={group.kind} label={toolKindLabel(group.kind)}>
                    {group.tools.map((tool) => (
                      <option key={tool.id} value={tool.id}>
                        {tool.name}
                        {tool.available ? "" : " (non disponibile)"}
                      </option>
                    ))}
                  </optgroup>
                ))}
                {/* Uno strumento scelto altrove e assente qui non si cancella di
                    nascosto: resta scelto, e si vede che qui non c'è. */}
                {unknownChoice && (
                  <option value={choice.tool}>{choice.tool} — non rilevato su questa macchina</option>
                )}
              </select>
              </div>
            </label>
          ) : (
            <>
              <p className="panel__note">
                {discovery.state === "asking"
                  ? "chiedo al motore quali strumenti ci sono…"
                  : "nessuno strumento rilevato"}
              </p>
              {discovery.state === "mute" && <p className="panel__note-why">{discovery.why}</p>}
              {/* Senza elenco si scrive l'identificativo a mano: il nodo resta
                  componibile, e quando la scoperta risponderà lo ritroverà. */}
              <label className="panel__field">
                <span>Strumento (identificativo)</span>
                <input
                  value={choice.tool}
                  placeholder="l'identificativo che il motore dichiarerà"
                  onChange={(event) => setChoice({ tool: event.target.value })}
                />
              </label>
            </>
          )}

          {chosen && (
            <div className="panel__tool-detail" data-off={chosen.available ? undefined : true}>
              <p>
                <span className="panel__tool-kind">{toolKindLabel(chosen.kind)}</span>
                {chosen.version !== "" && <span> · {chosen.version}</span>}
              </p>
              {chosen.path !== "" && <p className="panel__tool-path">{chosen.path}</p>}
              {/* IL MOTIVO SI MOSTRA SEMPRE, non solo quando manca: quando c'è
                  dice da dove — ed è l'unico modo per accorgersi che si sta per
                  usare il binario sbagliato fra due installazioni. */}
              {chosen.reason !== "" && (
                <p className="panel__tool-why">
                  {chosen.available ? "" : "non disponibile: "}
                  {chosen.reason}
                </p>
              )}
              {chosen.descriptor !== "" && (
                <p className="panel__tool-src">riconosciuto dal descrittore «{chosen.descriptor}»</p>
              )}
            </div>
          )}
          {unknownChoice && tools.length > 0 && (
            <p className="panel__tool-detail" data-off>
              questo strumento non è fra quelli rilevati qui: il flusso resta valido, ma su questa
              macchina non partirebbe.
            </p>
          )}
          {/* Due risposte alla stessa domanda: chi esegue non saprebbe a quale
              credere, e il pannello non può scegliere al posto di chi scrive. */}
          {rival !== "" && choice.tool !== "" && (
            <p className="panel__warn">
              questo passo dichiara anche un binario suo («bin»: {rival}): due verità su chi lo
              esegue.
            </p>
          )}
          {rejectedBySchema && (
            <p className="panel__warn">
              lo schema d'ingresso di questo passo non ammette campi in più: con questi il motore
              rifiuterebbe il flusso al caricamento.
            </p>
          )}

          <datalist id="known-models">
            {models.map((model) => (
              <option key={model} value={model} />
            ))}
          </datalist>
          <label className="panel__field">
            <span>Modello</span>
            {/* Un elenco chiuso qui sarebbe una bugia: nessun descrittore
                dichiara i modelli, e scriverne a memoria una decina darebbe a
                chi sceglie l'impressione che la macchina li abbia trovati. Il
                campo resta libero, e i suggerimenti sono soltanto quelli che
                esistono davvero — i modelli scritti negli altri passi. */}
            <input
              list="known-models"
              value={choice.model}
              placeholder="il nome del modello, come lo scrive lo strumento"
              onChange={(event) => setChoice({ model: event.target.value })}
            />
          </label>
          {models.length === 0 && chosen && (
            <p className="panel__note-why">
              nessun modello da suggerire: il descrittore di «{chosen.id}» non ne dichiara, e
              inventarne sarebbe peggio che lasciare il campo libero.
            </p>
          )}

          <ToolOptions
            tool={chosen}
            options={choice.options}
            onChange={(options) => setChoice({ options })}
          />

          <label className="panel__field">
            <span>Prompt</span>
            <textarea
              className="panel__prompt"
              rows={6}
              value={choice.prompt}
              placeholder="cosa deve fare, detto per intero"
              onChange={(event) => setChoice({ prompt: event.target.value })}
            />
          </label>
        </div>
      )}

      <label className="panel__field">
        <span>Tetto tentativi</span>
        <input
          type="number"
          min={1}
          value={step.max_attempts}
          onChange={(event) => onField({ max_attempts: Math.max(1, Number(event.target.value) || 1) })}
        />
      </label>

      <div className="panel__field">
        <span>Dipende da</span>
        {siblingIds.length === 0 ? (
          <p className="panel__empty">nessun altro passo in questo flusso</p>
        ) : (
          <div className="panel__deps">
            {siblingIds.map((id) => (
              <label key={id} className="panel__dep">
                <input
                  type="checkbox"
                  checked={step.deps.includes(id)}
                  onChange={(event) => onToggleDep(id, event.target.checked)}
                />
                {id}
              </label>
            ))}
          </div>
        )}
      </div>

      <label className="panel__field">
        <span>{usesTool ? "Altri parametri (JSON)" : "Parametri (JSON)"}</span>
        <textarea
          className="panel__with"
          rows={5}
          value={withDraft}
          onChange={(event) => setWithDraft(event.target.value)}
          onBlur={commitWith}
          placeholder="nessuno"
        />
      </label>
      {withError && <p className="panel__error">JSON non valido: {withError}</p>}

      <button type="button" className="panel__delete" onClick={onDelete}>
        Elimina passo
      </button>
    </>
  );
}

// ── le opzioni ───────────────────────────────────────────────────────────

interface ToolOptionsProps {
  /** Lo strumento scelto, se è fra quelli rilevati. */
  tool: Tool | undefined;
  options: Record<string, OptionValue>;
  onChange: (options: Record<string, OptionValue>) => void;
}

/** Una riga in composizione. L'`id` non esce da qui: serve a React per non
 *  rimontare la riga mentre se ne scrive il nome — senza, il campo perde il
 *  fuoco a ogni carattere digitato. */
interface OptionRow {
  id: number;
  name: string;
  value: OptionValue;
}

/**
 * Le opzioni di un passo: una scelta, non una riga di comando da scrivere.
 *
 * DUE MODI, E IL SECONDO È QUELLO DI OGGI. Se lo strumento dichiara le proprie
 * opzioni (`options` nel descrittore) ognuna diventa un controllo con la sua
 * forma — un elenco, un interruttore, un numero. Nessun descrittore lo dichiara
 * ancora: `toolbox::Descriptor` conosce `detect`, `enumerate`, `version`,
 * `config` e `note`, e nient'altro. Finché è così si aggiungono coppie
 * nome/valore a mano, e il pannello DICE che è un ripiego invece di far credere
 * che quelle poche righe siano tutto ciò che lo strumento accetta.
 *
 * LE RIGHE VIVONO QUI, non nel passo, e non è un dettaglio: un'opzione senza
 * nome non si scrive nel file — sarebbe una chiave vuota per il motore — e
 * quindi una riga appena aggiunta, se la sua unica esistenza fosse quella
 * salvata, sparirebbe nell'istante in cui la si crea. Si tengono in bozza
 * finché non hanno un nome, e verso il passo scende solo quello che ha senso
 * scrivere.
 *
 * Quello che non si fa, in nessuno dei due modi, è indovinare: un elenco di
 * flag plausibili scritto qui sembrerebbe rilevato dalla macchina, e sarebbe
 * scritto da chi quella macchina non l'ha guardata.
 */
function ToolOptions({ tool, options, onChange }: ToolOptionsProps) {
  const declared: OptionSpec[] = tool?.options ?? [];
  const nextId = useRef(0);
  // Monta una volta per passo selezionato — `StepEditor` ha `key={selectedNode}`
  // in `App` — e quindi la bozza riparte pulita a ogni cambio di selezione.
  const [rows, setRows] = useState<OptionRow[]>(() =>
    Object.entries(options)
      .filter(([name]) => !declared.some((spec) => spec.key === name))
      .map(([name, value]) => ({ id: nextId.current++, name, value })),
  );

  /** Le righe libere più le dichiarate: quello che finisce nel passo. */
  function push(freeRows: OptionRow[], declaredValues: Record<string, OptionValue>) {
    const next: Record<string, OptionValue> = { ...declaredValues };
    for (const row of freeRows) {
      if (row.name.trim() !== "") next[row.name] = row.value;
    }
    onChange(next);
  }

  const declaredValues: Record<string, OptionValue> = {};
  for (const spec of declared) {
    if (spec.key in options) declaredValues[spec.key] = options[spec.key]!;
  }

  function editRows(update: (rows: OptionRow[]) => OptionRow[]) {
    setRows((current) => {
      const next = update(current);
      push(next, declaredValues);
      return next;
    });
  }

  function setDeclared(key: string, value: OptionValue | undefined) {
    const values = { ...declaredValues };
    if (value === undefined) delete values[key];
    else values[key] = value;
    push(rows, values);
  }

  // Due righe con lo stesso nome non possono convivere in un oggetto JSON: sul
  // disco ne sopravvive una sola. Meglio dirlo mentre si scrive che lasciarlo
  // scoprire riaprendo il file.
  const named = rows.map((row) => row.name.trim()).filter((name) => name !== "");
  const duplicated = named.some((name, index) => named.indexOf(name) !== index);

  const preview = optionsPreview({
    tool: "",
    model: "",
    prompt: "",
    options: (() => {
      const shown: Record<string, OptionValue> = { ...declaredValues };
      for (const row of rows) if (row.name.trim() !== "") shown[row.name] = row.value;
      return shown;
    })(),
  });

  return (
    <div className="panel__options">
      <div className="panel__subtitle">Opzioni</div>

      {declared.length > 0 ? (
        declared.map((spec) => (
          <DeclaredOption
            key={spec.key}
            spec={spec}
            value={declaredValues[spec.key]}
            onSet={(value) => setDeclared(spec.key, value)}
            onClear={() => setDeclared(spec.key, undefined)}
          />
        ))
      ) : (
        <p className="panel__note-why">
          {tool
            ? `il descrittore di «${tool.id}» non dichiara quali opzioni accetta: qui si scrivono a mano, e appena il descrittore le porterà diventeranno una scelta guidata.`
            : "scegli uno strumento per sapere quali opzioni accetta."}
        </p>
      )}

      {/* Le opzioni scritte a mano restano anche quando il descrittore ne
          dichiara altre: una che lui non conosce non è per forza sbagliata, ed
          è chi esegue a scoprirlo, non questo pannello. */}
      {rows.map((row) => (
        <FreeOption
          key={row.id}
          name={row.name}
          value={row.value}
          onRename={(to) =>
            editRows((current) => current.map((item) => (item.id === row.id ? { ...item, name: to } : item)))
          }
          onSet={(next) =>
            editRows((current) => current.map((item) => (item.id === row.id ? { ...item, value: next } : item)))
          }
          onDrop={() => editRows((current) => current.filter((item) => item.id !== row.id))}
        />
      ))}

      {duplicated && (
        <p className="panel__warn">
          due opzioni si chiamano allo stesso modo: nel file ne resterà una sola.
        </p>
      )}

      <button
        type="button"
        className="panel__option-add"
        onClick={() => editRows((current) => [...current, { id: nextId.current++, name: "", value: "" }])}
      >
        aggiungi un'opzione
      </button>

      {preview !== "" && (
        <p className="panel__option-preview" title="quello che si sta componendo, non quello che finisce nel file">
          <span>ne esce:</span> <code>{preview}</code>
        </p>
      )}
    </div>
  );
}

/** Un'opzione che il descrittore dichiara: la sua forma decide il controllo. */
function DeclaredOption({
  spec,
  value,
  onSet,
  onClear,
}: {
  spec: OptionSpec;
  value: OptionValue | undefined;
  onSet: (value: OptionValue) => void;
  onClear: () => void;
}) {
  const chosen = value !== undefined;
  return (
    <div className="panel__option" data-declared>
      <label className="panel__field">
        <span title={spec.help}>{spec.label ?? spec.key}</span>
        {spec.kind === "flag" ? (
          <input type="checkbox" checked={value === true} onChange={(event) => (event.target.checked ? onSet(true) : onClear())} />
        ) : spec.kind === "choice" ? (
          <select
            value={chosen ? String(value) : ""}
            onChange={(event) => (event.target.value === "" ? onClear() : onSet(event.target.value))}
          >
            <option value="">— non scelta —</option>
            {(spec.choices ?? []).map((option) => (
              <option key={option} value={option}>
                {option}
              </option>
            ))}
          </select>
        ) : (
          <input
            type={spec.kind === "number" ? "number" : "text"}
            value={chosen ? String(value) : ""}
            onChange={(event) => {
              const text = event.target.value;
              if (text === "") onClear();
              else onSet(spec.kind === "number" ? Number(text) : text);
            }}
          />
        )}
      </label>
      {spec.help && <p className="panel__option-help">{spec.help}</p>}
    </div>
  );
}

/** Un'opzione scritta a mano: nome e valore, e la si può togliere. */
function FreeOption({
  name,
  value,
  onRename,
  onSet,
  onDrop,
}: {
  name: string;
  value: OptionValue;
  onRename: (to: string) => void;
  onSet: (value: OptionValue) => void;
  onDrop: () => void;
}) {
  return (
    <div className="panel__option">
      <input
        className="panel__option-key"
        value={name}
        placeholder="--opzione"
        onChange={(event) => onRename(event.target.value)}
      />
      {/* Un interruttore è un'opzione senza valore: si dichiara con `true`, e
          scriverlo così invece di lasciare il valore vuoto è la differenza fra
          `--verbose` e `--verbose ""`. */}
      {value === true ? (
        <span className="panel__option-flag">senza valore</span>
      ) : (
        <input
          className="panel__option-value"
          value={String(value)}
          placeholder="valore"
          onChange={(event) => onSet(event.target.value)}
        />
      )}
      <label className="panel__option-toggle" title="un'opzione che non vuole un valore">
        <input
          type="checkbox"
          checked={value === true}
          onChange={(event) => onSet(event.target.checked ? true : "")}
        />
        <span>sola</span>
      </label>
      <button type="button" className="panel__option-drop" onClick={onDrop} title="togli questa opzione">
        ×
      </button>
    </div>
  );
}
