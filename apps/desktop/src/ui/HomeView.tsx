import { useMemo, useRef, useState } from "react";
import { ArrowLeftRight, FolderPlus, Laptop, Plus, Search, Star, Usb, Wrench } from "lucide-react";
import type { SessionRecord } from "../terminal/api";
import { type LinkState, LinkLight, ProtocolBadge } from "./icons";
import { SessionIconGlyph } from "./sessionIcons";
import { targetLabel } from "./sessionList";

/** Saved sessions whose name, host, user or tag contains every word of `query`. */
export function matchSessions(sessions: SessionRecord[], query: string, limit = 6): SessionRecord[] {
  const words = query.toLowerCase().split(/\s+/).filter(Boolean);
  if (words.length === 0) return [];
  return sessions
    .filter((s) => {
      const hay = [s.name, s.host, s.username, ...(s.tags ?? []).map((t) => `#${t}`)].join(" ").toLowerCase();
      return words.every((w) => hay.includes(w));
    })
    .slice(0, limit);
}

function protocolOf(s: SessionRecord): string {
  if (s.protocol === "ssh" && s.filesOnly) return "files";
  if (s.protocol === "ssh" && s.mosh) return "mosh";
  return s.protocol;
}

function SessionLine({ s, link, onOpen }: { s: SessionRecord; link?: LinkState; onOpen: () => void }) {
  return (
    <li>
      <button type="button" className="home-session" onClick={onOpen} title={`Connect to ${targetLabel(s)}`}>
        {s.icon ? (
          <span className="proto proto-custom" aria-hidden="true">
            <SessionIconGlyph icon={s.icon} size={15} />
          </span>
        ) : (
          <ProtocolBadge protocol={protocolOf(s)} />
        )}
        <span className="home-session-name">{s.name}</span>
        <span className="home-session-target">{targetLabel(s)}</span>
        {link && <LinkLight state={link} />}
      </button>
    </li>
  );
}

/**
 * The start screen: connect by typing (a saved session or user@host),
 * the common first steps, and the sessions used last and starred.
 */
export function HomeView(p: {
  sessions: SessionRecord[];
  recent: string[];
  links: Map<string, LinkState>;
  onConnect: (s: SessionRecord) => void;
  /** Typed text that is not a saved session; returns why it could not be read, or null. */
  onConnectTyped: (text: string) => string | null;
  onNewSession: () => void;
  onLocalTerminal: () => void;
  onSerial: () => void;
  onTools: () => void;
  onImport: () => void;
}) {
  const [query, setQuery] = useState("");
  const [selected, setSelected] = useState(0);
  const [error, setError] = useState<string | null>(null);
  const input = useRef<HTMLInputElement>(null);
  const matches = useMemo(() => matchSessions(p.sessions, query), [p.sessions, query]);
  const byId = useMemo(() => new Map(p.sessions.map((s) => [s.id, s])), [p.sessions]);
  const recent = p.recent.map((id) => byId.get(id)).filter((s): s is SessionRecord => !!s);
  const favorites = p.sessions.filter((s) => s.favorite);

  const submit = () => {
    const pick = matches[selected];
    if (pick) {
      p.onConnect(pick);
      setQuery("");
      return;
    }
    if (!query.trim()) return;
    const why = p.onConnectTyped(query.trim());
    setError(why);
    if (!why) setQuery("");
  };

  return (
    <div className="home">
      <div className="home-inner">
        <h1 className="home-title">Open a connection</h1>
        <form
          className="home-connect"
          role="search"
          onSubmit={(e) => {
            e.preventDefault();
            submit();
          }}
        >
          <Search size={17} aria-hidden="true" className="home-connect-icon" />
          <input
            ref={input}
            autoFocus
            value={query}
            spellCheck={false}
            autoCapitalize="off"
            aria-label="Session name, or user@host to connect"
            placeholder="Session name, or user@host, ssh://…, telnet 10.0.0.1"
            aria-invalid={error ? true : undefined}
            onChange={(e) => {
              setQuery(e.target.value);
              setSelected(0);
              setError(null);
            }}
            onKeyDown={(e) => {
              if (e.key === "ArrowDown") {
                e.preventDefault();
                setSelected((i) => Math.min(i + 1, Math.max(0, matches.length - 1)));
              } else if (e.key === "ArrowUp") {
                e.preventDefault();
                setSelected((i) => Math.max(0, i - 1));
              } else if (e.key === "Escape") {
                setQuery("");
              }
            }}
          />
          <button type="submit" className="primary">
            Connect
          </button>
        </form>
        {error && (
          <p className="form-error home-error" role="alert">
            {error}
          </p>
        )}
        {matches.length > 0 && (
          <ul className="home-matches" aria-label="Matching sessions">
            {matches.map((s, i) => (
              <li key={s.id} className={i === selected ? "selected" : undefined}>
                <button type="button" className="home-session" onClick={() => p.onConnect(s)} onMouseEnter={() => setSelected(i)}>
                  <ProtocolBadge protocol={protocolOf(s)} />
                  <span className="home-session-name">{s.name}</span>
                  <span className="home-session-target">{targetLabel(s)}</span>
                  {p.links.get(s.id) && <LinkLight state={p.links.get(s.id) as LinkState} />}
                </button>
              </li>
            ))}
          </ul>
        )}

        <div className="home-actions">
          <button type="button" onClick={p.onNewSession}>
            <Plus size={18} aria-hidden="true" />
            New session
          </button>
          <button type="button" onClick={p.onLocalTerminal}>
            <Laptop size={18} aria-hidden="true" />
            Local terminal
          </button>
          <button type="button" onClick={p.onSerial}>
            <Usb size={18} aria-hidden="true" />
            Serial console
          </button>
          <button type="button" onClick={p.onTools}>
            <Wrench size={18} aria-hidden="true" />
            Network tools
          </button>
          <button type="button" onClick={p.onImport}>
            <ArrowLeftRight size={18} aria-hidden="true" />
            Import sessions
          </button>
        </div>

        {p.sessions.length === 0 ? (
          <div className="home-empty">
            <FolderPlus size={22} aria-hidden="true" />
            <p>
              No saved sessions yet. Save your servers and devices with <strong>New session</strong>, or bring them over
              from MobaXterm, PuTTY, FileZilla or <code>~/.ssh/config</code> with <strong>Import sessions</strong>.
            </p>
          </div>
        ) : (
          <div className="home-lists">
            <section aria-labelledby="home-recent">
              <h2 id="home-recent">Recent</h2>
              {recent.length === 0 ? (
                <p className="home-hint">Sessions you open show up here.</p>
              ) : (
                <ul className="home-list">
                  {recent.map((s) => (
                    <SessionLine key={s.id} s={s} link={p.links.get(s.id)} onOpen={() => p.onConnect(s)} />
                  ))}
                </ul>
              )}
            </section>
            <section aria-labelledby="home-favorites">
              <h2 id="home-favorites">
                <Star size={14} aria-hidden="true" /> Favorites
              </h2>
              {favorites.length === 0 ? (
                <p className="home-hint">Star a session in the list to keep it here.</p>
              ) : (
                <ul className="home-list">
                  {favorites.slice(0, 8).map((s) => (
                    <SessionLine key={s.id} s={s} link={p.links.get(s.id)} onOpen={() => p.onConnect(s)} />
                  ))}
                </ul>
              )}
            </section>
          </div>
        )}
      </div>
    </div>
  );
}
