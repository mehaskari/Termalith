// Remote (POSIX-style SFTP) path helpers. Pure, for testing.

export function joinRemote(dir: string, name: string): string {
  return dir.endsWith("/") ? `${dir}${name}` : `${dir}/${name}`;
}

/** Parent directory; the root is its own parent. */
export function parentRemote(path: string): string {
  const trimmed = path.length > 1 ? path.replace(/\/+$/, "") : path;
  const i = trimmed.lastIndexOf("/");
  if (i <= 0) {
    // "/a" -> "/", "C:" style roots from Windows servers stay put.
    return i === 0 ? "/" : trimmed;
  }
  // Windows OpenSSH exposes drives as "/C:"; keep "/C:/" as a root.
  const parent = trimmed.slice(0, i);
  return /^\/[A-Za-z]:$/.test(parent) ? `${parent}/` : parent;
}

export function formatSize(bytes: number | null): string {
  if (bytes === null) return "";
  if (bytes < 1024) return `${bytes} B`;
  const units = ["KB", "MB", "GB", "TB"];
  let v = bytes / 1024;
  let u = 0;
  while (v >= 1024 && u < units.length - 1) {
    v /= 1024;
    u++;
  }
  return `${v < 10 ? v.toFixed(1) : Math.round(v)} ${units[u]}`;
}

export function percent(transferred: number, total: number | null): number | null {
  if (!total) return null;
  return Math.min(100, Math.round((transferred / total) * 100));
}
