import React from "react";
import ReactDOM from "react-dom/client";
import { getCurrentWebviewWindow } from "@tauri-apps/api/webviewWindow";
import App, { DropOverlay } from "./App";

const _windowLabel = getCurrentWebviewWindow().label;

ReactDOM.createRoot(document.getElementById("root") as HTMLElement).render(
  <React.StrictMode>
    {_windowLabel === "drop-overlay" ? <DropOverlay /> : <App />}
  </React.StrictMode>,
);
