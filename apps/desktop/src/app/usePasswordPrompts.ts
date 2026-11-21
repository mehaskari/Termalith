import { useCallback, useRef, useState } from "react";
import type { PasswordAnswer } from "../ui/PasswordDialog";

export interface PasswordQuestion {
  target: string;
  kind: "password" | "passphrase";
  canSave: boolean;
  retry: "wrong" | "savedRejected" | null;
}

interface PasswordRequest extends PasswordQuestion {
  /** Distinct per request, so the dialog starts empty for the next one. */
  id: number;
  resolve: (answer: PasswordAnswer | null) => void;
}

/**
 * Password and key passphrase questions. Several connections may ask at
 * once (e.g. "Open all"), so they queue and one dialog shows at a time.
 */
export function usePasswordPrompts() {
  const [requests, setRequests] = useState<PasswordRequest[]>([]);
  const nextId = useRef(1);

  /** Resolves to the answer, or null when the user cancels. */
  const askPassword = useCallback(
    (question: PasswordQuestion) =>
      new Promise<PasswordAnswer | null>((resolve) =>
        setRequests((q) => [...q, { ...question, id: nextId.current++, resolve }]),
      ),
    [],
  );

  const passwordRequest = requests[0] ?? null;
  const answerPassword = useCallback(
    (answer: PasswordAnswer | null) => {
      if (!passwordRequest) return;
      setRequests((q) => q.filter((r) => r.id !== passwordRequest.id));
      passwordRequest.resolve(answer);
    },
    [passwordRequest],
  );

  return { passwordRequest, askPassword, answerPassword };
}
