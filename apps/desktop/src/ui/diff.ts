// Line diff (Myers, O((N+M)·D)) for comparing two text files. Pure, so it
// can be unit-tested.

export type DiffOp = { kind: "same" | "del" | "add"; text: string };

/** Above this many differing lines the diff gives up (very different files). */
export const MAX_EDIT_DISTANCE = 4000;

export function splitLines(text: string): string[] {
  const lines = text.replace(/\r\n/g, "\n").split("\n");
  if (lines.length > 1 && lines[lines.length - 1] === "") lines.pop();
  return lines;
}

/**
 * The shortest edit script turning `a` into `b`, as a sequence of
 * unchanged, deleted and added lines. `null` when the files differ in more
 * than [`MAX_EDIT_DISTANCE`] lines.
 */
export function diffLines(a: string[], b: string[]): DiffOp[] | null {
  // Strip a common prefix/suffix first: typical edits are small.
  let start = 0;
  while (start < a.length && start < b.length && a[start] === b[start]) start++;
  let endA = a.length;
  let endB = b.length;
  while (endA > start && endB > start && a[endA - 1] === b[endB - 1]) {
    endA--;
    endB--;
  }
  const A = a.slice(start, endA);
  const B = b.slice(start, endB);
  const n = A.length;
  const m = B.length;
  const max = n + m;
  const trace: Int32Array[] = [];
  const offset = max + 1;
  let v = new Int32Array(2 * max + 3);
  let found = n === 0 && m === 0;
  for (let d = 0; d <= max && !found; d++) {
    if (d > MAX_EDIT_DISTANCE) return null;
    trace.push(v.slice());
    for (let k = -d; k <= d; k += 2) {
      let x =
        k === -d || (k !== d && (v[offset + k - 1] ?? 0) < (v[offset + k + 1] ?? 0))
          ? (v[offset + k + 1] ?? 0)
          : (v[offset + k - 1] ?? 0) + 1;
      let y = x - k;
      while (x < n && y < m && A[x] === B[y]) {
        x++;
        y++;
      }
      v[offset + k] = x;
      if (x >= n && y >= m) {
        found = true;
        trace.push(v.slice());
        break;
      }
    }
    if (found) break;
  }
  // Walk back through the trace to build the script.
  const middle: DiffOp[] = [];
  let x = n;
  let y = m;
  for (let d = trace.length - 2; d >= 0 && (x > 0 || y > 0); d--) {
    const vd = trace[d] as Int32Array;
    const k = x - y;
    const prevK =
      k === -d || (k !== d && (vd[offset + k - 1] ?? 0) < (vd[offset + k + 1] ?? 0)) ? k + 1 : k - 1;
    const prevX = vd[offset + prevK] ?? 0;
    const prevY = prevX - prevK;
    while (x > prevX && y > prevY) {
      middle.push({ kind: "same", text: A[--x] as string });
      y--;
    }
    if (d > 0) {
      if (x === prevX) middle.push({ kind: "add", text: B[--y] as string });
      else middle.push({ kind: "del", text: A[--x] as string });
    }
  }
  while (x > 0 && y > 0) {
    middle.push({ kind: "same", text: A[--x] as string });
    y--;
  }
  middle.reverse();
  return [
    ...a.slice(0, start).map((text): DiffOp => ({ kind: "same", text })),
    ...middle,
    ...a.slice(endA).map((text): DiffOp => ({ kind: "same", text })),
  ];
}

export interface SideBySideRow {
  left: { no: number; text: string } | null;
  right: { no: number; text: string } | null;
  kind: "same" | "changed" | "del" | "add";
}

/** Pairs deletions with additions so changed lines sit side by side. */
export function sideBySide(ops: DiffOp[]): SideBySideRow[] {
  const rows: SideBySideRow[] = [];
  let l = 0;
  let r = 0;
  for (let i = 0; i < ops.length; ) {
    const op = ops[i] as DiffOp;
    if (op.kind === "same") {
      rows.push({ left: { no: ++l, text: op.text }, right: { no: ++r, text: op.text }, kind: "same" });
      i++;
      continue;
    }
    const dels: string[] = [];
    const adds: string[] = [];
    while (i < ops.length && (ops[i] as DiffOp).kind !== "same") {
      const o = ops[i] as DiffOp;
      (o.kind === "del" ? dels : adds).push(o.text);
      i++;
    }
    for (let j = 0; j < Math.max(dels.length, adds.length); j++) {
      const d = dels[j];
      const a = adds[j];
      rows.push({
        left: d !== undefined ? { no: ++l, text: d } : null,
        right: a !== undefined ? { no: ++r, text: a } : null,
        kind: d !== undefined && a !== undefined ? "changed" : d !== undefined ? "del" : "add",
      });
    }
  }
  return rows;
}
