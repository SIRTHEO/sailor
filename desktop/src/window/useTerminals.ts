import { useEffect, useMemo, useRef, useState } from "react";
import { useAsk } from "../ask";
import { t } from "../i18n";
import { commandLines, type CommandLine } from "../profiles";
import { ATTESTED_MS, listTerminals, OutputBus, watchTerminals, type TerminalSummary } from "../terminal";

/** How often the list is re-asked. A terminal is born and dies by hand. */
const REFRESH_MS = 4000;

/**
 * **THE LIST IS THE ENGINE'S, THE REST IS WHAT THIS WINDOW SAW.** Which
 * terminals exist is asked of `terminal_list` every time; which of them sent
 * `terminal_closed`, whether the channel attached and who is talking right now
 * are the three facts only a window that was listening can know.
 */
export function useTerminals(native: boolean) {
  const { asked, again } = useAsk<TerminalSummary[]>(native, listTerminals, REFRESH_MS, t("window.outside_the_shell"));
  const bus = useMemo(() => new OutputBus(), []);
  const [closed, setClosed] = useState<ReadonlyMap<string, string>>(() => new Map());
  const [channel, setChannel] = useState<{ on: boolean; why: string | null }>({ on: false, why: null });

  // The bytes stay out of React: what crosses over is who is talking.
  const [speaking, setSpeaking] = useState<ReadonlySet<string>>(new Set());
  useEffect(() => bus.watchSpeaking((now) => setSpeaking(now)), [bus]);

  // Without a clock an agent that stopped an hour ago stays drawn working.
  const [now, setNow] = useState(() => Date.now());
  useEffect(() => {
    if (!native) return;
    const tick = setInterval(() => setNow(Date.now()), ATTESTED_MS / 4);
    return () => clearInterval(tick);
  }, [native]);

  // Read once: what the product knows how to start, so the pane can tell an
  // agent from a shell. Not having it costs that word and nothing else.
  const [lines, setLines] = useState<CommandLine[]>([]);
  useEffect(() => {
    if (!native) return;
    let dropped = false;
    void commandLines().then((known) => { if (!dropped) setLines(known); }, () => {});
    return () => { dropped = true; };
  }, [native]);

  const refresh = useRef(again);
  refresh.current = again;

  useEffect(() => {
    if (!native) return;
    let stop: (() => void) | null = null;
    let dropped = false;
    void watchTerminals({
      // One pane at a time: bytes for a terminal nobody holds are not lost,
      // the pane that opens it later reads them back from the backlog.
      onOutput: (id, bytes, at) => { bus.deliver(id, bytes, at); },
      onClosed: (id, status) => {
        setClosed((before) => new Map(before).set(id, status));
        // Re-asked at once, so the list and the event agree again.
        refresh.current();
      },
    }).then((outcome) => {
      if ("why" in outcome) {
        setChannel({ on: false, why: outcome.why });
        return;
      }
      if (dropped) {
        outcome.stop();
        return;
      }
      stop = outcome.stop;
      setChannel({ on: true, why: null });
    });
    return () => {
      dropped = true;
      stop?.();
    };
  }, [native, bus]);

  return { asked, again, bus, closed, channel, speaking, now, lines };
}
