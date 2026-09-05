/* ═══════════════════════════════════════════════════════════════════════
   Quarta passata. Le tre cose che mancavano, misurate:
   · la tela era piena al 7% — adesso porta TUTTI i flussi in corsie;
   · i terminali erano una scheda con del testo dentro — adesso sono quattro,
     affiancati, ognuno con la sua riga d'ingresso;
   · il deposito non si vedeva — adesso si guarda come un database, con le
     tabelle vere lette da `crates/ledger/src/lib.rs`.
   ═══════════════════════════════════════════════════════════════════════ */

const VOCI = [
  ["board",     "◈", "Board",     "22"],
  ["terminals", "▮", "Terminals", "4"],
  ["ledger",    "▤", "Ledger",    ""],
  ["memory",    "◷", "Runs",      ""],
  ["sailor",    "⚓", "Sailor",    ""],
];

/* ─── le corsie: nome, colore, stato, passi ─── */
const FLUSSI = [
  ["relay", "#2563eb", "running", "$0.34", [
    ["check","went","chain-brake"],["check","went","pane-is-idle"],
    ["engine","running","write-the-baton"],["check","waiting","prompt-is-empty"],
    ["gesture","waiting","send-clear"],["check","waiting","signal-is-gone"]]],
  ["first-run", "#146447", "went", "—", [["check","went","working-tree-is-clean"]]],
  ["build-sailor", "#92531b", "broke", "$0.08", [
    ["check","went","fmt"],["check","broke","green-checks"],
    ["engine","waiting","fix"],["deposit","waiting","record"]]],
  ["pass-the-baton", "#6b46a8", "waiting", "—", [
    ["trigger","waiting","on-idle"],["human","waiting","ask-theo"],
    ["engine","waiting","hand-over"]]],
];
const COL = {went:"var(--went)",running:"var(--running)",waiting:"var(--waiting)",
             broke:"var(--broke)",capped:"var(--capped)",human:"var(--human)"};

const nd = ([k,s,id], sel) => `<div class="nd" ${sel?"data-sel":""}>
  <div class="ndh"><span class="ndk">${k}</span>
    <span class="nds" style="color:${COL[s]}">${s}</span></div>
  <div class="ndi">${id}</div></div>`;

const corsia = ([nome,col,stato,costo,passi], on) => `
  <div class="lane" ${on?"data-on":""}>
    <div class="lh"><span class="d2" style="background:${col}"></span>
      <span class="n2">${nome}</span>
      <span class="s2" style="color:${COL[stato]}">${passi.length} steps · ${stato}</span>
      <span class="grow"></span><span class="cost">${costo}</span></div>
    <div class="row2">${passi.map((p,i)=>nd(p, on&&i===2))
      .map((h,i)=> i ? `<span class="w2" ${passi[i][1]==="waiting"?"data-w":""}></span>`+h : h).join("")}</div>
  </div>`;

