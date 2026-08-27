import { Info } from "@phosphor-icons/react";

import { setupStallNoTrafficMinutes, type SetupStallKind } from "../lib/setupHealthAlert";
import { useI18n } from "../lib/i18n";

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

/// Shown when the app has been up for a while with zero savings recorded. Fires
/// alongside a native notification (see `maybeFireSetupStallAlert`); this is the
/// surface the user lands on when they open the tray.
export function SetupStallModal({
  kind,
  onClose,
  onOpenSettings,
  onContact,
}: SetupStallModalProps) {
  const { t } = useI18n();
  const lead = t(kind === "no_traffic" ? "setupStall.noTrafficLead" : "setupStall.noSavingsLead", {
    minutes: setupStallNoTrafficMinutes(),
  });
  const steps = kind === "no_traffic"
    ? [t("setupStall.noTrafficStep1"), t("setupStall.noTrafficStep2"), t("setupStall.noTrafficStep3")]
    : [t("setupStall.noSavingsStep1"), t("setupStall.noSavingsStep2")];

  return (
    <div
      className="modal-backdrop"
      role="dialog"
      aria-modal="true"
      aria-labelledby="setup-stall-title"
      onClick={onClose}
    >
      <div className="modal-card setup-stall" onClick={(event) => event.stopPropagation()}>
        <h3 id="setup-stall-title">{t("setupStall.title")}</h3>
        <p>{lead}</p>
        <ul className="setup-stall__steps">
          {steps.map((step) => (
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
              <strong className="setup-stall__note-title">{t("setupStall.claudeDesktopTitle")}</strong>
              <p>{t("setupStall.claudeDesktopWhy")}</p>
              <p className="setup-stall__note-action">{t("setupStall.claudeDesktopAction")}</p>
            </div>
          </aside>
        ) : null}
        <p className="setup-stall__contact">
          {t("setupStall.contactPrefix")} {" "}
          <button className="link-button" onClick={onContact} type="button">
            {t("setupStall.emailUs")}
          </button>{" "}
          {t("setupStall.contactSuffix")}
        </p>
        <div className="modal-actions">
          <button className="secondary-button" onClick={onClose} type="button">
            {t("actions.dismiss")}
          </button>
          <button className="primary-button" onClick={onOpenSettings} type="button">
            {t("setupStall.openSettings")}
          </button>
        </div>
      </div>
    </div>
  );
}
