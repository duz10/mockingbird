// mb-e3z — top-level React ErrorBoundary.
//
// Wraps the router root so a runtime throw in any mounted component
// renders a graceful fallback panel ("Something went wrong" + a reload
// affordance) instead of collapsing the whole webview to a blank white
// screen — the exact failure mode we hit during the blank-screen
// incident.
//
// Scope + limits (be honest about what this does NOT catch):
//   * Catches: runtime throws during render / lifecycle / in effects
//     that surface through React's render tree.
//   * Does NOT catch: module-load failures (a bad import throws before
//     React mounts — there's no tree to catch it), event-handler throws
//     (React swallows those by design), or async rejections. Those need
//     other nets; this closes the render-throw gap specifically.
//
// Cross-platform: this helps Windows exactly as much as macOS — the
// blank-screen risk is platform-agnostic. Flagged for main-merge.
//
// **Why a class component** (AGENTS says "no class components"): React
// — 18 and 19 alike — provides NO hook equivalent for an error
// boundary. `getDerivedStateFromError` / `componentDidCatch` are
// class-only APIs, and we deliberately don't pull in a third-party dep
// (`react-error-boundary`) for a ~40-line component. This is the single
// sanctioned exception to the no-class rule.

import { Component, type ErrorInfo, type ReactNode } from "react";

import { Button } from "../primitives";
import { t } from "../../i18n";

import styles from "./ErrorBoundary.module.css";

interface ErrorBoundaryProps {
  children: ReactNode;
}

interface ErrorBoundaryState {
  /** The caught error, or `null` while the subtree is healthy. */
  error: Error | null;
}

export class ErrorBoundary extends Component<
  ErrorBoundaryProps,
  ErrorBoundaryState
> {
  state: ErrorBoundaryState = { error: null };

  static getDerivedStateFromError(error: Error): ErrorBoundaryState {
    // Flip to the fallback UI on the next render.
    return { error };
  }

  componentDidCatch(error: Error, info: ErrorInfo): void {
    // No telemetry, ever (AGENTS S0.8) — log locally only so the crash
    // is diagnosable from the devtools console / the webview log.
    // eslint-disable-next-line no-console
    console.error(
      "[ErrorBoundary] render-tree throw:",
      error,
      info.componentStack,
    );
  }

  private handleReload = (): void => {
    // A hard reload re-runs boot from scratch — the simplest reliable
    // recovery for an unknown render fault. HashRouter keeps the URL,
    // but returning to a known-good surface is safer than trusting the
    // faulting route, so we send the user home.
    window.location.hash = "#/";
    window.location.reload();
  };

  render(): ReactNode {
    if (this.state.error) {
      return (
        <div className={styles.wrap} role="alert">
          <div className={styles.panel}>
            <h1 className={styles.title}>{t("error.boundary.title")}</h1>
            <p className={styles.body}>{t("error.boundary.body")}</p>
            <div className={styles.action}>
              <Button
                variant="primary"
                onClick={this.handleReload}
                ariaLabel={t("error.boundary.reload")}
              >
                {t("error.boundary.reload")}
              </Button>
            </div>
          </div>
        </div>
      );
    }
    return this.props.children;
  }
}
