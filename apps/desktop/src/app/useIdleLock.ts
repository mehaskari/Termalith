import { useEffect, useRef } from "react";

/** Whether `minutes` without use have passed since `last` (ms); 0 minutes = never. */
export function idleExpired(last: number, now: number, minutes: number): boolean {
  return minutes > 0 && now - last >= minutes * 60_000;
}

/**
 * Locks saved passwords after `minutes` without keyboard or mouse use in
 * this window (master password, setting "Lock saved passwords after").
 * Calls `onLock` once per idle period; any use starts a new one.
 */
export function useIdleLock(minutes: number, enabled: boolean, onLock: () => void) {
  const last = useRef(Date.now());
  const fired = useRef(false);
  const lock = useRef(onLock);
  lock.current = onLock;

  useEffect(() => {
    const used = () => {
      last.current = Date.now();
      fired.current = false;
    };
    const events = ["keydown", "pointerdown", "wheel"] as const;
    for (const e of events) window.addEventListener(e, used, { capture: true, passive: true });
    return () => {
      for (const e of events) window.removeEventListener(e, used, { capture: true });
    };
  }, []);

  useEffect(() => {
    if (!enabled || minutes <= 0) return;
    // A new limit counts from now, not from the last key press.
    last.current = Date.now();
    fired.current = false;
    const timer = window.setInterval(() => {
      if (!fired.current && idleExpired(last.current, Date.now(), minutes)) {
        fired.current = true;
        lock.current();
      }
    }, 15_000);
    return () => window.clearInterval(timer);
  }, [enabled, minutes]);
}
