import { useEffect, useRef, useState } from "react";
import {
  type EmbeddedInput,
  closeEmbedded,
  embeddedAck,
  embeddedInput,
  isIpcError,
  openEmbedded,
} from "../terminal/api";
import { scancode } from "./scancodes";

interface Props {
  sessionId: string;
  name: string;
  /** Asks for the RDP password; null = cancelled. Never stored. */
  askPassword: () => Promise<string | null>;
  onClose: () => void;
}

type Status =
  | { kind: "connecting" }
  | { kind: "connected"; width: number; height: number }
  | { kind: "closed"; message: string | null };

/** Wheel units the server expects per notch (Windows' WHEEL_DELTA). */
const WHEEL_NOTCH = 120;

/**
 * Embedded RDP (experimental, ADR-030): frames are drawn into a canvas that
 * scales to the tab; keyboard (scan codes, so the remote layout applies)
 * and mouse go back to the server, batched per animation frame.
 */
export function RdpViewer({ sessionId, name, askPassword, onClose }: Props) {
  const canvasRef = useRef<HTMLCanvasElement>(null);
  const idRef = useRef<number | null>(null);
  const queue = useRef<EmbeddedInput[]>([]);
  const flushPending = useRef(false);
  const [status, setStatus] = useState<Status>({ kind: "connecting" });
  const [attempt, setAttempt] = useState(0);
  /** One password prompt per attempt, even if the effect runs twice (StrictMode). */
  const prompt = useRef<{ attempt: number; answer: Promise<string | null> } | null>(null);

  useEffect(() => {
    let cancelled = false;
    let opened: number | null = null;
    setStatus({ kind: "connecting" });
    (async () => {
      if (prompt.current?.attempt !== attempt) prompt.current = { attempt, answer: askPassword() };
      const password = await prompt.current.answer;
      if (cancelled) return;
      if (password === null) {
        setStatus({ kind: "closed", message: "Cancelled." });
        return;
      }
      const canvas = canvasRef.current;
      const box = canvas?.parentElement?.getBoundingClientRect();
      // Ask for the visible size (rounded to even pixels); the server may adjust it.
      const width = Math.max(640, Math.round((box?.width ?? 1024) / 2) * 2);
      const height = Math.max(480, Math.round((box?.height ?? 768) / 2) * 2);
      try {
        const id = await openEmbedded(
          sessionId,
          password,
          width,
          height,
          (f) => {
            const ctx = canvasRef.current?.getContext("2d");
            if (ctx) ctx.putImageData(new ImageData(f.rgba as Uint8ClampedArray<ArrayBuffer>, f.w, f.h), f.x, f.y);
            if (idRef.current !== null) void embeddedAck(idRef.current).catch(() => {});
          },
          (e) => {
            if (e.kind === "connected") {
              const c = canvasRef.current;
              if (c) {
                c.width = e.width;
                c.height = e.height;
              }
              setStatus({ kind: "connected", width: e.width, height: e.height });
            } else {
              setStatus({ kind: "closed", message: e.reason });
              if (idRef.current !== null) void closeEmbedded(idRef.current).catch(() => {});
              idRef.current = null;
            }
          },
        );
        opened = id;
        if (cancelled) void closeEmbedded(id).catch(() => {});
        else idRef.current = id;
      } catch (e) {
        if (!cancelled) setStatus({ kind: "closed", message: isIpcError(e) ? e.message : String(e) });
      }
    })();
    return () => {
      cancelled = true;
      const id = idRef.current ?? opened;
      idRef.current = null;
      if (id !== null) void closeEmbedded(id).catch(() => {});
    };
    // A new attempt reconnects; the callbacks read refs, not props.
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [sessionId, attempt]);

  // Keyboard goes to the desktop as soon as it is shown.
  useEffect(() => {
    if (status.kind === "connected") canvasRef.current?.focus();
  }, [status.kind]);

  const send = (e: EmbeddedInput) => {
    queue.current.push(e);
    if (flushPending.current) return;
    flushPending.current = true;
    requestAnimationFrame(() => {
      flushPending.current = false;
      const id = idRef.current;
      const batch = queue.current.splice(0, 256);
      if (id !== null && batch.length) void embeddedInput(id, batch).catch(() => {});
    });
  };

  /** Pointer position in remote desktop pixels. */
  const remote = (e: React.MouseEvent<HTMLCanvasElement>) => {
    const c = e.currentTarget;
    const r = c.getBoundingClientRect();
    const scale = Math.min(r.width / c.width, r.height / c.height);
    const offX = (r.width - c.width * scale) / 2;
    const offY = (r.height - c.height * scale) / 2;
    const x = Math.round((e.clientX - r.left - offX) / scale);
    const y = Math.round((e.clientY - r.top - offY) / scale);
    return { x: Math.min(Math.max(x, 0), c.width - 1), y: Math.min(Math.max(y, 0), c.height - 1) };
  };

  const key = (e: React.KeyboardEvent, down: boolean) => {
    const code = scancode(e.code);
    if (code === null) return;
    e.preventDefault();
    e.stopPropagation();
    send({ kind: "key", scancode: code, down });
  };

  const connected = status.kind === "connected";
  return (
    <div className="rdp-viewer">
      <div className="pane-toolbar">
        <span className="rdp-title">
          {name}
          {connected && (
            <span className="hint">
              {" "}
              · {status.width}×{status.height} · experimental
            </span>
          )}
        </span>
        <span className="pane-actions">
          {connected && (
            <button
              type="button"
              title="Send Ctrl+Alt+Del"
              onClick={() => {
                for (const [code, down] of [
                  [0x1d, true],
                  [0x38, true],
                  [0xe053, true],
                  [0xe053, false],
                  [0x38, false],
                  [0x1d, false],
                ] as const)
                  send({ kind: "key", scancode: code, down });
              }}
            >
              Ctrl+Alt+Del
            </button>
          )}
          <button type="button" onClick={onClose}>
            Disconnect
          </button>
        </span>
      </div>
      <div className="rdp-stage">
        <canvas
          ref={canvasRef}
          className="rdp-canvas"
          tabIndex={0}
          aria-label={`Remote desktop ${name}. Keyboard and mouse go to the remote computer.`}
          hidden={!connected}
          onKeyDown={(e) => key(e, true)}
          onKeyUp={(e) => key(e, false)}
          onBlur={() => send({ kind: "releaseAll" })}
          onMouseMove={(e) => send({ kind: "move", ...remote(e) })}
          onMouseDown={(e) => {
            e.currentTarget.focus();
            send({ kind: "move", ...remote(e) });
            send({ kind: "button", button: e.button, down: true });
          }}
          onMouseUp={(e) => send({ kind: "button", button: e.button, down: false })}
          onContextMenu={(e) => e.preventDefault()}
          onWheel={(e) => {
            const vertical = Math.abs(e.deltaY) >= Math.abs(e.deltaX);
            const delta = vertical ? e.deltaY : e.deltaX;
            if (delta === 0) return;
            // Up/left is positive in RDP.
            send({ kind: "wheel", vertical, units: delta < 0 ? WHEEL_NOTCH : -WHEEL_NOTCH });
          }}
        />
        {status.kind === "connecting" && <p className="rdp-status">Connecting to {name}…</p>}
        {status.kind === "closed" && (
          <div className="rdp-status">
            <p>{status.message ? `Disconnected: ${status.message}` : "Disconnected."}</p>
            <button type="button" onClick={() => setAttempt((a) => a + 1)}>
              Reconnect
            </button>
          </div>
        )}
      </div>
    </div>
  );
}
