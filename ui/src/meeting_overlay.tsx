// Meeting overlay entry point.
//
// Mirrors `recording.tsx` — a separate Vite multi-entry bundle for
// the frameless meeting-activation window. The window is declared in
// `src-tauri/tauri.conf.json` (label: "meeting_overlay") with the
// same non-activating, always-on-top, transparent treatment as the
// dictation recording pill.

import { createRoot } from "react-dom/client";

import "./design/global.css";
import { MeetingOverlay } from "./meeting_overlay/MeetingOverlay";

const root = document.getElementById("meeting-overlay-root");
if (!root) throw new Error("#meeting-overlay-root missing");

createRoot(root).render(<MeetingOverlay />);
