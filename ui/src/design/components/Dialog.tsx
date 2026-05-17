// Dialog — modal sheet over the ambient bg. Uses the native <dialog>
// element under the hood (modern, accessible, focus-managed by the
// browser, ESC dismisses for free). Glass-thick surface so it feels
// floated. Wave 3 of the Design Language Phase. ADR 0023.

import { useEffect, useRef, type ReactNode } from "react";
import styles from "./Dialog.module.css";

export interface DialogProps {
  open: boolean;
  onClose: () => void;
  title?: string;
  /** Optional icon shown above the title (warning, lock, etc). */
  icon?: ReactNode;
  /** Body content. Most dialogs are <p>{message}</p>. */
  children: ReactNode;
  /** Footer action buttons (typically two Buttons). */
  actions?: ReactNode;
  /** Optional ARIA label override; defaults to title. */
  ariaLabel?: string;
}

export function Dialog({
  open,
  onClose,
  title,
  icon,
  children,
  actions,
  ariaLabel,
}: DialogProps) {
  const ref = useRef<HTMLDialogElement | null>(null);

  // Sync the open prop with the native dialog API. We use showModal()
  // (not show()) so the browser dims the rest of the page and traps
  // focus inside the dialog automatically.
  useEffect(() => {
    const dlg = ref.current;
    if (!dlg) return;
    if (open && !dlg.open) {
      dlg.showModal();
    } else if (!open && dlg.open) {
      dlg.close();
    }
  }, [open]);

  // Forward the native "close" event (e.g. ESC press) to onClose.
  useEffect(() => {
    const dlg = ref.current;
    if (!dlg) return;
    const handler = () => onClose();
    dlg.addEventListener("close", handler);
    return () => dlg.removeEventListener("close", handler);
  }, [onClose]);

  return (
    <dialog
      ref={ref}
      className={styles.dialog}
      aria-label={ariaLabel ?? title}
      onClick={(e) => {
        // Click on the backdrop closes the dialog. <dialog> doesn't
        // natively dismiss on backdrop click; we detect it by
        // comparing the click target to the dialog itself (the
        // backdrop click bubbles AS the dialog because of how
        // ::backdrop works).
        if (e.target === ref.current) onClose();
      }}
    >
      <div className={styles.body}>
        {icon ? <div className={styles.icon}>{icon}</div> : null}
        {title ? <h2 className={styles.title}>{title}</h2> : null}
        <div className={styles.content}>{children}</div>
        {actions ? <div className={styles.actions}>{actions}</div> : null}
      </div>
    </dialog>
  );
}
