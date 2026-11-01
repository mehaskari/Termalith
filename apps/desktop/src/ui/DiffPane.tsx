import { useEffect, useState } from "react";
import { type TerminalId, isIpcError, readRemoteText } from "../terminal/api";
import { type SideBySideRow, diffLines, sideBySide, splitLines } from "./diff";

export interface DiffTarget {
  terminalId: TerminalId;
  dir: string;
  name: string;
}

/** Two remote text files side by side, differences highlighted. */
export function DiffPane({ left, right }: { left: DiffTarget; right: DiffTarget }) {
  const [rows, setRows] = useState<SideBySideRow[] | null>(null);
  const [error, setError] = useState<string | null>(null);

  useEffect(() => {
    let live = true;
    Promise.all([
      readRemoteText(left.terminalId, left.dir, left.name),
      readRemoteText(right.terminalId, right.dir, right.name),
    ]).then(
      ([a, b]) => {
        if (!live) return;
        const ops = diffLines(splitLines(a.content), splitLines(b.content));
        if (!ops) setError("The files are too different to compare line by line.");
        else setRows(sideBySide(ops));
      },
      (e) => live && setError(isIpcError(e) ? e.message : String(e)),
    );
    return () => {
      live = false;
    };
  }, [left, right]);

  const changes = rows?.filter((r) => r.kind !== "same").length ?? 0;
  return (
    <div className="diff-pane">
      <div className="diff-head">
        <code title={`${left.dir}/${left.name}`}>{left.name}</code>
        <code title={`${right.dir}/${right.name}`}>{right.name}</code>
      </div>
      {error && (
        <p className="form-error diff-msg" role="alert">
          {error}
        </p>
      )}
      {!rows && !error && <p className="diff-msg">Loading…</p>}
      {rows && changes === 0 && <p className="diff-msg">The files are identical.</p>}
      {rows && (
        <div className="diff-body" role="table" aria-label="Differences">
          {rows.map((r, i) => (
            <div key={i} className={`diff-row ${r.kind}`} role="row">
              <span className="diff-no">{r.left?.no ?? ""}</span>
              <span className={`diff-text ${r.kind === "changed" || r.kind === "del" ? "removed" : ""}`}>
                {r.left?.text ?? ""}
              </span>
              <span className="diff-no">{r.right?.no ?? ""}</span>
              <span className={`diff-text ${r.kind === "changed" || r.kind === "add" ? "added" : ""}`}>
                {r.right?.text ?? ""}
              </span>
            </div>
          ))}
        </div>
      )}
      {rows && changes > 0 && <p className="diff-msg">{changes} changed line(s)</p>}
    </div>
  );
}
