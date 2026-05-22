// Command Center overlay entry point.
//
// Mirrors `recording.tsx` and `meeting_overlay.tsx` — a separate
// Vite multi-entry bundle for the frameless Command Center window
// declared in `src-tauri/tauri.conf.json` (label: "command_center").

import { createRoot } from "react-dom/client";

import "./design/global.css";
import { CommandCenter } from "./command_center/CommandCenter";

const root = document.getElementById("command-center-root");
if (!root) throw new Error("#command-center-root missing");

createRoot(root).render(<CommandCenter />);
