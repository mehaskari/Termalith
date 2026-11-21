import { useCallback, useEffect, useState } from "react";
import { listen } from "@tauri-apps/api/event";
import {
  type AuthPrompt,
  type HostKeyAnswer,
  type HostKeyPrompt,
  respondAuthPrompt,
  respondHostKey,
} from "../terminal/api";

/**
 * The questions the backend asks while connecting: unknown or changed host
 * keys, and keyboard-interactive prompts (one-time codes). Each queue shows
 * its first entry; host keys come before other prompts.
 */
export function useConnectionPrompts() {
  const [hostKeyPrompts, setHostKeyPrompts] = useState<HostKeyPrompt[]>([]);
  const [authPrompts, setAuthPrompts] = useState<AuthPrompt[]>([]);

  useEffect(() => {
    const unlistenHostKey = listen<HostKeyPrompt>("host-key-prompt", (e) =>
      setHostKeyPrompts((q) => [...q, e.payload]),
    );
    const unlistenAuth = listen<AuthPrompt>("auth-prompt", (e) => setAuthPrompts((q) => [...q, e.payload]));
    return () => {
      void unlistenHostKey.then((f) => f());
      void unlistenAuth.then((f) => f());
    };
  }, []);

  const answerHostKey = useCallback((prompt: HostKeyPrompt, answer: HostKeyAnswer) => {
    setHostKeyPrompts((q) => q.filter((p) => p.requestId !== prompt.requestId));
    void respondHostKey(prompt.requestId, answer);
  }, []);

  const answerAuth = useCallback((prompt: AuthPrompt, answers: string[] | null) => {
    setAuthPrompts((q) => q.filter((p) => p.requestId !== prompt.requestId));
    void respondAuthPrompt(prompt.requestId, answers);
  }, []);

  const hostKeyPrompt = hostKeyPrompts[0];
  return {
    hostKeyPrompt,
    answerHostKey,
    /** Shown only while no host key question is open. */
    authPrompt: hostKeyPrompt ? undefined : authPrompts[0],
    answerAuth,
  };
}
