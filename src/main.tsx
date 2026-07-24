import React from "react";
import ReactDOM from "react-dom/client";
import { getCurrentWebviewWindow } from "@tauri-apps/api/webviewWindow";
import App, { DropHandle, DropPanel } from "./App";
import { DROP_HANDLE_LABEL, DROP_PANEL_LABEL } from "./lib/overlay";

const _windowLabel = getCurrentWebviewWindow().label;

ReactDOM.createRoot(document.getElementById("root") as HTMLElement).render(
  <React.StrictMode>
    {_windowLabel === DROP_HANDLE_LABEL ? <DropHandle /> : _windowLabel === DROP_PANEL_LABEL ? <DropPanel /> : <App />}
  </React.StrictMode>,
);