/* ─── i terminali: quattro, e ognuno si usa ─── */
const TERM = [
  ["relay","claude opus-5","var(--running)","~/personal/sailor",
   `<span class="p">›</span> cargo test -p sailor --no-fail-fast<br>
&nbsp;&nbsp; Compiling ledger v0.1.0<br>&nbsp;&nbsp; Compiling flow v0.1.0<br>
&nbsp;&nbsp; Compiling actions v0.1.0<br>&nbsp;&nbsp; Compiling sailor v0.1.0<br>
<span class="w">warning</span>: unused variable «ledger»<br>
&nbsp;&nbsp;--&gt; crates/sailor/src/system.rs:214:9<br>
&nbsp;&nbsp;&nbsp;&nbsp;|<br>
214 |&nbsp;&nbsp;&nbsp; let ledger = open_ledger()?;<br>
&nbsp;&nbsp;&nbsp;&nbsp;|&nbsp;&nbsp;&nbsp;&nbsp;&nbsp;&nbsp;^^^^^^ help: prefix with an underscore<br><br>
&nbsp;&nbsp;&nbsp; Finished test profile in 41.28s<br>
&nbsp;&nbsp;&nbsp;&nbsp;Running unittests src/lib.rs<br><br>
running 247 tests<br>
....................................................................<br>
....................................................................<br>
test result: <span class="p">ok</span>. 247 passed; 0 failed; 0 ignored<br><br>
<span class="p">›</span> sailor flow run relay<br>
&nbsp;&nbsp;<span class="p">went</span>&nbsp;&nbsp;chain-brake&nbsp;&nbsp;&nbsp;&nbsp;&nbsp;0.4s<br>
&nbsp;&nbsp;<span class="p">went</span>&nbsp;&nbsp;pane-is-idle&nbsp;&nbsp;&nbsp;&nbsp;0.2s<br>
&nbsp;&nbsp;<span class="w">···</span>&nbsp;&nbsp;write-the-baton&nbsp;&nbsp;12s&nbsp;&nbsp;$0.08`, true],
  ["checks","codex","var(--waiting)","~/…/sailor-worktrees/checks",
   `<span class="p">›</span> npm run build<br>&nbsp;&nbsp;tsc --noEmit<br>
&nbsp;&nbsp;vitest run<br>&nbsp;&nbsp;Test Files 23 passed<br>
&nbsp;&nbsp;Tests 247 passed<br>&nbsp;&nbsp;<span class="p">✓</span> built in 1.22s`, false],
  ["sources","zsh","var(--went)","~/personal/sailor",
   `<span class="p">›</span> git log --oneline -4<br>29a45553 style(window)<br>
21a0e8a7 test(window)<br>29bafbef feat(window)<br>05fbcf3d chore(init)`, false],
  ["night","claude","var(--broke)","~/personal/sailor",
   `<span class="p">›</span> sailor flow run night<br>
<span class="e">error</span>: unknown field «retries»<br>&nbsp;&nbsp;at flows/night.flow.json:14<br>
&nbsp;&nbsp;<span class="w">help</span>: did you mean «max_attempts»?`, false],
];
const cella = ([n,cli,col,dir,out], on) => `<div class="tcell" ${on?"data-on":""}>
  <div class="thead"><span class="live2" style="background:${col}"></span><b>${n}</b>
    <span>${cli}</span><span class="grow"></span><span>${dir}</span><span class="x2">×</span></div>
  <div class="tbody">${out}</div>
  <div class="tin"><span>›</span><input placeholder="type a command, or ask about a flow"><span class="caret"></span></div>
</div>`;

/* ─── il deposito: le tabelle vere di crates/ledger/src/lib.rs ─── */
const TABELLE = [
  ["runs", 0, ["run_id","kind","entity","status","total_cost_micros","started_at","ended_at","error"]],
  ["steps", 0, ["run_id","step_id","attempt","species","outcome","started_at","input_digest"]],
  ["events", 0, ["event_id","run_id","kind","at","payload"]],
  ["model_calls", 0, ["call_id","run_id","step_id","cli","requested_model","actual_model","input_tokens","output_tokens","cost_micros"]],
  ["inventory_items", 310, ["item_id","kind","name","first_seen","last_seen","gone_at"]],
  ["processes", 2, ["pid","purpose","started_at","ended_at"]],
  ["snapshots", 0, ["run_id","at","state"]],
  ["store", 0, ["key","value","written_at"]],
];
const RIGHE_INV = [
  ["skill:better-layout","skill","better-layout","2026-08-27","2026-09-02",null],
  ["skill:form-design","skill","form-design","2026-08-27","2026-09-02",null],
  ["cli:claude","cli","claude 2.1.4","2026-08-10","2026-09-02",null],
  ["cli:codex","cli","codex 0.9.2","2026-08-10","2026-09-02",null],
  ["cli:gemini","cli","gemini","2026-08-14","2026-08-29","2026-08-30"],
  ["hook:pre-commit","hook","pre-commit","2026-08-20","2026-09-02",null],
];

