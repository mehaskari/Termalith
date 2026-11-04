import type { ReactElement } from "react";

/** Small stroke icons (16px grid), drawn for Termalith. */
const PATHS: Record<string, ReactElement> = {
  chevron: <path d="M6 4l4 4-4 4" />,
  folder: <path d="M2 4.5A1.5 1.5 0 013.5 3h3l1.5 1.5h4.5A1.5 1.5 0 0114 6v5.5a1.5 1.5 0 01-1.5 1.5h-9A1.5 1.5 0 012 11.5z" />,
  folderOpen: (
    <path d="M2 11.5V4.5A1.5 1.5 0 013.5 3h3l1.5 1.5h4A1.5 1.5 0 0113.5 6v1M2 11.5L3.6 7.6A1 1 0 014.5 7h9.6a.6.6 0 01.55.83L13.3 12.2a1.3 1.3 0 01-1.2.8H3.5A1.5 1.5 0 012 11.5z" />
  ),
  folderPlus: (
    <>
      <path d="M2 4.5A1.5 1.5 0 013.5 3h3l1.5 1.5h4.5A1.5 1.5 0 0114 6v5.5a1.5 1.5 0 01-1.5 1.5h-9A1.5 1.5 0 012 11.5z" />
      <path d="M8 7v4M6 9h4" />
    </>
  ),
  plus: <path d="M8 3v10M3 8h10" />,
  search: (
    <>
      <circle cx="7" cy="7" r="4" />
      <path d="M10 10l3.5 3.5" />
    </>
  ),
  star: <path d="M8 2.5l1.7 3.5 3.8.5-2.8 2.6.7 3.8L8 11.1l-3.4 1.8.7-3.8L2.5 6.5l3.8-.5z" />,
  more: (
    <>
      <circle cx="3.5" cy="8" r=".6" />
      <circle cx="8" cy="8" r=".6" />
      <circle cx="12.5" cy="8" r=".6" />
    </>
  ),
  edit: <path d="M10.5 3l2.5 2.5L6 12.5H3.5V10zM9 4.5l2.5 2.5" />,
  copy: (
    <>
      <rect x="5.5" y="5.5" width="8" height="8" rx="1.5" />
      <path d="M10.5 5.5V4A1.5 1.5 0 009 2.5H4A1.5 1.5 0 002.5 4v5A1.5 1.5 0 004 10.5h1.5" />
    </>
  ),
  trash: <path d="M3 4.5h10M6.5 4.5V3h3v1.5M4.5 4.5l.6 8a1 1 0 001 .9h3.8a1 1 0 001-.9l.6-8" />,
  files: <path d="M5 13V3M2.5 5.5L5 3l2.5 2.5M11 3v10M8.5 10.5L11 13l2.5-2.5" />,
  terminal: (
    <>
      <rect x="2" y="3" width="12" height="10" rx="1.5" />
      <path d="M4.5 6.5L6.5 8l-2 1.5M8 10h3" />
    </>
  ),
  plug: <path d="M6 2.5v3M10 2.5v3M4.5 5.5h7v2a3.5 3.5 0 01-7 0zM8 11v2.5" />,
  gear: (
    <>
      <circle cx="8" cy="8" r="2" />
      <path d="M8 1.8v1.7M8 12.5v1.7M1.8 8h1.7M12.5 8h1.7M3.6 3.6l1.2 1.2M11.2 11.2l1.2 1.2M3.6 12.4l1.2-1.2M11.2 4.8l1.2-1.2" />
    </>
  ),
  info: (
    <>
      <circle cx="8" cy="8" r="6" />
      <path d="M8 7.2V11M8 5v.1" />
    </>
  ),
  tools: <path d="M2.5 13.5l5-5M9.5 2.6a3 3 0 00-1.8 4.1l-5.2 5.2M9.5 2.6l-.9 2.1 1.4 1.4 2.1-.9a3 3 0 01-4.1 1.5" />,
  key: (
    <>
      <circle cx="5.5" cy="10.5" r="2.5" />
      <path d="M7.3 8.7L13 3M11 5l1.5 1.5M9.5 6.5L11 8" />
    </>
  ),
  swap: <path d="M3 5.5h9.5M10 3l2.5 2.5L10 8M13 10.5H3.5M6 8l-2.5 2.5L6 13" />,
  x: <path d="M4 4l8 8M12 4l-8 8" />,
  play: <path d="M5 3.5v9l7-4.5z" />,
};

export type IconName = keyof typeof PATHS;

export function Icon({ name, size = 16, className }: { name: IconName; size?: number; className?: string }) {
  return (
    <svg
      className={`ic ${className ?? ""}`}
      width={size}
      height={size}
      viewBox="0 0 16 16"
      fill="none"
      stroke="currentColor"
      strokeWidth={1.4}
      strokeLinecap="round"
      strokeLinejoin="round"
      aria-hidden="true"
      focusable="false"
    >
      {PATHS[name]}
    </svg>
  );
}

const PROTOCOL_LABEL: Record<string, string> = {
  ssh: "SSH",
  rdp: "RDP",
  vnc: "VNC",
  telnet: "TEL",
  ftp: "FTP",
  serial: "COM",
  local: "SH",
  files: "SFTP",
};

/** Color-coded protocol tile used in the session tree and tabs. */
export function ProtocolBadge({ protocol }: { protocol: string }) {
  return (
    <span className={`proto proto-${protocol}`} aria-hidden="true">
      {PROTOCOL_LABEL[protocol] ?? protocol.slice(0, 3).toUpperCase()}
    </span>
  );
}
