import { type FormEvent, useEffect, useState } from "react";
import {
  type MasterStatus,
  isIpcError,
  masterChange,
  masterDisable,
  masterEnable,
  masterLock,
  masterReset,
  masterStatus,
  masterUnlock,
} from "../terminal/api";
import { Dialog } from "./Dialog";

const MIN_LENGTH = 8;

function message(e: unknown): string {
  if (isIpcError(e) && e.code === "wrong_master_password") return "Wrong master password.";
  return isIpcError(e) ? e.message : String(e);
}

/** Confirms deleting every saved password when the master password is forgotten. */
function ForgotPassword({ onDone }: { onDone: (reset: boolean) => void }) {
  const [asking, setAsking] = useState(false);
  const [error, setError] = useState<string | null>(null);
  if (!asking)
    return (
      <button type="button" className="link" onClick={() => setAsking(true)}>
        Forgot the master password?
      </button>
    );
  return (
    <div className="pane-warning">
      It cannot be recovered. Resetting deletes every saved password and the master password; you then
      enter server passwords again.
      <button
        type="button"
        className="danger"
        onClick={() => masterReset().then(() => onDone(true), (e) => setError(message(e)))}
      >
        Delete saved passwords
      </button>
      <button type="button" onClick={() => setAsking(false)}>
        Cancel
      </button>
      {error && <span role="alert"> {error}</span>}
    </div>
  );
}

/** Asked at startup when saved passwords are locked. Skipping is fine. */
export function UnlockDialog({ onClose, onReset }: { onClose: () => void; onReset?: () => void }) {
  const [password, setPassword] = useState("");
  const [busy, setBusy] = useState(false);
  const [error, setError] = useState<string | null>(null);

  async function submit(e: FormEvent) {
    e.preventDefault();
    setBusy(true);
    setError(null);
    try {
      await masterUnlock(password);
      setPassword("");
      onClose();
    } catch (err) {
      setError(message(err));
    } finally {
      setBusy(false);
    }
  }

  return (
    <Dialog
      title="Unlock saved passwords"
      onCancel={onClose}
      actions={
        <>
          <button type="button" onClick={onClose}>
            Not now
          </button>
          <button type="submit" form="unlock-form" className="primary" disabled={busy || !password}>
            Unlock
          </button>
        </>
      }
    >
      <form id="unlock-form" className="form-grid" onSubmit={submit}>
        <p className="span-2">
          Saved passwords are protected by your master password. Without it, servers ask for their password
          each time.
        </p>
        <label className="span-2">
          Master password
          <input type="password" value={password} onChange={(e) => setPassword(e.target.value)} autoFocus />
        </label>
        {error && (
          <p className="form-error span-2" role="alert">
            {error}
          </p>
        )}
        <div className="span-2">
          <ForgotPassword
            onDone={() => {
              onReset?.();
              onClose();
            }}
          />
        </div>
      </form>
    </Dialog>
  );
}

type Mode = "idle" | "set" | "change" | "off";

/** The Settings section: turn the master password on or off, change it, lock now. */
export function MasterPasswordSettings({
  secretsAvailable,
  onReset,
  portable = false,
}: {
  secretsAvailable: boolean;
  /** Portable mode: saving passwords needs a master password, which cannot be turned off. */
  portable?: boolean;
  /** Saved passwords were deleted (forgotten master password). */
  onReset?: () => void;
}) {
  const [status, setStatus] = useState<MasterStatus | null>(null);
  const [mode, setMode] = useState<Mode>("idle");
  const [current, setCurrent] = useState("");
  const [next, setNext] = useState("");
  const [confirm, setConfirm] = useState("");
  const [busy, setBusy] = useState(false);
  const [error, setError] = useState<string | null>(null);
  const [unlocking, setUnlocking] = useState(false);

  const refresh = () => masterStatus().then(setStatus, () => {});
  useEffect(() => {
    void refresh();
  }, []);

  const reset = () => {
    setMode("idle");
    setCurrent("");
    setNext("");
    setConfirm("");
    setError(null);
  };

  const needsNew = mode === "set" || mode === "change";
  const problem =
    needsNew && next.length < MIN_LENGTH
      ? `At least ${MIN_LENGTH} characters.`
      : needsNew && next !== confirm
        ? "The two passwords differ."
        : null;

  async function run() {
    setBusy(true);
    setError(null);
    try {
      if (mode === "set") await masterEnable(next);
      if (mode === "change") await masterChange(current, next);
      if (mode === "off") await masterDisable(current);
      reset();
      await refresh();
    } catch (e) {
      setError(message(e));
    } finally {
      setBusy(false);
    }
  }

  if (!secretsAvailable || !status) return null;
  return (
    <fieldset className="span-2 master-password">
      <legend>Master password</legend>
      <p className="hint">
        {status.protected
          ? status.locked
            ? "Saved passwords are locked."
            : "Saved passwords are protected by a master password."
          : portable
            ? "Portable mode: set a master password to save passwords on this drive. It cannot be recovered if forgotten."
            : "Optional: protect saved passwords with a password Termalith asks for at startup. It cannot be recovered if forgotten."}
      </p>
      {mode === "idle" && (
        <div className="pane-actions">
          {!status.protected && (
            <button type="button" onClick={() => setMode("set")}>
              Set master password…
            </button>
          )}
          {status.protected && status.locked && (
            <button type="button" onClick={() => setUnlocking(true)}>
              Unlock…
            </button>
          )}
          {status.protected && (
            <>
              <button type="button" onClick={() => setMode("change")}>
                Change…
              </button>
              {!portable && (
                <button type="button" onClick={() => setMode("off")}>
                  Turn off…
                </button>
              )}
            </>
          )}
          {status.protected && !status.locked && (
            <button type="button" onClick={() => void masterLock().then(refresh)}>
              Lock now
            </button>
          )}
        </div>
      )}
      {mode !== "idle" && (
        <div className="form-grid">
          {(mode === "change" || mode === "off") && (
            <label className="span-2">
              Current master password
              <input type="password" value={current} onChange={(e) => setCurrent(e.target.value)} autoFocus />
            </label>
          )}
          {needsNew && (
            <>
              <label>
                New master password
                <input type="password" value={next} onChange={(e) => setNext(e.target.value)} autoFocus={mode === "set"} />
              </label>
              <label>
                Repeat it
                <input type="password" value={confirm} onChange={(e) => setConfirm(e.target.value)} />
              </label>
            </>
          )}
          {mode === "set" && (
            <p className="pane-warning span-2">
              If you forget it, saved passwords cannot be recovered; you would reset and enter server passwords
              again.
            </p>
          )}
          {(error || (needsNew && next && problem)) && (
            <p className="form-error span-2" role="alert">
              {error ?? problem}
            </p>
          )}
          <div className="pane-actions span-2">
            <button type="button" onClick={reset}>
              Cancel
            </button>
            <button
              type="button"
              className="primary"
              disabled={busy || (needsNew && !!problem) || ((mode === "change" || mode === "off") && !current)}
              onClick={() => void run()}
            >
              {mode === "set" ? "Set" : mode === "change" ? "Change" : "Turn off"}
            </button>
          </div>
          {mode !== "set" && (
            <div className="span-2">
              <ForgotPassword
                onDone={() => {
                  reset();
                  onReset?.();
                  void refresh();
                }}
              />
            </div>
          )}
        </div>
      )}
      {unlocking && (
        <UnlockDialog
          onReset={onReset}
          onClose={() => {
            setUnlocking(false);
            void refresh();
          }}
        />
      )}
    </fieldset>
  );
}
