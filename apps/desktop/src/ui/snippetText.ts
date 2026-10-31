// Placeholder handling for snippets: `{{name}}` is replaced by a value the
// user types before sending. Pure, for testing.

const PLACEHOLDER = /\{\{\s*([A-Za-z_][\w-]*)\s*\}\}/g;

/** Distinct variable names, in order of first appearance. */
export function snippetVariables(command: string): string[] {
  const seen: string[] = [];
  for (const m of command.matchAll(PLACEHOLDER)) {
    const name = m[1];
    if (name && !seen.includes(name)) seen.push(name);
  }
  return seen;
}

export function fillSnippet(command: string, values: Record<string, string>): string {
  return command.replace(PLACEHOLDER, (whole, name: string) => values[name] ?? whole);
}

/**
 * Text to type into the terminal: newlines become carriage returns (what
 * the Enter key sends). A trailing Enter is added only when asked.
 */
export function terminalInput(text: string, pressEnter: boolean): string {
  const body = text.replace(/\r?\n/g, "\r").replace(/\r+$/, "");
  return pressEnter ? `${body}\r` : body;
}
