import type { MacroStep } from "./api";

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

/** Sends the steps with their pauses; `signal` stops between steps. */
export async function playMacro(
  steps: MacroStep[],
  write: (data: string) => Promise<void>,
  signal: AbortSignal,
): Promise<void> {
  for (const step of steps) {
    if (step.delayMs > 0) await new Promise((resolve) => setTimeout(resolve, step.delayMs));
    if (signal.aborted) return;
    await write(step.data);
  }
}