function deposito(tab){
  const t = TABELLE.find(x => x[0] === tab);
  const vuota = t[1] === 0;
  return `<div class="dbwrap">
    <div class="dbside">
      <div class="grp" style="padding:3px 13px 6px">Tables</div>
      ${TABELLE.map(([n,c]) => `<div class="dbtable" data-tab="${n}" ${n===tab?"data-on":""}>
        ${n}<span class="cnt">${c || "0"}</span></div>`).join("")}
      <div class="grp" style="padding:13px 13px 6px">The file</div>
      <div style="padding:0 13px"><span class="where">~/.config/sailor/ledger</span></div>
    </div>
    <div class="dbmain">
      <div class="dbq"><input value="select * from ${tab} order by 1 desc limit 200" spellcheck="false">
        <button class="act">Run</button></div>
      ${vuota ? `<div class="empty"><b>This table is empty</b>
          The ledger has never been created on this machine — <span class="where">~/.config/sailor/ledger</span>
          does not exist yet.<br>Showing the schema instead of a made-up count.
          <table style="margin-top:15px;text-align:left"><tr><th>column</th></tr>
          ${t[2].map(c=>`<tr><td class="m">${c}</td></tr>`).join("")}</table></div>`
        : `<div class="dbgrid"><table><tr>${t[2].map(c=>`<th>${c}</th>`).join("")}</tr>
          ${RIGHE_INV.map((r,i)=>`<tr ${i===2?"data-on":""}>${r.map(v =>
            `<td>${v === null ? '<span class="null">null</span>' : v}</td>`).join("")}</tr>`).join("")}
          </table></div>
        <div class="dbrow"><div class="colhead">Selected row</div>
          <dl class="kv3">${t[2].map((c,i)=>`<dt>${c}</dt><dd>${
            RIGHE_INV[2][i] === null ? '<span class="null">null</span>' : RIGHE_INV[2][i]}</dd>`).join("")}</dl></div>`}
    </div></div>`;
}

const SCHERMI = {
board: {crumb:["Board"], gesti:true, stage:`<div class="canv"><div class="paper2"></div>
  <div style="position:relative;height:100%;overflow:auto;padding-bottom:34px">
    ${FLUSSI.map((f,i)=>corsia(f, i===0)).join("")}</div>
  <div class="legend"><span>how a step ends</span>
    <i style="color:var(--went)">went</i><i style="color:var(--running)">running</i>
    <i style="color:var(--waiting)">waiting</i><i style="color:var(--broke)">broke</i>
    <i style="color:var(--capped)">capped</i><i style="color:var(--human)">to a person</i></div></div>`},

terminals: {crumb:["Terminals"],
  stage:`<div class="tgrid">${TERM.map((t,i)=>cella(t,i===0)).join("")}</div>`},

ledger: {crumb:["Ledger","inventory_items"], stage: deposito("inventory_items")},

memory: {crumb:["Runs","Today"], stage:`<div class="sheet">
  <div class="sheettabs"><button data-on>Runs</button><button>Spend</button><button>Quota</button><button>Faults</button></div>
  <div class="split" style="margin-bottom:13px">
    <div class="card"><h3>4 runs today</h3><p>3 went · 1 broke · 0 capped</p></div>
    <div class="card"><h3>$2.34</h3><p>128k tokens · 4 calls · <b>3 without a price</b></p>
      <p style="color:var(--warn);margin-top:4px">A total with an unknown in it is a floor, not a sum.</p></div>
  </div>
  <table><tr><th>run</th><th>ended</th><th>when</th><th>lasted</th><th>steps</th><th>cost</th></tr>
  <tr><td class="m">relay</td><td><span class="tag" data-t="ok">went</span></td><td>12 min ago</td>
      <td class="n">4m 12s</td><td class="n">7/7</td><td class="n">$0.34</td></tr>
  <tr><td class="m">build-sailor</td><td><span class="tag" data-t="no">broke</span></td><td>1 h ago</td>
      <td class="n">1m 03s</td><td class="n">3/9</td><td class="n">$0.08</td></tr>
  <tr><td class="m">first-run</td><td><span class="tag" data-t="ok">went</span></td><td>2 h ago</td>
      <td class="n">0m 18s</td><td class="n">1/1</td><td class="n">—</td></tr></table>
  <div class="split" style="margin-top:15px">
    <div class="card"><h3>Five-hour window</h3><p>50% used · resets 03:29</p>
      <div class="bar-mini"><i style="width:50%;background:var(--warn)"></i></div></div>
    <div class="card"><h3>Seven-day window</h3><p>32% used</p>
      <div class="bar-mini"><i style="width:32%;background:var(--ok)"></i></div></div></div></div>`},

sailor: {crumb:["Sailor","What it keeps"], stage:`<div class="sheet">
  <div class="sheettabs"><button data-on>What it keeps</button><button>What it can do</button>
    <button>Engines &amp; profiles</button><button>Models</button><button>Appearance</button></div>
  <h2>What Sailor keeps</h2>
  <p class="note">Everything Sailor keeps, where it actually lives, and how much room it takes.
     A thing whose place you do not know is a thing you do not control.</p>
  <table><tr><th>what</th><th>where</th><th>how many</th><th>size</th></tr>
  <tr><td>Flows — yours</td><td><span class="where">~/.config/sailor/flows</span></td><td class="n">21</td><td class="n">344 KB</td></tr>
  <tr><td>Flows — this project</td><td><span class="where">./flows</span></td><td class="n">1</td><td class="n">28 KB</td></tr>
  <tr><td>Flows — shipped</td><td><span class="where">inside the binary</span></td><td class="n">3</td><td class="n">—</td></tr>
  <tr><td>Ledger</td><td><span class="where">~/.config/sailor/ledger</span></td>
      <td colspan="2" style="color:var(--warn)">not created yet — nothing written here</td></tr>
  <tr><td>Faults</td><td><span class="where">docs/guasti-incontrati.md</span></td><td class="n">66</td><td class="n">96 KB</td></tr>
  <tr><td>Machine inventory</td><td><span class="where">~/.config/sailor/equipment</span></td><td class="n">1</td><td class="n">652 KB</td></tr>
  <tr><td>Prices</td><td><span class="where">~/.config/sailor/pricing.json</span></td><td class="n">48</td><td class="n">8 KB</td></tr>
  <tr><td>Signing identity</td><td><span class="where">~/.config/sailor/signing-identity</span></td><td class="n">1</td><td class="n">4 KB</td></tr></table>
  <p class="note" style="margin-top:9px;color:var(--warn)"><b>The empty row is the point.</b>
     A store that is missing, shown as a plausible count, would not be hiding — it would be
     telling you something false.</p></div>`},
};

