import { setupStallMinutes, type SetupStallKind } from "../lib/setupHealthAlert";

export interface SetupStallModalProps {
  kind: SetupStallKind;
  /// Dismiss without navigating. The once-per-day throttle in
  /// `maybeFireSetupStallAlert` is what stops this from re-appearing.
  onClose: () => void;
  /// Take the user to the connector/runtime controls in Settings.
  onOpenSettings: () => void;
}

const LEAD: Record<SetupStallKind, string> = {
  no_traffic:
    "Headroom has been running for {minutes} minutes and has not seen a single request from Claude Code or Codex. That usually means traffic is not reaching it yet.",
  no_savings:
    "Requests are reaching Headroom, but none of them have been optimized. That usually means optimization is paused or blocked rather than misrouted.",
};

const STEPS: Record<SetupStallKind, string[]> = {
  no_traffic: [
    "Restart your terminal, editor, or coding agent so it picks up Headroom's settings.",
    "Check that the connector for your agent is enabled under Settings.",
    "Confirm Headroom itself is running. The menu bar icon is solid when it is.",
  ],
  no_savings: [
    "Check that optimization is not paused on the Home screen.",
    "Confirm your plan still allows optimization under Upgrade.",
    "Restart your coding agent so it reconnects through Headroom.",
  ],
};

/// Shown when the app has been up for a while with zero savings recorded. Fires
/// alongside a native notification (see `maybeFireSetupStallAlert`); this is the
/// surface the user lands on when they open the tray.
export function SetupStallModal({ kind, onClose, onOpenSettings }: SetupStallModalProps) {
  const lead = LEAD[kind].replace("{minutes}", String(setupStallMinutes()));

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
