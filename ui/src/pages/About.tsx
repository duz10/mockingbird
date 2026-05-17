// The "About" page is small enough to ship in Wave A — it's stable.

import { Card, PageHeader } from "../components/primitives";
import { MicIcon } from "../design/Icon";
import { t } from "../i18n";

import styles from "./About.module.css";

export function AboutPage() {
  return (
    <>
      <PageHeader title={t("about.title")} />
      <div className={styles.grid}>
        <Card>
          <div className={styles.hero}>
            <MicIcon size={32} />
            <div>
              <h2 className={styles.tagline}>{t("about.tagline")}</h2>
              <p className={styles.version}>
                {t("about.version")} <span className="mono">0.1.0</span>
              </p>
            </div>
          </div>
        </Card>

        <Card title="What it does">
          <ul className={styles.list}>
            <li>Hold a hotkey, speak, release — your text appears in the focused app.</li>
            <li>Local Whisper for speech-to-text. Local Ollama for cleanup. Cloud Claude optional.</li>
            <li>Per-app dictionary + style examples that improve while you work.</li>
            <li>Full provenance: every session's raw → cleaned → final transcript is saved locally.</li>
          </ul>
        </Card>

        <Card title="What it doesn't do">
          <ul className={styles.list}>
            <li>Send any data to Anthropic, to us, or to anyone else by default.</li>
            <li>Capture audio outside of when you're holding a hotkey.</li>
            <li>Inject into secure input fields (password boxes are detected and skipped).</li>
            <li>Lose your transcripts — raw audio is immutable and provenance is total.</li>
          </ul>
        </Card>

        <Card title="Open source">
          <p className={styles.muted}>{t("about.license")}.</p>
          <p>
            <a
              href="https://github.com/dustinboyd/mockingbird"
              target="_blank"
              rel="noreferrer"
            >
              {t("about.github")} ↗
            </a>
          </p>
        </Card>
      </div>
    </>
  );
}
