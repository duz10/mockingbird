// Main window entry point.

import React from "react";
import { createRoot } from "react-dom/client";
import { HashRouter, Route, Routes, Navigate } from "react-router-dom";
import { Toaster } from "sonner";

import "./design/global.css";

import { App } from "./App";
import { InsightsPage } from "./pages/Insights";
import { HistoryPage } from "./pages/History";
import { DictionaryPage } from "./pages/Dictionary";
import { ModesPage } from "./pages/Modes";
import { SettingsPage } from "./pages/Settings";
import { AboutPage } from "./pages/About";
import { DesignSystemPage } from "./pages/DesignSystem";

const container = document.getElementById("root");
if (!container) throw new Error("#root missing");

createRoot(container).render(
  <React.StrictMode>
    <HashRouter>
      <App>
        <Routes>
          <Route path="/" element={<Navigate to="/insights" replace />} />
          <Route path="/insights" element={<InsightsPage />} />
          <Route path="/history" element={<HistoryPage />} />
          <Route path="/history/:id" element={<HistoryPage />} />
          <Route path="/dictionary" element={<DictionaryPage />} />
          <Route path="/modes" element={<ModesPage />} />
          <Route path="/settings" element={<SettingsPage />} />
          <Route path="/settings/:tab" element={<SettingsPage />} />
          <Route path="/about" element={<AboutPage />} />
          {/* Developer-only design-system showcase. Not linked from
              the sidebar; reach by typing #/design-system. Removed at
              the W6 cutover commit. ADR 0023. */}
          <Route path="/design-system" element={<DesignSystemPage />} />
          <Route path="*" element={<Navigate to="/insights" replace />} />
        </Routes>
      </App>
    </HashRouter>
    <Toaster
      position="bottom-right"
      theme="system"
      richColors
      closeButton
      duration={3000}
    />
  </React.StrictMode>,
);
