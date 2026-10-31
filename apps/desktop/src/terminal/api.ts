// Typed wrappers around the IPC commands. The backend validates all
// arguments; nothing here is a security boundary.
import { Channel, invoke } from "@tauri-apps/api/core";

export type TerminalId = number;

export interface ExitPayload {
  exit: { code: number; signal: string | null } | null;
}

export interface IpcError {
  code: string;
  message: string;
  detail?: unknown;
}

export function isIpcError(e: unknown): e is IpcError {
  return (
    typeof e === "object" &&
    e !== null &&
    typeof (e as IpcError).code === "string" &&
    typeof (e as IpcError).message === "string"
  );
}

/** Opens a terminal of the given size and wires its output/exit streams. */
export type Opener = (
  cols: number,
  rows: number,
  onOutput: (data: Uint8Array) => void,
  onExit: (payload: ExitPayload) => void,
) => Promise<TerminalId>;

function channels(onOutput: (data: Uint8Array) => void, onExit: (p: ExitPayload) => void) {
  const output = new Channel<ArrayBuffer>();
  output.onmessage = (buf) => onOutput(new Uint8Array(buf));
  const exit = new Channel<ExitPayload>();
  exit.onmessage = onExit;
  return { onOutput: output, onExit: exit };
}

export const openLocalTerminal: Opener = (cols, rows, onOutput, onExit) =>
  invoke<TerminalId>("terminal_open", { cols, rows, ...channels(onOutput, onExit) });

/** Another terminal next to `source`: a local shell, or a shell on the same SSH connection. */
export const splitTerminal =
  (source: TerminalId): Opener =>
  (cols, rows, onOutput, onExit) =>
    invoke<TerminalId>("terminal_split", { source, cols, rows, ...channels(onOutput, onExit) });

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

// ---------------------------------------------------------------- sessions

export type AuthMethod = { kind: "password" } | { kind: "keyFile"; path: string } | { kind: "agent" };

export interface SessionInput {
  name: string;
  host: string;
  port: number;
  username: string;
  auth: AuthMethod;
  folder: string | null;
  /** Saved session to tunnel through (ProxyJump). */
  jumpVia?: string | null;
}

export interface SessionRecord extends SessionInput {
  id: string;
  hasSavedSecret: boolean;
  favorite?: boolean;
  updatedAt: number;
}

export interface AppInfo {
  version: string;
  secretsAvailable: boolean;
  agentAvailable: boolean;
}

export interface ImportCandidate {
  alias: string;
  input: SessionInput;
  warnings: string[];
  jumpAlias: string | null;
}

export const sshConfigPreview = () => invoke<ImportCandidate[]>("ssh_config_preview");
export const sshConfigImport = (aliases: string[]) =>
  invoke<{ created: string[]; skipped: string[]; unresolvedJumps: string[] }>("ssh_config_import", { aliases });

export const appInfo = () => invoke<AppInfo>("app_info");
export const listSessions = () => invoke<SessionRecord[]>("sessions_list");
export const createSession = (input: SessionInput) =>
  invoke<SessionRecord>("session_create", { input });
export const updateSession = (id: string, input: SessionInput) =>
  invoke<SessionRecord>("session_update", { id, input });
export const deleteSession = (id: string) => invoke<void>("session_delete", { id });
export const forgetSessionSecret = (id: string) => invoke<void>("session_forget_secret", { id });

// ---------------------------------------------------------------- SSH

export interface QuickTarget {
  host: string;
  port: number;
  username: string;
  auth: AuthMethod;
}

export type SshTarget = { sessionId: string } | { quick: QuickTarget };

export interface SshOpenRequest {
  sessionId?: string;
  quick?: QuickTarget;
  /** Sent once for this connection attempt; never stored in the UI. */
  password?: string;
  savePassword?: boolean;
}

export async function openSsh(
  request: SshOpenRequest,
  cols: number,
  rows: number,
  onOutput: (data: Uint8Array) => void,
  onExit: (payload: ExitPayload) => void,
): Promise<{ id: TerminalId; label: string }> {
  return invoke("ssh_open", { request, cols, rows, ...channels(onOutput, onExit) });
}

export interface HostKeyPrompt {
  requestId: number;
  host: string;
  port: number;
  algorithm: string;
  fingerprint: string;
}

export type HostKeyAnswer = "reject" | "acceptOnce" | "acceptAndSave";

export const respondHostKey = (requestId: number, answer: HostKeyAnswer) =>
  invoke<boolean>("host_key_respond", { requestId, answer });

