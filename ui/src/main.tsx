// Main window entry point.

import React from "react";
import { createRoot } from "react-dom/client";
import { HashRouter, Route, Routes, Navigate } from "react-router-dom";
import { Toaster } from "sonner";

import "./design/global.css";

import { App } from "./App";
import { InsightsPage } from "./pages/Insights";
import { DictationsPage } from "./pages/Dictations";
import { MeetingsPage } from "./pages/Meetings";
import { ActivityPage } from "./pages/Activity";
import { DictionaryPage } from "./pages/Dictionary";
import { KnowledgeGraphPage } from "./routes/knowledge-graph";
import { ModesPage } from "./pages/Modes";
import { SettingsPage } from "./pages/Settings";
import { AboutPage } from "./pages/About";

const container = document.getElementById("root");
if (!container) throw new Error("#root missing");

createRoot(container).render(
  <React.StrictMode>
    <HashRouter>
      <App>
        <Routes>
          <Route path="/" element={<Navigate to="/insights" replace />} />
          <Route path="/insights" element={<InsightsPage />} />
          {/* Legacy /history routes preserved for any deep links
              that may have escaped into shortcuts / notifications
              before the 2026-05-21 rename. New canonical path is
              /dictations. Drop these in a future cleanup once the
              tag has aged. */}
          <Route path="/dictations" element={<DictationsPage />} />
          <Route path="/dictations/:id" element={<DictationsPage />} />
          <Route path="/history" element={<Navigate to="/dictations" replace />} />
          <Route path="/history/:id" element={<DictationsPage />} />
          <Route path="/meetings" element={<MeetingsPage />} />
          <Route path="/meetings/:uuid" element={<MeetingsPage />} />
          <Route path="/activity" element={<ActivityPage />} />
          <Route path="/activity/:id" element={<ActivityPage />} />
          {/* Phase 1D Wave 1D.2 (ADR 0052) -- KG dashboard. The page
              itself gates on the KgGraphEnabled store flag and
              renders a disabled-state when off (route-level guard
              so bookmarks / manual URLs don't 404; mirrors the
              graph-off-UI invariant). */}
          <Route path="/knowledge-graph" element={<KnowledgeGraphPage />} />
          <Route path="/dictionary" element={<DictionaryPage />} />
          <Route path="/modes" element={<ModesPage />} />
          <Route path="/settings" element={<SettingsPage />} />
          <Route path="/settings/:tab" element={<SettingsPage />} />
          <Route path="/about" element={<AboutPage />} />
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
