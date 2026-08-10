import { Info } from "@phosphor-icons/react";

import { setupStallNoTrafficMinutes, type SetupStallKind } from "../lib/setupHealthAlert";

export interface SetupStallModalProps {
  kind: SetupStallKind;
  /// Dismiss without navigating. The once-per-day throttle in
  /// `maybeFireSetupStallAlert` is what stops this from re-appearing.
  onClose: () => void;
  /// Take the user to the connector/runtime controls in Settings.
  onOpenSettings: () => void;
  /// Open a support mail prefilled with the diagnostics for this alert. The
  /// steps above cover the common causes; this is the way out when they don't.
  onContact: () => void;
}

// The no_traffic branch only fires when a connector is configured but has never
// seen traffic come back, so the copy can state that as fact rather than asking
// the user to go check whether anything is connected.
const LEAD: Record<SetupStallKind, string> = {
  no_traffic:
    "Your coding agent is connected to Headroom, but {minutes} minutes on, not one request has come back through it. That almost always means the agent is still running with the settings it had before Headroom was installed.",
  no_savings:
    "Requests are reaching Headroom, but none of them have been optimized. That usually means your coding agent connected before Headroom was ready, or optimization is paused.",
};

const STEPS: Record<SetupStallKind, string[]> = {
  no_traffic: [
    "Quit your terminal, editor, or coding agent completely, then reopen it. A new tab or window often reuses the old environment.",
    "Run your agent from a freshly opened terminal so it picks up Headroom's settings.",
    "Confirm Headroom itself is running. The menu bar icon is solid when it is.",
  ],
  // Reconnecting first: an agent that started before Headroom was ready is the
  // common cause, and it is the cheapest thing to try.
  //
  // There is deliberately no "check your plan" step. `evaluateSetupStall`
  // returns null when the account gate has optimization switched off, so if the
  // plan were the cause this modal would never have appeared. Listing it sent
  // people to verify the one thing already ruled out.
  no_savings: [
    "Restart your coding agent so it reconnects through Headroom.",
    "Check that Headroom is not paused. The Home screen shows its current state.",
  ],
};

// Shown only on the no-traffic branch, where the steps above would otherwise
// send a Claude-desktop user chasing a stale terminal environment they don't
// have. No detection behind it: having the desktop app installed doesn't mean
// that's where they run Claude Code, so the title asks rather than accuses.
// The body names Anthropic as the source of the limitation, because "this
// cannot work" without whose constraint it is reads as Headroom being broken.
//
// Split into title / why / what-to-do so the one line that is actually
// actionable isn't buried at the end of a paragraph.
const CLAUDE_DESKTOP_NOTE = {
  title: "Running Claude Code in the Claude desktop app?",
  why: "That one can't be optimized. Anthropic's desktop app doesn't use the Claude Code CLI's configuration, so its requests never pass through Headroom. That is a design decision on Anthropic's side, not something restarting will fix.",
  action:
    "Run Claude Code from a terminal, or use the VS Code extension, and Headroom picks it up automatically.",
};

/// Shown when the app has been up for a while with zero savings recorded. Fires
/// alongside a native notification (see `maybeFireSetupStallAlert`); this is the
/// surface the user lands on when they open the tray.
export function SetupStallModal({
  kind,
  onClose,
  onOpenSettings,
  onContact,
}: SetupStallModalProps) {
  const lead = LEAD[kind].replace("{minutes}", String(setupStallNoTrafficMinutes()));

  return (
    <div
      className="modal-backdrop"
      role="dialog"
      aria-modal="true"
      aria-labelledby="setup-stall-title"
      onClick={onClose}
    >
      <div className="modal-card setup-stall" onClick={(event) => event.stopPropagation()}>
        <h3 id="setup-stall-title">Headroom hasn't saved anything yet</h3>
        <p>{lead}</p>
        <ul className="setup-stall__steps">
          {STEPS[kind].map((step) => (
            <li key={step}>{step}</li>
          ))}
        </ul>
        {kind === "no_traffic" ? (
          <aside className="setup-stall__note">
            <Info
              aria-hidden="true"
              className="setup-stall__note-icon"
              size={15}
              weight="fill"
            />
            <div className="setup-stall__note-body">
              <strong className="setup-stall__note-title">{CLAUDE_DESKTOP_NOTE.title}</strong>
              <p>{CLAUDE_DESKTOP_NOTE.why}</p>
              <p className="setup-stall__note-action">{CLAUDE_DESKTOP_NOTE.action}</p>
            </div>
          </aside>
        ) : null}
        <p className="setup-stall__contact">
          Still stuck after all that?{" "}
          <button className="link-button" onClick={onContact} type="button">
            Email us
          </button>{" "}
          and we'll look into it.
        </p>
        <div className="modal-actions">
          <button className="secondary-button" onClick={onClose} type="button">
            Dismiss
          </button>
          <button className="primary-button" onClick={onOpenSettings} type="button">
            Open settings
          </button>
        </div>
      </div>
    </div>
  );
}
