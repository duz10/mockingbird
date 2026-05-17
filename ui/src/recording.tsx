// Recording overlay entry point. Fleshed out in Wave B.

import { createRoot } from "react-dom/client";

import "./design/global.css";
import { RecordingWindow } from "./recording/RecordingWindow";

const root = document.getElementById("recording-root");
if (!root) throw new Error("#recording-root missing");

createRoot(root).render(<RecordingWindow />);
