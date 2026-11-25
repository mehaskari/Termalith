/**
 * Terminal output as text for whoever asks for it (a macro waiting for a
 * prompt). Nothing is decoded or kept while nobody listens.
 */
type Listener = (text: string) => void;

const listeners = new Map<number, Set<Listener>>();
const decoders = new Map<number, TextDecoder>();

/** Called by the terminal view for every chunk of output. */
export function publishOutput(terminalId: number, data: Uint8Array) {
  const set = listeners.get(terminalId);
  if (!set || set.size === 0) return;
  let decoder = decoders.get(terminalId);
  if (!decoder) {
    decoder = new TextDecoder();
    decoders.set(terminalId, decoder);
  }
  const text = decoder.decode(data, { stream: true });
  for (const l of set) l(text);
}

/** Receives the terminal's output from now on; returns the unsubscribe function. */
export function subscribeOutput(terminalId: number, listener: Listener): () => void {
  let set = listeners.get(terminalId);
  if (!set) {
    set = new Set();
    listeners.set(terminalId, set);
  }
  set.add(listener);
  return () => {
    set.delete(listener);
    if (set.size === 0) {
      listeners.delete(terminalId);
      decoders.delete(terminalId);
    }
  };
}

/** Escape sequences (colors, cursor moves) removed, so a prompt matches as shown. */
export function plainText(text: string): string {
  return text.replace(/\x1b\[[0-9;?]*[ -/]*[@-~]|\x1b\][^\x07\x1b]*(?:\x07|\x1b\\)|\x1b[@-Z\\-_]/g, "");
}
