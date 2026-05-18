// Recording overlay entry point.
//
// After the W6 cutover the design-version machinery is gone, so the
// recording bundle no longer needs to import the store for its side
// effect. Global CSS + the component itself is all we need.

import { createRoot } from "react-dom/client";

import "./design/global.css";
import { RecordingWindow } from "./recording/RecordingWindow";

const root = document.getElementById("recording-root");
if (!root) throw new Error("#recording-root missing");

createRoot(root).render(<RecordingWindow />);
