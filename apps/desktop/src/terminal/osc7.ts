const MAX_PATH = 4096;

/**
 * Parses an OSC 7 report (`file://host/path`) from a shell. Terminal output
 * is untrusted: anything but a plain absolute path is ignored, and the result
 * only ever selects a folder to list.
 */
export function parseOsc7(data: string): { host: string; path: string } | null {
  const [, host, raw] = /^file:\/\/([^/]*)(\/.*)$/.exec(data) ?? [];
  if (host === undefined || raw === undefined || raw.length > MAX_PATH) return null;
  let path = raw;
  try {
    path = decodeURIComponent(path);
  } catch {
    // Shells that report $PWD as it is: a stray % is part of the name.
  }
  if (/[\x00-\x1f\x7f]/.test(path)) return null;
  return { host, path };
}

/**
 * Makes bash or zsh report its folder until the shell exits. Typed into the
 * terminal, visibly and only after the user agrees; nothing on the server is
 * changed. The leading space keeps it out of most shell histories.
 */
export const FOLDER_REPORT_SETUP =
  ` if [ -n "$ZSH_VERSION" ]; then __termalith_cwd() { printf '\\033]7;file://%s%s\\033\\\\' "$HOST" "$PWD"; }; ` +
  `precmd_functions+=(__termalith_cwd); __termalith_cwd; ` +
  `elif [ -n "$BASH_VERSION" ]; then __termalith_cwd() { printf '\\033]7;file://%s%s\\033\\\\' "$HOSTNAME" "$PWD"; }; ` +
  `PROMPT_COMMAND="__termalith_cwd\${PROMPT_COMMAND:+;$PROMPT_COMMAND}"; __termalith_cwd; fi`;
