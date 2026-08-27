import React from "react";
import ReactDOM from "react-dom/client";
import App from "./App";
import { I18nProvider } from "./lib/i18n";
import "./styles.css";
import "./community-upstream.css";

document.documentElement.dataset.edition = "local-community";

// Only macOS puts a vibrancy layer behind the webview. Elsewhere the window is
// transparent with nothing behind it, so the translucent --surface-* tokens need
// an opaque base to composite onto (see html[data-vibrancy="none"] in styles.css).
if (!navigator.userAgent.includes("Mac")) {
  document.documentElement.dataset.vibrancy = "none";
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
    <I18nProvider>
      <App />
    </I18nProvider>
  </React.StrictMode>
);
