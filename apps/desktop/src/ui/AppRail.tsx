import { useEffect, useRef, useState } from "react";
import type { LucideIcon } from "lucide-react";
import { ArrowLeftRight, House, Info, KeyRound, Laptop, Plug, Settings, Usb, Wrench } from "lucide-react";
import type { ShellChoice } from "../terminal/api";

interface RailButtonProps {
  icon: LucideIcon;
  label: string;
  title: string;
  active?: boolean;
  onClick: () => void;
  onContextMenu?: (e: React.MouseEvent) => void;
  expanded?: boolean;
}

function RailButton({ icon: Glyph, label, title, active, onClick, onContextMenu, expanded }: RailButtonProps) {
  return (
    <button
      type="button"
      className={`rail-button ${active ? "active" : ""}`}
      title={title}
      aria-label={title}
      aria-current={active ? "page" : undefined}
      aria-expanded={expanded}
      onClick={onClick}
      onContextMenu={onContextMenu}
    >
      <Glyph size={19} strokeWidth={1.7} aria-hidden="true" />
      <span className="rail-label" aria-hidden="true">
        {label}
      </span>
    </button>
  );
}

/**
 * The narrow bar on the far left: where to go and what to open, always one
 * click away. Settings and About sit at the bottom.
 */
export function AppRail(p: {
  homeActive: boolean;
  toolsActive: boolean;
  shells: ShellChoice[];
  onHome: () => void;
  onQuickConnect: () => void;
  onLocalTerminal: (shellId?: string) => void;
  onSerial: () => void;
  onTools: () => void;
  onKeys: () => void;
  onImport: () => void;
  onImportSessions: () => void;
  onExport: () => void;
  onSettings: () => void;
  onAbout: () => void;
}) {
  const [menu, setMenu] = useState<"shells" | "transfer" | null>(null);
  const ref = useRef<HTMLElement>(null);
  useEffect(() => {
    if (!menu) return;
    const close = (e: MouseEvent) => {
      if (!ref.current?.contains(e.target as Node)) setMenu(null);
    };
    const esc = (e: KeyboardEvent) => e.key === "Escape" && setMenu(null);
    window.addEventListener("mousedown", close);
    window.addEventListener("keydown", esc);
    return () => {
      window.removeEventListener("mousedown", close);
      window.removeEventListener("keydown", esc);
    };
  }, [menu]);
  const run = (f: () => void) => () => {
    setMenu(null);
    f();
  };
  return (
    <nav className="rail" aria-label="Main" ref={ref}>
      <RailButton icon={House} label="Home" title="Home" active={p.homeActive} onClick={p.onHome} />
      <RailButton icon={Plug} label="Connect" title="Quick connect" onClick={p.onQuickConnect} />
      <RailButton
        icon={Laptop}
        label="Local"
        title={p.shells.length > 1 ? "Local terminal (right-click to choose a shell)" : "Local terminal"}
        onClick={() => p.onLocalTerminal()}
        expanded={p.shells.length > 1 ? menu === "shells" : undefined}
        onContextMenu={(e) => {
          if (p.shells.length < 2) return;
          e.preventDefault();
          setMenu("shells");
        }}
      />
      <RailButton icon={Usb} label="Serial" title="Serial console" onClick={p.onSerial} />
      <RailButton icon={Wrench} label="Tools" title="Network tools and file servers" active={p.toolsActive} onClick={p.onTools} />
      <RailButton icon={KeyRound} label="Keys" title="SSH keys" onClick={p.onKeys} />
      <RailButton
        icon={ArrowLeftRight}
        label="Import"
        title="Import and export sessions"
        expanded={menu === "transfer"}
        onClick={() => setMenu((m) => (m === "transfer" ? null : "transfer"))}
      />
      <span className="rail-spacer" />
      <RailButton icon={Settings} label="Settings" title="Settings" onClick={p.onSettings} />
      <RailButton icon={Info} label="About" title="About Termalith" onClick={p.onAbout} />
      {menu === "shells" && (
        <div className="menu rail-menu" role="menu" style={{ top: 132 }}>
          {p.shells.map((s) => (
            <button key={s.id} type="button" role="menuitem" title={s.id} onClick={run(() => p.onLocalTerminal(s.id))}>
              {s.name}
            </button>
          ))}
        </div>
      )}
      {menu === "transfer" && (
        <div className="menu rail-menu" role="menu" style={{ top: 330 }}>
          <button type="button" role="menuitem" onClick={run(p.onImport)}>
            Import ~/.ssh/config…
          </button>
          <button type="button" role="menuitem" onClick={run(p.onImportSessions)}>
            Import sessions (JSON, MobaXterm, PuTTY, FileZilla)…
          </button>
          <button type="button" role="menuitem" onClick={run(p.onExport)}>
            Export sessions…
          </button>
        </div>
      )}
    </nav>
  );
}
