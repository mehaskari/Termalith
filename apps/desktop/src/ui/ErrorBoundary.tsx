import { Component, type ErrorInfo, type ReactNode } from "react";
import { reportFrontendError } from "../terminal/api";

interface State {
  error: Error | null;
  details: string;
}

/** Replaces a blank window after a UI crash with a way to reload; the error is saved as a crash report. */
export class ErrorBoundary extends Component<{ children: ReactNode }, State> {
  state: State = { error: null, details: "" };

  static getDerivedStateFromError(error: Error): Partial<State> {
    return { error };
  }

  componentDidCatch(error: Error, info: ErrorInfo) {
    const details = [error.stack ?? "", info.componentStack ? `Component stack:${info.componentStack}` : ""]
      .filter(Boolean)
      .join("\n\n");
    this.setState({ details });
    void reportFrontendError(`${error.name}: ${error.message}`, details).catch(() => {});
  }

  render() {
    const { error, details } = this.state;
    if (!error) return this.props.children;
    const text = `${error.name}: ${error.message}\n\n${details}`;
    return (
      <div className="crash-screen" role="alert">
        <h1>Something went wrong</h1>
        <p>
          The Termalith window hit an error. Reloading restarts the window and closes open connections. A crash
          report was saved; you can share it from the next start.
        </p>
        <pre>{text}</pre>
        <div className="crash-actions">
          <button type="button" onClick={() => void navigator.clipboard.writeText(text).catch(() => {})}>
            Copy details
          </button>
          <button type="button" className="primary" onClick={() => window.location.reload()}>
            Reload
          </button>
        </div>
      </div>
    );
  }
}
