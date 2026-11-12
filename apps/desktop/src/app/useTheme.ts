import { useEffect, useState } from "react";
import type { Settings } from "../terminal/api";
import { resolveTheme } from "../ui/theme";

/** The theme in use ("light"/"dark"), following the system when set to it; applied to the document. */
export function useTheme(choice: Settings["theme"]) {
  const [systemDark, setSystemDark] = useState(
    () => typeof window.matchMedia !== "function" || window.matchMedia("(prefers-color-scheme: dark)").matches,
  );
  const theme = resolveTheme(choice, systemDark);

  useEffect(() => {
    document.documentElement.dataset.theme = theme;
  }, [theme]);

  useEffect(() => {
    if (typeof window.matchMedia !== "function") return;
    const mq = window.matchMedia("(prefers-color-scheme: dark)");
    const onChange = (e: MediaQueryListEvent) => setSystemDark(e.matches);
    mq.addEventListener("change", onChange);
    return () => mq.removeEventListener("change", onChange);
  }, []);

  return theme;
}
