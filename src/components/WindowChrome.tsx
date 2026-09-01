import type { MouseEvent } from "react";
import { Minus, Square, X } from "@phosphor-icons/react";
import { getCurrentWindow } from "@tauri-apps/api/window";
import { useI18n } from "../lib/i18n";

export type WindowChromePlatform = "macos" | "windows";

export interface WindowChromeProps {
  platform: WindowChromePlatform;
  title?: string;
}

type WindowAction = "hide" | "minimize" | "toggle maximize" | "start drag";

function reportWindowActionError(action: WindowAction, error: unknown) {
  console.error(`Failed to ${action} window`, error);
}

export function WindowChrome({
  platform,
  title = "Headroom",
}: WindowChromeProps) {
  const { t } = useI18n();

  function startDragging(event: MouseEvent<HTMLElement>) {
    if (event.button !== 0) {
      return;
    }

    const target = event.target as HTMLElement;
    if (target.closest("button, a, input, select, textarea, [data-no-drag]")) {
      return;
    }

    void getCurrentWindow()
      .startDragging()
      .catch((error) => reportWindowActionError("start drag", error));
  }

  function runWindowAction(action: WindowAction, operation: () => Promise<void>) {
    void operation().catch((error) => reportWindowActionError(action, error));
  }

  const hideLabel = t(platform === "windows" ? "window.close" : "window.hide");
  const minimizeLabel = t("window.minimize");
  const maximizeLabel = t("window.maximizeRestore");

  const hideControl = (
    <button
      key="hide"
      aria-label={hideLabel}
      className="window-chrome__control window-chrome__control--hide"
      onClick={() =>
        runWindowAction("hide", () => getCurrentWindow().hide())
      }
      title={hideLabel}
      type="button"
    >
      <X aria-hidden="true" size={10} weight="bold" />
    </button>
  );
  const minimizeControl = (
    <button
      key="minimize"
      aria-label={minimizeLabel}
      className="window-chrome__control window-chrome__control--minimize"
      onClick={() =>
        runWindowAction("minimize", () => getCurrentWindow().minimize())
      }
      title={minimizeLabel}
      type="button"
    >
      <Minus aria-hidden="true" size={10} weight="bold" />
    </button>
  );
  const maximizeControl = (
    <button
      key="maximize"
      aria-label={maximizeLabel}
      className="window-chrome__control window-chrome__control--maximize"
      onClick={() =>
        runWindowAction("toggle maximize", () =>
          getCurrentWindow().toggleMaximize()
        )
      }
      title={maximizeLabel}
      type="button"
    >
      <Square aria-hidden="true" size={9} weight="bold" />
    </button>
  );
  const controls =
    platform === "windows"
      ? [minimizeControl, maximizeControl, hideControl]
      : [hideControl, minimizeControl, maximizeControl];

  return (
    <header
      className="window-chrome"
      onMouseDown={startDragging}
    >
      <div className="window-chrome__controls" data-no-drag>
        {controls}
      </div>
      <span className="window-chrome__title">{title}</span>
      <span className="window-chrome__spacer" aria-hidden="true" />
    </header>
  );
}
