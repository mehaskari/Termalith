import type { ReactElement } from "react";
import type { SessionIcon } from "../terminal/api";

/** Pictures for sessions and folders (16px stroke grid), drawn for Termalith. */
const PICTURES: Record<string, { label: string; svg: ReactElement }> = {
  server: {
    label: "Server",
    svg: (
      <>
        <rect x="2.5" y="2.5" width="11" height="4.5" rx="1" />
        <rect x="2.5" y="9" width="11" height="4.5" rx="1" />
        <path d="M5 4.75h.01M5 11.25h.01" />
      </>
    ),
  },
  terminal: {
    label: "Terminal",
    svg: (
      <>
        <rect x="2" y="3" width="12" height="10" rx="1.5" />
        <path d="M4.5 6.5L6.5 8l-2 1.5M8 10h3" />
      </>
    ),
  },
  desktop: {
    label: "Desktop",
    svg: (
      <>
        <rect x="2" y="2.5" width="12" height="8" rx="1" />
        <path d="M6 13.5h4M8 10.5v3" />
      </>
    ),
  },
  laptop: {
    label: "Laptop",
    svg: (
      <>
        <rect x="3.5" y="3" width="9" height="6.5" rx="1" />
        <path d="M1.5 12.5h13l-1.5-3h-10z" />
      </>
    ),
  },
  window: {
    label: "Window",
    svg: (
      <>
        <rect x="2.5" y="2.5" width="11" height="11" rx="1" />
        <path d="M2.5 8h11M8 2.5v11" />
      </>
    ),
  },
  vm: {
    label: "Virtual machine",
    svg: (
      <>
        <rect x="2" y="3" width="12" height="9" rx="1" />
        <rect x="5" y="5.5" width="6" height="4" rx=".5" />
        <path d="M6 14h4" />
      </>
    ),
  },
  container: { label: "Container", svg: <path d="M8 2l5.5 3v6L8 14 2.5 11V5zM2.5 5L8 8l5.5-3M8 8v6" /> },
  router: {
    label: "Router",
    svg: (
      <>
        <rect x="2" y="8.5" width="12" height="5" rx="1" />
        <path d="M5 11h.01M7.5 11h.01M4.5 8.5l-.8-3M11.5 8.5l.8-3M6 4.2a3 3 0 014 0" />
      </>
    ),
  },
  switch: {
    label: "Switch",
    svg: (
      <>
        <rect x="1.5" y="5" width="13" height="6" rx="1" />
        <path d="M4 8h1M6.5 8h1M9 8h1M11.5 8h1" />
      </>
    ),
  },
  firewall: {
    label: "Firewall",
    svg: (
      <>
        <rect x="2" y="3" width="12" height="10" rx="1" />
        <path d="M2 6.3h12M2 9.7h12M6 3v3.3M10 6.3v3.4M6 9.7V13" />
      </>
    ),
  },
  wifi: { label: "Access point", svg: <path d="M2.5 6.5a8 8 0 0111 0M4.5 8.8a5 5 0 017 0M6.5 11a2 2 0 013 0M8 13.2h.01" /> },
  database: {
    label: "Database",
    svg: (
      <>
        <ellipse cx="8" cy="4" rx="5" ry="1.8" />
        <path d="M3 4v8c0 1 2.2 1.8 5 1.8s5-.8 5-1.8V4M3 8c0 1 2.2 1.8 5 1.8s5-.8 5-1.8" />
      </>
    ),
  },
  storage: {
    label: "Storage",
    svg: (
      <>
        <rect x="2" y="4" width="12" height="8" rx="1.5" />
        <path d="M2 9h12M11 10.5h.01" />
      </>
    ),
  },
  cloud: { label: "Cloud", svg: <path d="M4.5 12.5h7a3 3 0 00.4-6A4 4 0 004.3 7.6 2.5 2.5 0 004.5 12.5z" /> },
  web: {
    label: "Web",
    svg: (
      <>
        <circle cx="8" cy="8" r="5.5" />
        <path d="M2.5 8h11M8 2.5c1.7 1.6 2.5 3.4 2.5 5.5s-.8 3.9-2.5 5.5c-1.7-1.6-2.5-3.4-2.5-5.5S6.3 4.1 8 2.5z" />
      </>
    ),
  },
  mail: {
    label: "Mail",
    svg: (
      <>
        <rect x="2" y="3.5" width="12" height="9" rx="1" />
        <path d="M2 4.5l6 4.5 6-4.5" />
      </>
    ),
  },
  printer: {
    label: "Printer",
    svg: (
      <>
        <path d="M4.5 6V2.5h7V6M4.5 11.5h-2V6h11v5.5h-2" />
        <rect x="4.5" y="9.5" width="7" height="4" />
      </>
    ),
  },
  camera: {
    label: "Camera",
    svg: (
      <>
        <path d="M2 5.5h3l1-1.5h4l1 1.5h3v7H2z" />
        <circle cx="8" cy="9" r="2.2" />
      </>
    ),
  },
  phone: {
    label: "Phone",
    svg: (
      <>
        <rect x="4.5" y="1.5" width="7" height="13" rx="1.5" />
        <path d="M7 12.5h2" />
      </>
    ),
  },
  chip: {
    label: "Device",
    svg: (
      <>
        <rect x="4.5" y="4.5" width="7" height="7" rx="1" />
        <path d="M6.5 2v2.5M9.5 2v2.5M6.5 11.5V14M9.5 11.5V14M2 6.5h2.5M2 9.5h2.5M11.5 6.5H14M11.5 9.5H14" />
      </>
    ),
  },
  plug: { label: "Serial", svg: <path d="M6 2.5v3M10 2.5v3M4.5 5.5h7v2a3.5 3.5 0 01-7 0zM8 11v2.5" /> },
  shield: { label: "Security", svg: <path d="M8 1.8l5 2v4c0 3-2.2 5.2-5 6.4-2.8-1.2-5-3.4-5-6.4v-4z" /> },
  key: {
    label: "Key",
    svg: (
      <>
        <circle cx="5.5" cy="10.5" r="2.5" />
        <path d="M7.3 8.7L13 3M11 5l1.5 1.5M9.5 6.5L11 8" />
      </>
    ),
  },
  home: { label: "Home", svg: <path d="M2.5 7.5L8 3l5.5 4.5M4 6.5v7h8v-7" /> },
  office: {
    label: "Office",
    svg: (
      <>
        <rect x="3.5" y="2" width="9" height="12" rx=".5" />
        <path d="M6 4.5h1M9 4.5h1M6 7h1M9 7h1M6 9.5h1M9 9.5h1M7 14v-2h2v2" />
      </>
    ),
  },
  lab: { label: "Lab", svg: <path d="M6.5 2h3M7 2v4L3.2 12.3A1.2 1.2 0 004.2 14h7.6a1.2 1.2 0 001-1.7L9 6V2" /> },
  star: { label: "Star", svg: <path d="M8 2.5l1.7 3.5 3.8.5-2.8 2.6.7 3.8L8 11.1l-3.4 1.8.7-3.8L2.5 6.5l3.8-.5z" /> },
  folder: {
    label: "Folder",
    svg: <path d="M2 4.5A1.5 1.5 0 013.5 3h3l1.5 1.5h4.5A1.5 1.5 0 0114 6v5.5a1.5 1.5 0 01-1.5 1.5h-9A1.5 1.5 0 012 11.5z" />,
  },
};

