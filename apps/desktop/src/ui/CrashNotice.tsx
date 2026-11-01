import { useState } from "react";
import { type CrashReport, openWebLink } from "../terminal/api";
import { Dialog } from "./Dialog";
import { REPO } from "./project";

/** Issue page with a title only; the report is pasted by the user after reviewing it. */
export function issueUrl(report: Pick<CrashReport, "summary">): string {
  const title = `Crash: ${report.summary}`.slice(0, 120);
  const body = "What were you doing when Termalith closed?\n\nCrash report (please check it contains nothing private):\n\n```\n(paste here)\n```\n";
  return `${REPO}/issues/new?title=${encodeURIComponent(title)}&body=${encodeURIComponent(body)}`;
}

/** Shown at startup when earlier runs left crash reports. */
export function CrashNotice({ reports, onDismiss }: { reports: CrashReport[]; onDismiss: () => void }) {
  const [copied, setCopied] = useState(false);
  const text = reports.map((r) => r.text).join("\n----\n");
  return (
    <Dialog
      title="Termalith closed unexpectedly"
      tone="warning"
      onCancel={onDismiss}
      actions={
        <>
          <button
            type="button"
            onClick={() =>
              void navigator.clipboard.writeText(text).then(
                () => setCopied(true),
                () => {},
              )
            }
          >
            {copied ? "Copied" : "Copy report"}
          </button>
          <button type="button" onClick={() => void openWebLink(issueUrl(reports[0] ?? { summary: "unknown" })).catch(() => {})}>
            Report on GitHub
          </button>
          <button type="button" className="primary" onClick={onDismiss}>
            Dismiss
          </button>
        </>
      }
    >
      <div className="crash-notice">
        <p>
          {reports.length === 1 ? "A crash report was" : `${reports.length} crash reports were`} saved on this
          computer. Nothing has been sent. Reporting it helps fix the problem; please check the report contains
          nothing private before you share it.
        </p>
        <textarea readOnly value={text} rows={10} aria-label="Crash report" spellCheck={false} />
      </div>
    </Dialog>
  );
}