// ---------------------------------------------------------------- files

export interface RemoteEntry {
  name: string;
  kind: "dir" | "file" | "symlink" | "other";
  size: number | null;
  modified: number | null;
  permissions: number | null;
}

export type TransferEvent =
  | { kind: "progress"; transferred: number; total: number | null }
  | { kind: "done"; destination: string }
  | { kind: "failed"; message: string }
  | { kind: "cancelled" };

export interface DroppedFile {
  token: number;
  name: string;
  size: number;
}

export const sftpHome = (terminalId: TerminalId) => invoke<string>("sftp_home", { terminalId });
export const sftpList = (terminalId: TerminalId, path: string) =>
  invoke<RemoteEntry[]>("sftp_list", { terminalId, path });

function transferChannel(onEvent: (e: TransferEvent) => void) {
  const ch = new Channel<TransferEvent>();
  ch.onmessage = onEvent;
  return ch;
}

export const sftpDownload = (
  terminalId: TerminalId,
  dir: string,
  name: string,
  size: number | null,
  onEvent: (e: TransferEvent) => void,
) =>
  invoke<{ id: number; downloadsDir: string }>("sftp_download", {
    terminalId,
    dir,
    name,
    size,
    onEvent: transferChannel(onEvent),
  });

export const sftpUpload = (
  terminalId: TerminalId,
  token: number,
  dir: string,
  onEvent: (e: TransferEvent) => void,
) => invoke<number>("sftp_upload", { terminalId, token, dir, onEvent: transferChannel(onEvent) });

export const cancelTransfer = (id: number) => invoke<boolean>("transfer_cancel", { id });

export const forgetHostKey = (host: string, port: number) =>
  invoke<{ removed: number; remainingInUserFile: number }>("known_host_forget", { host, port });

// ---------------------------------------------------------------- tunnels

export type TunnelKind = { kind: "local"; remoteHost: string; remotePort: number } | { kind: "socks" };

export type TunnelSpec = TunnelKind & { localPort: number; allowRemoteClients: boolean };

export type TunnelInfo = TunnelKind & {
  id: number;
  terminalId: number;
  localAddress: string;
  exposed: boolean;
  activeConnections: number;
  totalConnections: number;
};

export const startTunnel = (terminalId: TerminalId, spec: TunnelSpec) =>
  invoke<TunnelInfo>("tunnel_start", { terminalId, spec });
export const listTunnels = (terminalId: TerminalId) => invoke<TunnelInfo[]>("tunnel_list", { terminalId });
export const stopTunnel = (id: number) => invoke<boolean>("tunnel_stop", { id });

// ---------------------------------------------------------------- settings

export interface Settings {
  theme: "system" | "dark" | "light";
  fontSize: number;
  fontFamily: string;
  cursorStyle: "block" | "bar" | "underline";
  cursorBlink: boolean;
  scrollback: number;
}

export const DEFAULT_SETTINGS: Settings = {
  theme: "system",
  fontSize: 13,
  fontFamily: "",
  cursorStyle: "block",
  cursorBlink: true,
  scrollback: 5000,
};

export const getSettings = () => invoke<Settings>("settings_get");
export const saveSettings = (settings: Settings) => invoke<void>("settings_save", { settings });
export const setFavorite = (id: string, favorite: boolean) => invoke<void>("session_set_favorite", { id, favorite });
export const exportSessions = () => invoke<{ json: string; savedTo: string | null }>("sessions_export");
export const importSessions = (json: string) =>
  invoke<{ created: string[]; skipped: string[]; unresolvedJumps: string[] }>("sessions_import", { json });

// ---------------------------------------------------------------- serial

export interface SerialConfig {
  path: string;
  baudRate: number;
  dataBits: number;
  parity: "none" | "odd" | "even";
  stopBits: number;
  flowControl: "none" | "software" | "hardware";
}

export interface SerialPortInfo {
  path: string;
  description: string;
}

export const serialPorts = () => invoke<SerialPortInfo[]>("serial_ports");

export const openSerial =
  (config: SerialConfig): Opener =>
  (_cols, _rows, onOutput, onExit) =>
    invoke<TerminalId>("serial_open", { config, ...channels(onOutput, onExit) });

export function serialSummary(c: SerialConfig): string {
  return `${c.baudRate} ${c.dataBits}${c.parity[0]?.toUpperCase() ?? "N"}${c.stopBits}`;
}