export const ICON_NAMES = Object.keys(PICTURES);

/** Colors offered for icons (readable on light and dark backgrounds). */
export const ICON_COLORS: Array<{ name: string; value: string }> = [
  { name: "Gray", value: "#868e96" },
  { name: "Red", value: "#e03131" },
  { name: "Orange", value: "#f08c00" },
  { name: "Green", value: "#2f9e44" },
  { name: "Teal", value: "#0c8599" },
  { name: "Blue", value: "#1971c2" },
  { name: "Violet", value: "#7048e8" },
  { name: "Pink", value: "#c2255c" },
];

export function iconLabel(name: string): string {
  return PICTURES[name]?.label ?? "Icon";
}

/** A session or folder icon; unknown names show the server picture. */
export function SessionIconGlyph({ icon, size = 16, className }: { icon: SessionIcon; size?: number; className?: string }) {
  const picture = PICTURES[icon.name] ?? PICTURES.server;
  return (
    <svg
      className={`ic session-icon ${className ?? ""}`}
      width={size}
      height={size}
      viewBox="0 0 16 16"
      fill="none"
      stroke={icon.color ?? "currentColor"}
      strokeWidth={1.4}
      strokeLinecap="round"
      strokeLinejoin="round"
      aria-hidden="true"
      focusable="false"
    >
      {picture?.svg}
    </svg>
  );
}

/** Picks a picture and a color; "Automatic" (null) shows the protocol or folder. */
export function IconPicker({
  value,
  onChange,
  automaticLabel = "Automatic",
}: {
  value: SessionIcon | null;
  onChange: (icon: SessionIcon | null) => void;
  automaticLabel?: string;
}) {
  const color = value?.color ?? null;
  return (
    <div className="icon-picker">
      <div className="icon-grid" role="radiogroup" aria-label="Icon">
        <button
          type="button"
          role="radio"
          aria-checked={value === null}
          className="icon-choice auto"
          onClick={() => onChange(null)}
        >
          {automaticLabel}
        </button>
        {ICON_NAMES.map((name) => (
          <button
            key={name}
            type="button"
            role="radio"
            aria-checked={value?.name === name}
            aria-label={iconLabel(name)}
            title={iconLabel(name)}
            className="icon-choice"
            onClick={() => onChange({ name, color })}
          >
            <SessionIconGlyph icon={{ name, color }} size={18} />
          </button>
        ))}
      </div>
      {value && (
        <div className="icon-colors" role="radiogroup" aria-label="Icon color">
          <button
            type="button"
            role="radio"
            aria-checked={color === null}
            className="color-choice none"
            title="Default color"
            aria-label="Default color"
            onClick={() => onChange({ name: value.name, color: null })}
          />
          {ICON_COLORS.map((c) => (
            <button
              key={c.value}
              type="button"
              role="radio"
              aria-checked={color === c.value}
              className="color-choice"
              title={c.name}
              aria-label={c.name}
              style={{ background: c.value }}
              onClick={() => onChange({ name: value.name, color: c.value })}
            />
          ))}
        </div>
      )}
    </div>
  );
}
