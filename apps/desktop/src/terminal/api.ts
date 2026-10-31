// Typed wrappers around the terminal IPC commands. The backend validates all
// arguments; nothing here is a security boundary.
import { Channel, invoke } from "@tauri-apps/api/core";

export type TerminalId = number;

export interface ExitPayload {
  exit: { code: number; signal: string | null } | null;
}

export interface IpcError {
  code: string;
  message: string;
}

export function isIpcError(e: unknown): e is IpcError {
  return (
    typeof e === "object" &&
    e !== null &&
    typeof (e as IpcError).code === "string" &&
    typeof (e as IpcError).message === "string"
  );
}

export function openTerminal(
  cols: number,
  rows: number,
  onOutput: (data: Uint8Array) => void,
  onExit: (payload: ExitPayload) => void,
): Promise<TerminalId> {
  const output = new Channel<ArrayBuffer>();
  output.onmessage = (buf) => onOutput(new Uint8Array(buf));
  const exit = new Channel<ExitPayload>();
  exit.onmessage = onExit;
  return invoke<TerminalId>("terminal_open", { cols, rows, onOutput: output, onExit: exit });
}

export function writeTerminal(id: TerminalId, data: string): Promise<void> {
  return invoke("terminal_write", { id, data });
}

export function resizeTerminal(id: TerminalId, cols: number, rows: number): Promise<void> {
  return invoke("terminal_resize", { id, cols, rows });
}

export function ackTerminal(id: TerminalId, bytes: number): Promise<void> {
  return invoke("terminal_ack", { id, bytes });
}

export function closeTerminal(id: TerminalId): Promise<void> {
  return invoke("terminal_close", { id });
}
