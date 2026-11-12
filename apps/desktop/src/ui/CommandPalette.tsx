import { type KeyboardEvent, useMemo, useState } from "react";

export interface PaletteItem {
  id: string;
  label: string;
  /** Smaller text after the label (target, kind). */
  detail?: string;
  group: "Tabs" | "Sessions" | "Actions";
  run: () => void;
}

interface Props {
  items: PaletteItem[];
  /** Items made from what was typed (e.g. "Connect: ssh me@host"), listed first. */
  typed?: (query: string) => PaletteItem[];
  onClose: () => void;
}

/** Case-insensitive: every word must appear in the label or detail. */
export function filterPalette(items: PaletteItem[], query: string): PaletteItem[] {
  const words = query.toLowerCase().split(/\s+/).filter(Boolean);
  if (!words.length) return items;
  return items.filter((i) => {
    const text = `${i.label} ${i.detail ?? ""} ${i.group}`.toLowerCase();
    return words.every((w) => text.includes(w));
  });
}

/** Quick search over tabs, sessions and actions (Cmd/Ctrl+K). */
export function CommandPalette({ items, typed, onClose }: Props) {
  const [query, setQuery] = useState("");
  const [index, setIndex] = useState(0);
  const shown = useMemo(
    () => [...(typed?.(query) ?? []), ...filterPalette(items, query)].slice(0, 50),
    [items, query, typed],
  );
  const current = Math.min(index, Math.max(0, shown.length - 1));

  function run(item: PaletteItem | undefined) {
    if (!item) return;
    onClose();
    item.run();
  }

  function onKeyDown(e: KeyboardEvent<HTMLInputElement>) {
    if (e.key === "ArrowDown") {
      e.preventDefault();
      setIndex((current + 1) % Math.max(1, shown.length));
    } else if (e.key === "ArrowUp") {
      e.preventDefault();
      setIndex((current - 1 + shown.length) % Math.max(1, shown.length));
    } else if (e.key === "Enter") {
      e.preventDefault();
      run(shown[current]);
    } else if (e.key === "Escape") {
      e.preventDefault();
      onClose();
    }
  }

  return (
    <div className="palette-backdrop" onMouseDown={(e) => e.target === e.currentTarget && onClose()}>
      <div className="palette" role="dialog" aria-label="Command palette">
        <input
          autoFocus
          value={query}
          placeholder="Search tabs, sessions and actions, or type ssh user@host…"
          spellCheck={false}
          aria-label="Search"
          aria-controls="palette-list"
          onChange={(e) => {
            setQuery(e.target.value);
            setIndex(0);
          }}
          onKeyDown={onKeyDown}
        />
        <ul id="palette-list" role="listbox">
          {shown.length === 0 && <li className="palette-empty">No matches</li>}
          {shown.map((item, i) => (
            <li
              key={item.id}
              role="option"
              aria-selected={i === current}
              className={i === current ? "selected" : undefined}
              onMouseEnter={() => setIndex(i)}
              onMouseDown={(e) => {
                e.preventDefault();
                run(item);
              }}
            >
              <span className="palette-group">{item.group}</span>
              <span className="palette-label">{item.label}</span>
              {item.detail && <span className="palette-detail">{item.detail}</span>}
            </li>
          ))}
        </ul>
      </div>
    </div>
  );
}