const PASSO = `<div class="full" id="full">
  <div class="fullbar"><span class="crumb"><b>relay</b><span class="chev">›</span><span>write-the-baton</span></span>
    <span class="pill" data-t="run">running · 12s</span><span class="grow"></span>
    <button class="act">Run this step only</button>
    <button class="act" id="chiudi">Close <kbd>Esc</kbd></button></div>
  <div class="fullbody">
    <section class="col"><div class="colhead">What comes in</div>
      <div class="code"><span class="k">pane</span>: <span class="v">"relay-01"</span><br>
        <span class="k">mandate</span>: <span class="v">"~/mandates/baton.md"</span></div>
      <p class="tiny">From <b>pane-is-idle</b>, the step before.</p></section>
    <section class="col"><div class="colhead">What it does</div>
      <label class="lbl">Engine</label><select class="field"><option>claude · opus-5</option></select>
      <label class="lbl">Prompt</label>
      <textarea class="field" rows="4">Write the baton into the pane and hand over.</textarea>
      <label class="lbl">Kind</label>
      <div class="seg"><button>Repeatable</button><button data-on>Compensable</button><button>To a person</button></div>
      <div class="two"><div><label class="lbl">Attempts</label><input class="field" value="3"></div>
        <div><label class="lbl">Time cap</label><input class="field" value="20 min"></div></div></section>
    <section class="col"><div class="colhead">What comes out</div>
      <div class="code"><span class="k">session</span>: <span class="v">SessionId</span></div>
      <p class="tiny">Goes to <b>prompt-is-empty</b>, the step after.</p>
      <div class="colhead" style="margin-top:14px">Live</div>
      <div class="term" style="height:170px"><span class="p">›</span> writing baton…<br>
        &nbsp;&nbsp;wrote 1 file<br><span class="p">›</span> <span class="caret"></span></div></section>
  </div></div>`;

const PALETTE = `<div class="scrim" id="scrim"><div class="pal">
  <input class="palinput" value="prof" readonly>
  <div class="palgrp">Go to</div>
  <div class="palrow" data-on><span class="palico">⚑</span>Engines &amp; profiles<span class="palkey">Sailor</span></div>
  <div class="palrow"><span class="palico">▤</span>Ledger → model_calls<span class="palkey">Ledger</span></div>
  <div class="palgrp">Do</div>
  <div class="palrow"><span class="palico">⇄</span>Switch profile to <b>codex lavoro</b></div>
  <div class="palrow"><span class="palico">↓</span>Import a flow from a file…</div>
  <div class="palrow"><span class="palico">▶</span>Run <b>relay</b></div>
  <div class="palfoot"><kbd>↑↓</kbd> move <kbd>↵</kbd> pick <kbd>Esc</kbd> close</div></div></div>`;

