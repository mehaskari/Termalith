import type { IDecoration, IDisposable, Terminal } from "@xterm/xterm";

export type KeywordKind = "bad" | "warn" | "good";

export interface KeywordHit {
  start: number;
  length: number;
  kind: KeywordKind;
}

const BUILT_IN: Record<KeywordKind, string[]> = {
  bad: ["error", "errors", "fail", "failed", "failure", "fatal", "critical", "panic", "denied", "refused"],
  warn: ["warn", "warning", "warnings"],
  good: ["ok", "success", "successful", "succeeded", "passed"],
};

/** The user's extra words per color (Settings → Keyword colors). */
export type ExtraKeywords = Partial<Record<KeywordKind, string[]>>;

const escape = (w: string) => w.replace(/[.*+?^${}()|[\]\\]/g, "\\$&");

/**
 * One case-insensitive pattern for the built-in and extra words. A word
 * matches whole: not inside a longer word or a hyphenated name, so "up"
 * does not color "update" and "down" does not color "shutdown-x".
 */
export function keywordPattern(extra: ExtraKeywords = {}): RegExp {
  const group = (kind: KeywordKind) =>
    [...new Set([...(extra[kind] ?? []).map((w) => w.trim()).filter(Boolean), ...BUILT_IN[kind]])]
      .sort((a, b) => b.length - a.length)
      .map(escape)
      .join("|");
  return new RegExp(`(?<![\\w-])(?:(${group("bad")})|(${group("warn")})|(${group("good")}))(?![\\w-])`, "gi");
}

const DEFAULT_PATTERN = keywordPattern();

/** Keywords in one line of output. */
export function findKeywords(text: string, pattern: RegExp = DEFAULT_PATTERN): KeywordHit[] {
  const hits: KeywordHit[] = [];
  for (const m of text.matchAll(pattern)) {
    hits.push({ start: m.index, length: m[0].length, kind: m[1] ? "bad" : m[2] ? "warn" : "good" });
  }
  return hits;
}

/** After output stops for this long, new lines are scanned. */
const SCAN_DELAY_MS = 120;
/** A flood of output: only the end of it is looked at. */
const MAX_LINES_PER_SCAN = 500;
/** Oldest decorations are dropped beyond this. */
const MAX_DECORATIONS = 5000;

/**
 * Colors keywords with decorations on top of the terminal, so the data itself
 * stays as it arrived: copying, logs and saved output are unaffected. Lines
 * are scanned once complete; full-screen programs are left alone.
 */
export class OutputHighlighter implements IDisposable {
  private enabled = false;
  private scanned = 0;
  private timer: ReturnType<typeof setTimeout> | undefined;
  private readonly decorations: IDecoration[] = [];
  private pattern = DEFAULT_PATTERN;
  private words = "";

  constructor(private readonly term: Terminal) {}

  setEnabled(on: boolean) {
    if (on === this.enabled) return;
    this.enabled = on;
    if (on) {
      this.scanned = this.term.buffer.normal.baseY + this.term.buffer.normal.cursorY;
    } else {
      clearTimeout(this.timer);
      this.clear();
    }
  }

  /** The user's extra words; takes effect for new output. */
  setWords(extra: ExtraKeywords) {
    const key = JSON.stringify(extra);
    if (key === this.words) return;
    this.words = key;
    this.pattern = keywordPattern(extra);
  }

  /** Call after output was written. */
  schedule() {
    if (!this.enabled) return;
    clearTimeout(this.timer);
    this.timer = setTimeout(() => this.scan(), SCAN_DELAY_MS);
  }

  dispose() {
    clearTimeout(this.timer);
    this.clear();
  }

  private clear() {
    for (const d of this.decorations.splice(0)) d.dispose();
  }

  private color(kind: KeywordKind): string {
    const theme = this.term.options.theme;
    if (kind === "bad") return theme?.red ?? "#e06c75";
    if (kind === "warn") return theme?.yellow ?? "#e5c07b";
    return theme?.green ?? "#98c379";
  }

  private scan() {
    const buffer = this.term.buffer.active;
    if (buffer.type !== "normal") return;
    const cursor = buffer.baseY + buffer.cursorY;
    const from = Math.max(Math.min(this.scanned, cursor), cursor - MAX_LINES_PER_SCAN);
    // The cursor's line may still grow.
    for (let y = from; y < cursor; y++) {
      const line = buffer.getLine(y);
      if (!line) continue;
      const hits = findKeywords(line.translateToString(true), this.pattern);
      if (hits.length === 0) continue;
      const columns = columnMap(line, this.term.cols);
      for (const hit of hits) this.decorate(y - cursor, columns, hit);
    }
    this.scanned = cursor;
    const excess = this.decorations.length - MAX_DECORATIONS;
    if (excess > 0) for (const d of this.decorations.splice(0, excess)) d.dispose();
  }

  private decorate(offset: number, columns: number[], hit: KeywordHit) {
    const x = columns[hit.start];
    const end = columns[hit.start + hit.length - 1];
    const marker = this.term.registerMarker(offset);
    if (x === undefined || end === undefined || !marker) {
      marker?.dispose();
      return;
    }
    const decoration = this.term.registerDecoration({
      marker,
      x,
      width: end - x + 1,
      foregroundColor: this.color(hit.kind),
    });
    if (decoration) this.decorations.push(decoration);
    else marker.dispose();
  }
}

/** Column of each character of a line: wide characters take two cells. */
function columnMap(line: { getCell(x: number): { getChars(): string; getWidth(): number } | undefined }, cols: number): number[] {
  const columns: number[] = [];
  for (let x = 0; x < cols; x++) {
    const cell = line.getCell(x);
    if (!cell || cell.getWidth() === 0) continue;
    const length = cell.getChars().length || 1;
    for (let i = 0; i < length; i++) columns.push(x);
  }
  return columns;
}
