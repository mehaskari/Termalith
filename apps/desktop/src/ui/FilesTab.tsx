import type { ComponentProps } from "react";
import type { Tab } from "../app/tabs";
import { FilesPanel } from "./FilesPanel";

/** The connection name of a files tab ("web-01 (files)" → "web-01"). */
export function filesTabConnection(title: string): string {
  return title.replace(/ \(files\)$/, "");
}

/**
 * A tab that is only a file browser (SFTP, FTP or S3): "Connecting…" or the
 * login error until the connection is up, then the Files panel. SFTP tabs
 * can open a terminal on the same connection.
 */
export function FilesTab(props: {
  tab: Tab;
  onOpenTerminal: () => void;
  panel: Omit<ComponentProps<typeof FilesPanel>, "terminalId" | "connection">;
}) {
  const { tab: t } = props;
  if (t.terminalId === undefined) {
    return (
      <div className="files-tab">
        <div className="desktop-pane">
          <div className="desktop-card">
            {t.desktopError ? (
              <p className="form-error" role="alert">
                {t.desktopError}
              </p>
            ) : (
              <p>Connecting…</p>
            )}
          </div>
        </div>
      </div>
    );
  }
  return (
    <div className="files-tab">
      <div className="pane-toolbar">
        <span />
        <span className="pane-actions">
          {!t.noShell && (
            <button
              type="button"
              title="Open a terminal on this connection (no second login)"
              onClick={props.onOpenTerminal}
            >
              Open terminal
            </button>
          )}
        </span>
      </div>
      <FilesPanel {...props.panel} terminalId={t.terminalId} connection={filesTabConnection(t.title)} />
    </div>
  );
}
