// Recording overlay entry point. Fleshed out in Wave B.
//
// We import `./lib/store` for its top-level side effect:
// `syncDesignVersionToDom()` reads the persisted design choice from
// localStorage and sets <html data-design="v1|v2"> on the recording
// window's document so the same v2 CSS overrides activate here as in
// the main window. Without this, the recording window would always
// render v1 because data-design would never be set. (DLW5.)

import { createRoot } from "react-dom/client";

import "./design/global.css";
import "./lib/store";
import { RecordingWindow } from "./recording/RecordingWindow";

const root = document.getElementById("recording-root");
if (!root) throw new Error("#recording-root missing");

createRoot(root).render(<RecordingWindow />);
