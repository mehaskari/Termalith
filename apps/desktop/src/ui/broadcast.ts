// Multi-terminal input ("broadcast"). Pure helpers for testing; the UI
// enforces explicit enabling, a confirmed target list and a visible banner.

export interface BroadcastTab {
  key: number;
  title: string;
  terminalId?: number;
  ended?: boolean;
  kind: string;
}

export interface BroadcastState {
  /** Tab whose keyboard input is mirrored. */
  source: number;
  /** Tabs receiving the input (never includes the source). */
  targets: number[];
  /** Show the source and targets side by side instead of as tabs. */
  tiled?: boolean;
}

const TERMINAL_KINDS = new Set(["local", "ssh", "serial", "telnet"]);

/**
 * Tabs that can be picked as targets: live terminals, plus `connecting`
 * terminal tabs still opening (input reaches them once they are connected).
 */
export function pickableTargets(tabs: BroadcastTab[], source: number, connecting: number[] = []): BroadcastTab[] {
  return tabs.filter(
    (t) =>
      t.key !== source &&
      TERMINAL_KINDS.has(t.kind) &&
      !t.ended &&
      (t.terminalId !== undefined || connecting.includes(t.key)),
  );
}

/** Tabs that can receive input right now. */
export function eligibleTargets(tabs: BroadcastTab[], source: number): BroadcastTab[] {
  return tabs.filter((t) => t.key !== source && t.kind !== "tools" && t.terminalId !== undefined && !t.ended);
}

/** Splits configured targets into live ones (with terminal ids) and skipped ones. */
export function resolveTargets(
  state: BroadcastState,
  tabs: BroadcastTab[],
): { live: Array<{ key: number; terminalId: number }>; skipped: BroadcastTab[] } {
  const live: Array<{ key: number; terminalId: number }> = [];
  const skipped: BroadcastTab[] = [];
  for (const key of state.targets) {
    const t = tabs.find((x) => x.key === key);
    if (!t) continue;
    if (t.terminalId === undefined || t.ended) skipped.push(t);
    else live.push({ key, terminalId: t.terminalId });
  }
  return { live, skipped };
}
