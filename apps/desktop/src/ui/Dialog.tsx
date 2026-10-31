import { type ReactNode, useEffect, useRef } from "react";

interface Props {
  title: string;
  onCancel: () => void;
  children: ReactNode;
  /** Rendered in the footer, right-aligned. */
  actions: ReactNode;
  tone?: "default" | "warning";
}

/** Modal built on <dialog>: focus trapping and Esc handling come from the browser. */
export function Dialog({ title, onCancel, children, actions, tone = "default" }: Props) {
  const ref = useRef<HTMLDialogElement>(null);

  useEffect(() => {
    const d = ref.current;
    if (d && !d.open) d.showModal();
  }, []);

  return (
    <dialog
      ref={ref}
      className={`dialog dialog-${tone}`}
      aria-labelledby="dialog-title"
      onCancel={(e) => {
        e.preventDefault();
        onCancel();
      }}
    >
      <h2 id="dialog-title">{title}</h2>
      <div className="dialog-body">{children}</div>
      <div className="dialog-actions">{actions}</div>
    </dialog>
  );
}
