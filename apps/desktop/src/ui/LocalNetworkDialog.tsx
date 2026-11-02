import { useState } from "react";
import { openLocalNetworkSettings, restartApp } from "../terminal/api";
import { Dialog } from "./Dialog";

interface Props {
  onClose: () => void;
}

/**
 * Shown when a connection fails with "No route to host" on macOS, which is
 * what Local Network privacy returns when Termalith is not allowed.
 */
export function LocalNetworkDialog({ onClose }: Props) {
  const [failed, setFailed] = useState<string | null>(null);
  return (
    <Dialog
      title="Allow local network access"
      tone="warning"
      onCancel={onClose}
      actions={
        <>
          <button type="button" onClick={onClose}>
            Close
          </button>
          <button type="button" onClick={() => void restartApp().catch((e) => setFailed(String(e)))}>
            Restart Termalith
          </button>
          <button
            type="button"
            className="primary"
            onClick={() =>
              void openLocalNetworkSettings().catch(() =>
                setFailed("Could not open System Settings. Open it from the Apple menu."),
              )
            }
          >
            Open Local Network settings
          </button>
        </>
      }
    >
      <p>
        macOS answered <em>No route to host</em>. That usually means Termalith is not allowed to reach
        devices on your local network, even though Terminal is.
      </p>
      <ol className="steps">
        <li>
          Click <strong>Open Local Network settings</strong> (System Settings → Privacy &amp; Security →
          Local Network).
        </li>
        <li>
          Turn <strong>Termalith</strong> on. If it is already on, turn it off and on again.
        </li>
        <li>
          Click <strong>Restart Termalith</strong> and connect again.
        </li>
      </ol>
      <p className="hint">
        Each new Termalith version counts as a new app to macOS until releases are signed, so this can
        come back after an update. If it still fails after a restart, restart the Mac once. If the
        device is switched off or really unreachable, you see the same error.
      </p>
      {failed && (
        <p className="form-error" role="alert">
          {failed}
        </p>
      )}
    </Dialog>
  );
}
