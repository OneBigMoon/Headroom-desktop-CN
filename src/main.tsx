import React from "react";
import ReactDOM from "react-dom/client";
import * as Sentry from "@sentry/react";
import App from "./App";
import "./styles.css";

// Only macOS puts a vibrancy layer behind the webview. Elsewhere the window is
// transparent with nothing behind it, so the translucent --surface-* tokens need
// an opaque base to composite onto (see html[data-vibrancy="none"] in styles.css).
if (!navigator.userAgent.includes("Mac")) {
  document.documentElement.dataset.vibrancy = "none";
}

// Packaged builds only. `npm run dev` serves this page to a plain browser, where
// window.__TAURI_INTERNALS__ does not exist, so every startup invoke and event
// listener throws on it -- RUST-8A/8B are 16 events of that, reported against no
// release from a dev machine. Sentry calls are no-ops until init, so the rest of
// the app's capture sites stay silent in dev rather than needing their own guard.
if (import.meta.env.PROD) {
  Sentry.init({
    dsn: import.meta.env.VITE_SENTRY_DSN,
    integrations: [Sentry.browserTracingIntegration()],
    tracesSampleRate: 0.1,
  });
}

function hideBootLoading() {
  const bootLoading = document.getElementById("boot-loading");
  if (!bootLoading) {
    return;
  }
  bootLoading.classList.add("boot-loading--done");
  window.setTimeout(() => {
    bootLoading.remove();
  }, 280);
}

window.addEventListener("headroom:boot-complete", () => {
  window.requestAnimationFrame(() => {
    hideBootLoading();
  });
});

ReactDOM.createRoot(document.getElementById("root") as HTMLElement).render(
  <React.StrictMode>
    <Sentry.ErrorBoundary fallback={<p>Something went wrong.</p>} showDialog>
      <App />
    </Sentry.ErrorBoundary>
  </React.StrictMode>
);
