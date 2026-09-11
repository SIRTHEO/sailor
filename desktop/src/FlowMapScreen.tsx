/**
 * The place that asks for the flow files and hands them to the map.
 *
 * **IT EXISTS SO THE READING TRAVELS WITH THE SURFACE.** Asked for from `App`,
 * the reader and the fetch land in the chunk that loads before anything is on
 * screen, and a map opened once a week costs every start of the window; behind
 * this one lazy door they arrive when somebody opens the place.
 */
import { useEffect, useState } from "react";
import { FlowMap, type FlowMapAsk } from "./FlowMap";
import { flowTexts } from "./flowmapask";
import { readFlowMap } from "./flowmapread";

export interface FlowMapScreenProps {
  /** False in a browser, where there is no engine to read the files. */
  native: boolean;
  onOpen?: (flow: string) => void;
}

/** Outside the shell there is no fault to report, and no answer either. */
const NO_ENGINE = "outside the shell: the engine reads the flow files";

export function FlowMapScreen({ native, onOpen }: FlowMapScreenProps) {
  const [ask, setAsk] = useState<FlowMapAsk>(() =>
    native ? { state: "reading" } : { state: "unreadable", why: NO_ENGINE },
  );

  // READ ON ARRIVAL, NOT ON A BEAT. Every flow file on the machine is a few
  // dozen reads: cheap once, wasteful every four seconds, and stale the moment
  // a flow is saved — so coming back to the place is what refreshes it.
  useEffect(() => {
    if (!native) return;
    let watching = true;
    flowTexts()
      .then((files) => {
        if (watching) setAsk({ state: "read", reading: readFlowMap(files) });
      })
      .catch((error: unknown) => {
        if (watching) setAsk({ state: "unreadable", why: String(error) });
      });
    return () => {
      watching = false;
    };
  }, [native]);

  return <FlowMap ask={ask} onOpen={onOpen} />;
}
