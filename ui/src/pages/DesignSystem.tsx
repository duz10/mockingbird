// /design-system — developer-only showcase route.
//
// Renders every Design Language v1 component primitive in every
// variant + state. Acts as the live, in-repo equivalent of
// docs/design/design-language-v1.html. When a component looks wrong
// here, fix the component file — never patch the page that uses it.
//
// This page is intentionally NOT linked from the sidebar. Reach it
// by typing #/design-system in the address bar. Removed at the W6
// cutover commit (the live design language IS the app by then; a
// separate showcase route stops being useful).
//
// Wave 3 of the Design Language Phase. ADR 0023.

import { useState } from "react";
import { toast } from "sonner";

import {
  Button,
  Chip,
  Dialog,
  Input,
  ListItem,
  MockingbirdMark,
  Segmented,
  Switch,
} from "../design/components";
import {
  BookIcon,
  HistoryIcon,
  MicIcon,
  SearchIcon,
  SettingsIcon,
  SparklesIcon,
} from "../design/Icon";

import styles from "./DesignSystem.module.css";

type SortValue = "newest" | "oldest" | "longest";

export function DesignSystemPage() {
  const [switchState, setSwitchState] = useState(true);
  const [sort, setSort] = useState<SortValue>("newest");
  const [searchValue, setSearchValue] = useState("");
  const [dialogOpen, setDialogOpen] = useState(false);

  return (
    <div className={styles.page}>
      <header className={styles.header}>
        <MockingbirdMark size={64} state="static" />
        <div>
          <p className={styles.eyebrow}>Design Language v1 · showcase</p>
          <h1 className={styles.h1}>Mockingbird, in pieces</h1>
          <p className={styles.lede}>
            Every component primitive in every state. Toggle v1↔v2 with
            the sidebar pill to compare. Source of truth:{" "}
            <code>docs/design/design-language-v1.html</code>.
          </p>
        </div>
      </header>

      {/* ────────── Brand mark ────────── */}
      <Section title="Brand mark · MockingbirdMark">
        <div className={styles.markRow}>
          <MarkCard label="static" state="static" />
          <MarkCard label="idle" state="idle" />
          <MarkCard label="active" state="active" />
          <MarkCard label="splash" state="splash" />
          <MarkCard label="exit" state="exit" />
        </div>
      </Section>

      {/* ────────── Buttons ────────── */}
      <Section title="Buttons · 7 variants × 3 sizes">
        <Row>
          <Button variant="filled">Filled</Button>
          <Button variant="tonal">Tonal</Button>
          <Button variant="outlined">Outlined</Button>
          <Button variant="text">Text</Button>
          <Button variant="glass">Glass</Button>
          <Button variant="icon" aria-label="Settings">
            <SettingsIcon />
          </Button>
          <Button variant="fab" leadingIcon={<MicIcon />}>
            New dictation
          </Button>
        </Row>
        <Row>
          <Button variant="filled" size="sm">Small</Button>
          <Button variant="filled" size="md">Medium</Button>
          <Button variant="filled" size="lg">Large</Button>
          <Button variant="filled" disabled>Disabled</Button>
        </Row>
      </Section>

      {/* ────────── Inputs ────────── */}
      <Section title="Inputs · text + glass-search">
        <div className={styles.inputGrid}>
          <Input label="Name" placeholder="e.g. Drafting" />
          <Input
            label="With helper"
            placeholder="hold to dictate"
            helperText="Hotkey is reserved when this is set."
          />
          <Input
            label="Error state"
            defaultValue="Right Alt"
            error="Already used by another mode."
          />
          <Input
            variant="search"
            placeholder="Search transcripts…"
            leadingIcon={<SearchIcon />}
            value={searchValue}
            onChange={(e) => setSearchValue(e.target.value)}
          />
        </div>
      </Section>

      {/* ────────── Switch ────────── */}
      <Section title="Switch · on / off">
        <div className={styles.switchGrid}>
          <Switch
            label="On-device transcription"
            description="Audio never leaves this device."
            checked={switchState}
            onChange={(e) => setSwitchState(e.target.checked)}
          />
          <Switch
            label="Auto-detect language"
            description="Currently locked to English."
            checked={false}
            onChange={() => undefined}
          />
          <Switch
            label="Disabled toggle"
            description="Greyed out, no pointer."
            checked={true}
            disabled
            onChange={() => undefined}
          />
        </div>
      </Section>

      {/* ────────── Chips ────────── */}
      <Section title="Chips · neutral + accent + dismissable">
        <Row>
          <Chip>Neutral</Chip>
          <Chip tone="accent">Accent</Chip>
          <Chip leadingIcon={<SparklesIcon size={12} />}>With icon</Chip>
          <Chip onDismiss={() => toast.message("Dismissed (just a demo)")}>
            Dismissable
          </Chip>
          <Chip onClick={() => toast.success("Clicked!")}>Clickable</Chip>
        </Row>
      </Section>

      {/* ────────── Segmented ────────── */}
      <Section title="Segmented control">
        <Segmented<SortValue>
          ariaLabel="Sort order"
          value={sort}
          onChange={setSort}
          options={[
            { value: "newest", label: "Newest" },
            { value: "oldest", label: "Oldest" },
            { value: "longest", label: "Longest" },
          ]}
        />
      </Section>

      {/* ────────── ListItem ────────── */}
      <Section title="List items · history-style rows">
        <div className={styles.listSurface}>
          <ListItem
            leading="PB"
            title="Podcast brainstorm"
            meta="38 min · 6,820 words · draft"
            trailing="YESTERDAY"
            onClick={() => toast("Selected: podcast brainstorm")}
          />
          <ListItem
            leading="VM"
            title="Voice memo — book idea"
            meta="3 min · 410 words"
            trailing="MAY 12"
            onClick={() => toast("Selected: voice memo")}
            selected
          />
          <ListItem
            leading={<HistoryIcon size={18} />}
            title="Standup notes"
            meta="6 min · 870 words"
            trailing="MAY 10"
            onClick={() => toast("Selected: standup notes")}
          />
          <ListItem
            leading={<BookIcon size={18} />}
            title="Static row · no click handler"
            meta="Doesn't react to clicks."
          />
        </div>
      </Section>

      {/* ────────── Dialog ────────── */}
      <Section title="Dialog · glass-thick modal">
        <Row>
          <Button variant="filled" onClick={() => setDialogOpen(true)}>
            Open dialog
          </Button>
          <Button
            variant="tonal"
            onClick={() => toast.success("Transcript saved to library")}
          >
            Trigger success toast
          </Button>
          <Button
            variant="outlined"
            onClick={() =>
              toast.error("Microphone disconnected — reconnect and tap Resume.")
            }
          >
            Trigger error toast
          </Button>
        </Row>
      </Section>

      <Dialog
        open={dialogOpen}
        onClose={() => setDialogOpen(false)}
        icon={<MicIcon size={24} />}
        title="Allow microphone access?"
        actions={
          <>
            <Button variant="text" onClick={() => setDialogOpen(false)}>
              Not now
            </Button>
            <Button
              variant="filled"
              onClick={() => {
                setDialogOpen(false);
                toast.success("Microphone access granted.");
              }}
            >
              Allow
            </Button>
          </>
        }
      >
        <p>
          Mockingbird needs your microphone to transcribe what you say. Audio
          stays on this device — Mockingbird never sends it anywhere.
        </p>
      </Dialog>
    </div>
  );
}

/* ---------------- helpers ---------------- */

function Section({ title, children }: { title: string; children: React.ReactNode }) {
  return (
    <section className={styles.section}>
      <h2 className={styles.h2}>{title}</h2>
      <div className={styles.body}>{children}</div>
    </section>
  );
}

function Row({ children }: { children: React.ReactNode }) {
  return <div className={styles.row}>{children}</div>;
}

function MarkCard({ label, state }: { label: string; state: "static" | "idle" | "active" | "splash" | "exit" }) {
  // Force a remount on splash/exit so the animation replays each render.
  const [tick, setTick] = useState(0);
  return (
    <button type="button" className={styles.markCard} onClick={() => setTick((t) => t + 1)}>
      <MockingbirdMark key={tick} size={64} state={state} />
      <span className={styles.markLabel}>{label}</span>
      <span className={styles.markHint}>
        {state === "splash" || state === "exit" ? "click to replay" : ""}
      </span>
    </button>
  );
}
