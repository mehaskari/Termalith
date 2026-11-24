import { Dialog } from "./Dialog";

export interface LegacyQuestion {
  /** The session name or user@host. */
  target: string;
  /** Which kind of algorithm had no match: kex, key, cipher or mac. */
  kind: string;
  /** What the server offered. */
  offered: string[];
  /** A saved session can keep the choice. */
  canRemember: boolean;
}

export type LegacyAnswer = "once" | "always";

const KIND_NAMES: Record<string, string> = {
  kex: "key exchange",
  key: "host key",
  cipher: "encryption",
  mac: "integrity check",
};

/** "key exchange" for "kex"; unknown kinds as they are. */
export function legacyKindName(kind: string): string {
  return KIND_NAMES[kind] ?? kind;
}

/**
 * The server knows only old algorithms that Termalith leaves out by
 * default (ADR-043). The user may allow them once or for this session;
 * the host key is still checked as usual.
 */
export function LegacyAlgorithmsDialog({
  question,
  onAnswer,
}: {
  question: LegacyQuestion;
  onAnswer: (answer: LegacyAnswer | null) => void;
}) {
  return (
    <Dialog
      title="Old device: weaker security"
      tone="warning"
      onCancel={() => onAnswer(null)}
      actions={
        <>
          <button type="button" onClick={() => onAnswer(null)}>
            Don't connect
          </button>
          <button type="button" className={question.canRemember ? undefined : "primary"} onClick={() => onAnswer("once")}>
            Connect once
          </button>
          {question.canRemember && (
            <button type="button" className="primary" onClick={() => onAnswer("always")}>
              Always allow for this session
            </button>
          )}
        </>
      }
    >
      <p>
        <strong>{question.target}</strong> only offers old {legacyKindName(question.kind)} algorithms that
        Termalith does not use by default:
      </p>
      <p>
        <code>{question.offered.join(", ")}</code>
      </p>
      <p>
        This is common on older switches, routers and firewalls. The connection is still encrypted and the
        host key is still checked, but these algorithms are weaker than modern ones. Update the device&apos;s
        firmware when you can.
      </p>
      {question.canRemember && (
        <p className="hint">
          “Always” saves the choice on this session (Edit session → Allow old SSH algorithms).
        </p>
      )}
    </Dialog>
  );
}
