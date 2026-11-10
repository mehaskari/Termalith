import type { EmbeddedEvent } from "../terminal/api";

/** Largest cursor image browsers show (Chromium ignores bigger ones). */
const MAX_CURSOR = 128;

/**
 * The CSS cursor for the remote pointer's shape. The image is in desktop
 * pixels; it is scaled like the desktop, so it matches what is drawn.
 */
export function cssCursor(e: Extract<EmbeddedEvent, { kind: "cursor" }>, canvas: HTMLCanvasElement): string {
  if (e.hidden) return "none";
  const img = e.image;
  if (!img || img.rgba.length !== img.width * img.height * 4) return "default";
  const r = canvas.getBoundingClientRect();
  const fit = canvas.width && canvas.height ? Math.min(r.width / canvas.width, r.height / canvas.height) : 1;
  const scale = Math.min(fit || 1, MAX_CURSOR / img.width, MAX_CURSOR / img.height);
  const w = Math.max(1, Math.round(img.width * scale));
  const h = Math.max(1, Math.round(img.height * scale));
  const src = document.createElement("canvas");
  src.width = img.width;
  src.height = img.height;
  const sctx = src.getContext("2d");
  if (!sctx) return "default";
  sctx.putImageData(new ImageData(new Uint8ClampedArray(img.rgba), img.width, img.height), 0, 0);
  const out = document.createElement("canvas");
  out.width = w;
  out.height = h;
  const octx = out.getContext("2d");
  if (!octx) return "default";
  octx.drawImage(src, 0, 0, w, h);
  const x = Math.min(w - 1, Math.round(img.hotX * scale));
  const y = Math.min(h - 1, Math.round(img.hotY * scale));
  return `url(${out.toDataURL("image/png")}) ${x} ${y}, default`;
}
