import type { HostKeyAnswer, HostKeyPrompt } from "../terminal/api";
import { Dialog } from "./Dialog";

interface Props {
  prompt: HostKeyPrompt;
  onAnswer: (answer: HostKeyAnswer) => void;
}

/** Shown only for keys never seen before. Changed keys never reach this dialog. */
export function HostKeyDialog({ prompt, onAnswer }: Props) {
  const where = prompt.port === 22 ? prompt.host : `${prompt.host}:${prompt.port}`;
  return (
    <Dialog
      title="Unknown host key"
      tone="warning"
      onCancel={() => onAnswer("reject")}
      actions={
        <>
          <button type="button" onClick={() => onAnswer("reject")}>
            Don't connect
          </button>
          <button type="button" onClick={() => onAnswer("acceptOnce")}>
            Connect once
          </button>
          <button type="button" className="primary" onClick={() => onAnswer("acceptAndSave")}>
            Trust and connect
          </button>
        </>
      }
    >
      <p>
        This is the first connection to <strong>{where}</strong>. Its identity cannot be verified
        automatically.
      </p>
      <dl className="fingerprint">
        <dt>Key type</dt>
        <dd>{prompt.algorithm}</dd>
        <dt>Fingerprint</dt>
        <dd>
          <code>{prompt.fingerprint}</code>
        </dd>
      </dl>
      <p className="hint">
        Compare the fingerprint with one obtained from the server's administrator (for example the
        output of <code>ssh-keygen -lf /etc/ssh/ssh_host_*_key.pub</code> on the server) before
        trusting it.
      </p>
    </Dialog>
  );
}
