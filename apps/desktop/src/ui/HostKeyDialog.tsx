import { type HostKeyAnswer, type HostKeyPrompt, RDP_CERT_ALGORITHM } from "../terminal/api";
import { Dialog } from "./Dialog";

interface Props {
  prompt: HostKeyPrompt;
  onAnswer: (answer: HostKeyAnswer) => void;
}

/**
 * Unknown SSH host keys and RDP certificates. Changed SSH keys never reach
 * this dialog; a changed RDP certificate does (servers renew self-signed
 * certificates), shown with the one trusted before.
 */
export function HostKeyDialog({ prompt, onAnswer }: Props) {
  const rdp = prompt.algorithm === RDP_CERT_ALGORITHM;
  const defaultPort = rdp ? 3389 : 22;
  const where = prompt.port === defaultPort ? prompt.host : `${prompt.host}:${prompt.port}`;
  const changed = rdp && prompt.previousFingerprint !== undefined;
  const title = changed ? "Server certificate changed" : rdp ? "Unknown server certificate" : "Unknown host key";
  return (
    <Dialog
      title={title}
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
            {changed ? "Trust new certificate" : "Trust and connect"}
          </button>
        </>
      }
    >
      {changed ? (
        <p>
          <strong>{where}</strong> presented a different certificate than the one you trusted. Windows
          renews its self-signed Remote Desktop certificate from time to time, but this can also mean
          someone is intercepting the connection.
        </p>
      ) : (
        <p>
          This is the first connection to <strong>{where}</strong>. Its identity cannot be verified
          automatically.
        </p>
      )}
      <dl className="fingerprint">
        <dt>{rdp ? "Type" : "Key type"}</dt>
        <dd>{prompt.algorithm}</dd>
        <dt>{changed ? "New fingerprint" : "Fingerprint"}</dt>
        <dd>
          <code>{prompt.fingerprint}</code>
        </dd>
        {changed && (
          <>
            <dt>Trusted before</dt>
            <dd>
              <code>{prompt.previousFingerprint}</code>
            </dd>
          </>
        )}
      </dl>
      <p className="hint">
        {rdp ? (
          <>
            Compare the fingerprint with the server's certificate before trusting it. On the Windows
            server, PowerShell prints it (without colons) with{" "}
            <code>{"(Get-ChildItem 'Cert:\\LocalMachine\\Remote Desktop').GetCertHashString('SHA256')"}</code>
          </>
        ) : (
          <>
            Compare the fingerprint with one obtained from the server's administrator (for example the
            output of <code>ssh-keygen -lf /etc/ssh/ssh_host_*_key.pub</code> on the server) before
            trusting it.
          </>
        )}
      </p>
    </Dialog>
  );
}
