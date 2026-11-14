import { type UpdateInfo, openWebLink } from "../terminal/api";

const DISMISSED_KEY = "termalith.updateDismissed";

/** The version the user chose "Later" for; that version is not shown again. */
export function dismissedVersion(): string | null {
  try {
    return localStorage.getItem(DISMISSED_KEY);
  } catch {
    return null;
  }
}

function dismiss(version: string) {
  try {
    localStorage.setItem(DISMISSED_KEY, version);
  } catch {
    // Without storage the notice simply comes back next start.
  }
}

/** A small corner notice for a newer release: Download or Later, nothing else. */
export function UpdateToast({ info, onClose }: { info: UpdateInfo; onClose: () => void }) {
  if (!info.latest) return null;
  const latest = info.latest;
  return (
    <div className="update-toast" role="status" aria-label="Update available">
      <span>
        Termalith <strong>{latest}</strong> is available.
      </span>
      {info.url && (
        <button
          type="button"
          className="primary"
          onClick={() => {
            void openWebLink(info.url ?? "").catch(() => {});
            onClose();
          }}
        >
          Download
        </button>
      )}
      <button
        type="button"
        onClick={() => {
          dismiss(latest);
          onClose();
        }}
      >
        Later
      </button>
    </div>
  );
}
