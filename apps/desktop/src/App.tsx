import { TerminalView } from "./terminal/TerminalView";

// Phase 0a spike: a single local terminal filling the window. Tabs, sessions
// and navigation are Phase 1 and intentionally absent.
export function App() {
  return (
    <main className="app">
      <TerminalView />
    </main>
  );
}