const DELTA = [
  ["La tela era piena al 7%","Misurato, non stimato. Mostrava un flusso solo su 1560px. Adesso porta <b>tutti i flussi in corsie</b>, come già fa il prodotto vero: il grafo è uno, le corsie sono i flussi dentro."],
  ["I terminali erano una scheda con del testo","Adesso sono <b>quattro affiancati</b>, ognuno con la sua riga d'ingresso, il suo motore, la sua cartella e il suo stato. È il posto dove uno ci passa la giornata."],
  ["Il deposito non si vedeva","Adesso si guarda <b>come un database</b>, perché è un database: le otto tabelle vere lette da <code>crates/ledger/src/lib.rs</code>, la query in cima, la griglia, la riga scelta sotto."],
  ["Una tabella vuota mostra lo schema","Non un conto inventato. Il deposito non esiste ancora su questa macchina: le tabelle a zero dicono le loro colonne e dove starebbe il file."],
  ["Via il pannello in basso","Serviva a tenere il terminale, e i terminali adesso hanno un posto vero. Un pannello che duplica una sezione è una sezione in meno."],
  ["La misura dello spreco, rifatta due volte","Il primo contatore guardava le foglie del DOM e dichiarava i terminali <b>vuoti al 6%</b> mentre allo schermo erano pieni: un pannello di testo è fatto di span minuscoli. Il secondo conta i pixel di sfondo — board 52%, ledger 50%, runs 56%, Sailor 51%. I terminali restano al 91% <b>e va bene così</b>: un terminale è nero per mestiere, e riempirlo di finto output per far contento un numero sarebbe il difetto, non la cura."],
];

function vai(dove){
  const s = SCHERMI[dove]; if(!s) return;
  document.getElementById("stage").innerHTML = s.stage;
  document.getElementById("crumb").innerHTML =
    s.crumb.map((c,i)=> i ? '<span class="chev">›</span><span>'+c+'</span>' : '<b>'+c+'</b>').join("")
    + (s.gesti ? '<span class="st"><span class="live"></span>relay <b>4 of 7</b> · <b>$0.34</b> today</span>' : "");
  for (const id of ["views","save","run"])
    document.getElementById(id).style.display = s.gesti ? "" : "none";
  document.getElementById("icons").innerHTML =
    '<div class="grp2">Work</div>' +
    VOCI.slice(0,2).map(v=>voce(v,dove)).join("") +
    '<div class="grp2">What happened</div>' +
    VOCI.slice(2,4).map(v=>voce(v,dove)).join("") +
    '<div class="grp2">Itself</div>' +
    VOCI.slice(4).map(v=>voce(v,dove)).join("");
  document.querySelectorAll("[data-go-to]").forEach(a => a.onclick = () => vai(a.dataset.goTo));
  document.querySelectorAll(".nd").forEach(n => n.ondblclick = apriPasso);
  document.querySelectorAll("[data-tab]").forEach(t => t.onclick = () => {
    SCHERMI.ledger.stage = deposito(t.dataset.tab);
    SCHERMI.ledger.crumb = ["Ledger", t.dataset.tab];
    vai("ledger");
  });
}
const voce = (v, dove) => '<a data-go-to="'+v[0]+'"'+(v[0]===dove?" data-on":"")+'>'+
  '<span class="gl">'+v[1]+'</span>'+v[2]+(v[3]?'<span class="badge">'+v[3]+'</span>':"")+'</a>';

function apriPasso(){
  document.getElementById("overlay").innerHTML = PASSO;
  document.getElementById("chiudi").onclick = () => document.getElementById("overlay").innerHTML = "";
}
function apriPalette(){
  document.getElementById("overlay").innerHTML = PALETTE;
  document.getElementById("scrim").onclick = () => document.getElementById("overlay").innerHTML = "";
}
document.addEventListener("keydown", e => {
  if (e.key === "Escape") document.getElementById("overlay").innerHTML = "";
  if (e.key === "k" && (e.metaKey || e.ctrlKey)) { e.preventDefault(); apriPalette(); }
});
document.getElementById("cmdk").onclick = apriPalette;
vai("board");
document.getElementById("delta").innerHTML = DELTA.map(([t,p]) =>
  '<div class="card"><h3>'+t+'</h3><p>'+p+'</p></div>').join("");
