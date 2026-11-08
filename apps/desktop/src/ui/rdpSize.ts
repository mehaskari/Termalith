/**
 * Above about 2560×1600 device pixels the scale is lowered, so full-screen
 * updates stay manageable over IPC.
 */
const MAX_PIXELS = 2560 * 1600;

export interface DesktopSize {
  width: number;
  height: number;
  /** Display scale in percent for the remote computer (100-500). */
  scale: number;
}

const even = (n: number) => Math.max(2, Math.round(n / 2) * 2);

/**
 * The remote desktop for a stage of `width`×`height` CSS pixels: device
 * pixels on a high-density screen, with the matching scale so text keeps
 * its size and is sharp.
 */
export function desktopSize(width: number, height: number, devicePixelRatio: number): DesktopSize {
  const cap = Math.sqrt(MAX_PIXELS / Math.max(1, width * height));
  const ratio = Math.max(1, Math.min(devicePixelRatio || 1, 5, cap));
  return {
    width: Math.min(8192, even(width * ratio)),
    height: Math.min(8192, even(height * ratio)),
    scale: Math.round(ratio * 100),
  };
}

/** The same scale for a new stage size (Windows changes the scale only at login). */
export function resizedDesktop(width: number, height: number, scale: number): { width: number; height: number } {
  const ratio = scale / 100;
  return { width: Math.min(8192, even(width * ratio)), height: Math.min(8192, even(height * ratio)) };
}
