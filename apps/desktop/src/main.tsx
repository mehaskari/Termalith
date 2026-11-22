import { StrictMode } from "react";
import { createRoot } from "react-dom/client";
import { App } from "./App";
import { MovedTerminal, movedTerminalId } from "./ui/MovedTerminal";
import { ErrorBoundary } from "./ui/ErrorBoundary";
import "./styles.css";

const root = document.getElementById("root");
if (!root) throw new Error("missing #root element");

// A terminal moved into its own window loads this page with ?terminal=<id>.
const movedId = movedTerminalId(window.location.search);

createRoot(root).render(
  <StrictMode>
    <ErrorBoundary>
      {movedId === null ? <App /> : <MovedTerminal id={movedId} />}
    </ErrorBoundary>
  </StrictMode>,
);
