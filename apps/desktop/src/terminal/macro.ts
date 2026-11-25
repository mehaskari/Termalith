import type { MacroStep } from "./api";
import { plainText } from "./outputTap";

const MAX_DELAY_MS = 2000;
/** Keys closer together than this are one step, so playback stays smooth. */
const MERGE_MS = 40;

/** Collects keystrokes with the pauses between them. */
export class MacroRecorder {
  private readonly recorded: MacroStep[] = [];
  private last: number | null = null;

  add(data: string, now: number = Date.now()) {
    const delay = this.last === null ? 0 : Math.min(now - this.last, MAX_DELAY_MS);
    this.last = now;
    const previous = this.recorded[this.recorded.length - 1];
    if (previous && delay < MERGE_MS) previous.data += data;
    else this.recorded.push({ delayMs: delay, data });
  }

  get steps(): MacroStep[] {
    return this.recorded;
  }
}

const KEY_NAMES: Record<string, string> = {
  "\r": "⏎",
  "\t": "⇥",
  "\x7f": "⌫",
  "\x1b[A": "↑",
  "\x1b[B": "↓",
  "\x1b[C": "→",
  "\x1b[D": "←",
};

/** Readable form of recorded keys, for checking before saving. */
export function describeKeys(steps: MacroStep[]): string {
  const text = steps.map((s) => s.data).join("");
  return text.replace(/\x1b\[[A-D]|[\x00-\x1f\x7f]/g, (key) => {
    const name = KEY_NAMES[key];
    if (name) return name;
    if (key === "\x1b") return "⎋";
    return `^${String.fromCharCode(key.charCodeAt(0) + 64)}`;
  });
}

/** Waits `ms`, or less when `signal` stops the macro. */
function pause(ms: number, signal: AbortSignal): Promise<void> {
  return new Promise((resolve) => {
    const timer = setTimeout(resolve, ms);
    signal.addEventListener(
      "abort",
      () => {
        clearTimeout(timer);
        resolve();
      },
      { once: true },
    );
  });
}

/** How long a step waits for its text before the macro stops. */
export const WAIT_FOR_TIMEOUT_MS = 60_000;

export interface PlayOptions {
  /** Subscribes to the terminal's output (needed for steps that wait for text). */
  output?: (listener: (text: string) => void) => () => void;
  timeoutMs?: number;
}

/**
 * Sends the steps with their pauses; `signal` stops between steps and during
 * pauses. A step with `waitFor` first waits until that text appears in the
 * output that came after the previous step, and the macro stops with an
 * error if it does not within the time limit.
 */
export async function playMacro(
  steps: MacroStep[],
  write: (data: string) => Promise<void>,
  signal: AbortSignal,
  opts: PlayOptions = {},
): Promise<void> {
  const timeout = opts.timeoutMs ?? WAIT_FOR_TIMEOUT_MS;
  let seen = "";
  let wake: (() => void) | null = null;
  const unsubscribe = opts.output?.((text) => {
    seen = (seen + plainText(text)).slice(-16_384);
    wake?.();
  });
  try {
    for (const step of steps) {
      if (step.delayMs > 0) await pause(step.delayMs, signal);
      if (signal.aborted) return;
      const want = step.waitFor;
      if (want && unsubscribe) {
        const found = await new Promise<boolean>((resolve) => {
          const done = (v: boolean) => {
            wake = null;
            clearTimeout(timer);
            resolve(v);
          };
          const timer = setTimeout(() => done(false), timeout);
          signal.addEventListener("abort", () => done(true), { once: true });
          wake = () => {
            if (seen.includes(want)) done(true);
          };
          wake();
        });
        if (signal.aborted) return;
        if (!found) throw new Error(`Macro stopped: "${want}" did not appear within ${Math.round(timeout / 1000)} s.`);
      }
      seen = "";
      await write(step.data);
    }
  } finally {
    unsubscribe?.();
  }
}

/**
 * Keys as editable text: Enter is \r, Tab \t, Escape \e, a backslash \\,
 * other control characters \xNN. `decodeKeys` reverses it exactly.
 */
export function encodeKeys(data: string): string {
  return data.replace(/[\\\x00-\x1f\x7f]/g, (c) => {
    if (c === "\\") return "\\\\";
    if (c === "\r") return "\\r";
    if (c === "\n") return "\\n";
    if (c === "\t") return "\\t";
    if (c === "\x1b") return "\\e";
    return `\\x${c.charCodeAt(0).toString(16).padStart(2, "0")}`;
  });
}

/** The keys an `encodeKeys` text stands for; an unknown escape stays as typed. */
export function decodeKeys(text: string): string {
  return text.replace(/\\(x[0-9a-fA-F]{2}|[\\rnte])/g, (_, e: string) => {
    if (e === "\\") return "\\";
    if (e === "r") return "\r";
    if (e === "n") return "\n";
    if (e === "t") return "\t";
    if (e === "e") return "\x1b";
    return String.fromCharCode(Number.parseInt(e.slice(1), 16));
  });
}
