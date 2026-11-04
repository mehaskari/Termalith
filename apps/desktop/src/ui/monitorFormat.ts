const UNITS = ["B", "KB", "MB", "GB", "TB", "PB"];

/** Short size for the monitor bar: 1 decimal from GB up, none below. */
export function formatBytes(bytes: number): string {
  let value = bytes;
  let unit = 0;
  while (value >= 1024 && unit < UNITS.length - 1) {
    value /= 1024;
    unit += 1;
  }
  return `${unit >= 3 ? value.toFixed(1) : Math.round(value)} ${UNITS[unit]}`;
}
