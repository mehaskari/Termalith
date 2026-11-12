// Reasons to confirm a paste before it reaches the shell. Pure, for tests.

const HIDDEN = /[​-‏‪-‮⁠-⁤⁦-⁩﻿]/;
// Control characters other than tab and line breaks (escape sequences).
const CONTROL = /[\x00-\x08\x0B\x0C\x0E-\x1F\x7F]/;
const PIPE_TO_SHELL = /\b(curl|wget|fetch|iwr|invoke-webrequest)\b[^\n|]*\|\s*(sudo\s+)?(ba|z|da|k|fi)?sh\b/i;

/** Why `text` deserves a look before pasting; empty when it is plain. */
export function pasteWarnings(text: string): string[] {
  const reasons: string[] = [];
  const normalized = text.replace(/\r\n?/g, "\n");
  const breaks = (normalized.match(/\n/g) ?? []).length;
  if (breaks > 0) {
    const lines = normalized.replace(/\n$/, "").split("\n").length;
    reasons.push(
      lines > 1
        ? `${lines} lines: each one runs as soon as it is pasted`
        : "it ends with a line break, so it runs at once",
    );
  }
  if (PIPE_TO_SHELL.test(normalized)) reasons.push("it downloads a script and runs it");
  if (HIDDEN.test(text)) reasons.push("it contains invisible characters that can hide what runs");
  if (CONTROL.test(text)) reasons.push("it contains control characters (escape sequences)");
  return reasons;
}

/** The text with invisible and control characters shown as ⟨U+XXXX⟩. */
export function revealHidden(text: string): string {
  return text.replace(
    new RegExp(`${HIDDEN.source}|${CONTROL.source}`, "g"),
    (c) => `⟨U+${c.codePointAt(0)!.toString(16).toUpperCase().padStart(4, "0")}⟩`,
  );
}
