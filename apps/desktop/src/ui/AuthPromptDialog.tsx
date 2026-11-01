import { type FormEvent, useState } from "react";
import type { AuthPrompt } from "../terminal/api";
import { Dialog } from "./Dialog";

interface Props {
  prompt: AuthPrompt;
  /** `null` cancels the login. */
  onAnswer: (answers: string[] | null) => void;
}

/** Keyboard-interactive questions from the server (one-time codes, PAM). */
export function AuthPromptDialog({ prompt, onAnswer }: Props) {
  // Held only while the dialog is open.
  const [answers, setAnswers] = useState<string[]>(() => prompt.fields.map(() => ""));

  function submit(e: FormEvent) {
    e.preventDefault();
    onAnswer(answers);
    setAnswers(prompt.fields.map(() => ""));
  }

  return (
    <Dialog
      title={prompt.name || "Additional authentication"}
      onCancel={() => onAnswer(null)}
      actions={
        <>
          <button type="button" onClick={() => onAnswer(null)}>
            Cancel
          </button>
          <button type="submit" form="auth-prompt-form" className="primary">
            Continue
          </button>
        </>
      }
    >
      <form id="auth-prompt-form" className="stack" onSubmit={submit}>
        <p>
          <strong>
            {prompt.user}@{prompt.host}
            {prompt.port !== 22 && `:${prompt.port}`}
          </strong>{" "}
          asks:
        </p>
        {prompt.instructions && <p className="auth-instructions">{prompt.instructions}</p>}
        {prompt.fields.map((f, i) => (
          <label key={i}>
            {f.text || `Answer ${i + 1}`}
            <input
              type={f.echo ? "text" : "password"}
              autoFocus={i === 0}
              autoComplete={f.echo ? "one-time-code" : "off"}
              spellCheck={false}
              autoCapitalize="off"
              value={answers[i] ?? ""}
              onChange={(e) => setAnswers((a) => a.map((x, j) => (j === i ? e.target.value : x)))}
            />
          </label>
        ))}
        <p className="hint">The questions above come from the server.</p>
      </form>
    </Dialog>
  );
}
