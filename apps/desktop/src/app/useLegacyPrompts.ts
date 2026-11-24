import { useCallback, useRef, useState } from "react";
import type { LegacyAnswer, LegacyQuestion } from "../ui/LegacyAlgorithmsDialog";

interface LegacyRequest extends LegacyQuestion {
  id: number;
  resolve: (answer: LegacyAnswer | null) => void;
}

/** Questions about old SSH algorithms (ADR-043), one dialog at a time. */
export function useLegacyPrompts() {
  const [requests, setRequests] = useState<LegacyRequest[]>([]);
  const nextId = useRef(1);

  /** Resolves to the user's answer, or null when they decline. */
  const askLegacy = useCallback(
    (question: LegacyQuestion) =>
      new Promise<LegacyAnswer | null>((resolve) =>
        setRequests((q) => [...q, { ...question, id: nextId.current++, resolve }]),
      ),
    [],
  );

  const legacyRequest = requests[0] ?? null;
  const answerLegacy = useCallback(
    (answer: LegacyAnswer | null) => {
      if (!legacyRequest) return;
      setRequests((q) => q.filter((r) => r.id !== legacyRequest.id));
      legacyRequest.resolve(answer);
    },
    [legacyRequest],
  );

  return { legacyRequest, askLegacy, answerLegacy };
}
