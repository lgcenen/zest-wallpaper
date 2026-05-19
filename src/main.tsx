import React from "react";
import ReactDOM from "react-dom/client";
import App from "./App";
import { initializeWorkbenchDocumentPreferences } from "./state/workbench-preferences";
import "./styles.css";

initializeWorkbenchDocumentPreferences();

ReactDOM.createRoot(document.getElementById("root") as HTMLElement).render(
  <React.StrictMode>
    <App />
  </React.StrictMode>,
);
