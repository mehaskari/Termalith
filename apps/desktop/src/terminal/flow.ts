// Client side of terminal flow control and input ordering. Pure logic so it
// can be unit-tested without Tauri or a DOM.

/** Largest input string sent per IPC call, in UTF-16 code units. At most 3
 * UTF-8 bytes per unit keeps each chunk under the backend's 64 KiB limit. */
export const MAX_INPUT_UNITS = 16 * 1024;

/** Splits user input into IPC-sized chunks without separating surrogate
 * pairs. */
export function chunkInput(data: string, maxUnits = MAX_INPUT_UNITS): string[] {
  if (maxUnits < 2) throw new RangeError("maxUnits must be at least 2");
  const chunks: string[] = [];
  let start = 0;
  while (start < data.length) {
    let end = Math.min(start + maxUnits, data.length);
    if (end < data.length && isHighSurrogate(data.charCodeAt(end - 1))) end -= 1;
    chunks.push(data.slice(start, end));
    start = end;
  }
  return chunks;
}

function isHighSurrogate(unit: number): boolean {
  return unit >= 0xd800 && unit <= 0xdbff;
}

/**
 * Accumulates rendered-byte counts and reports them to the backend. Bytes
 * rendered before the terminal id is known are held, not dropped; losing
 * them would leave the backend's in-flight window permanently short.
 */
export class AckBatcher<Id> {
  private id: Id | null = null;
  private pending = 0;
  private scheduled = false;

  constructor(
    private readonly send: (id: Id, bytes: number) => void,
    // Wrapped: WebKit throws if queueMicrotask is called unbound.
    private readonly schedule: (fn: () => void) => void = (fn) => queueMicrotask(fn),
  ) {}

  setId(id: Id): void {
    this.id = id;
    this.kick();
  }

  add(bytes: number): void {
    this.pending += bytes;
    this.kick();
  }

  private kick(): void {
    if (this.id === null || this.pending === 0 || this.scheduled) return;
    this.scheduled = true;
    this.schedule(() => {
      this.scheduled = false;
      if (this.id === null || this.pending === 0) return;
      const bytes = this.pending;
      this.pending = 0;
      this.send(this.id, bytes);
    });
  }
}

/** Runs async tasks strictly one after another, so keystrokes reach the PTY
 * in the order they were typed. Failures are reported and do not stall the
 * queue. */
export class SerialQueue {
  private tail: Promise<void> = Promise.resolve();

  constructor(private readonly onError: (e: unknown) => void) {}

  push(task: () => Promise<void>): Promise<void> {
    this.tail = this.tail.then(task).catch(this.onError);
    return this.tail;
  }
}
