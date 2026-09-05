import { useI18n, type Translate, type TranslationKey } from "../lib/i18n";

const TOOL_MODE_LABEL_KEYS: Record<string, TranslationKey> = {
  lite: "tools.mode.lite",
  full: "tools.mode.full",
  ultra: "tools.mode.ultra",
  "wenyan-lite": "tools.mode.wenyanLite",
  "wenyan-full": "tools.mode.wenyanFull",
  "wenyan-ultra": "tools.mode.wenyanUltra",
};

export function localizedToolModeLabel(mode: string, t: Translate): string {
  const key = TOOL_MODE_LABEL_KEYS[mode];
  return key ? t(key) : mode;
}

function selectedToolMode(defaultMode: string | null | undefined, modes: string[]): string {
  if (defaultMode && modes.includes(defaultMode)) {
    return defaultMode;
  }
  return modes.includes("full") ? "full" : modes[0];
}

export function ToolModeSelector({
  toolId,
  name,
  installed,
  enabled,
  defaultMode,
  supportedModes,
  disabled,
  onChange,
}: {
  toolId: string;
  name: string;
  installed: boolean;
  enabled: boolean;
  defaultMode?: string | null;
  supportedModes?: string[];
  disabled: boolean;
  onChange: (mode: string) => void;
}) {
  const { t } = useI18n();
  const modes = supportedModes?.filter(Boolean) ?? [];

  if (!installed || modes.length === 0) {
    return null;
  }
  if (!enabled) {
    return <p className="tool-mode-selector__notice">{t("tools.defaultMode.enableFirst")}</p>;
  }

  const selectedMode = selectedToolMode(defaultMode, modes);
  const detailKey: TranslationKey | null =
    toolId === "ponytail"
      ? "tools.defaultMode.ponytailHelp"
      : toolId === "caveman"
        ? "tools.defaultMode.cavemanHelp"
        : null;

  return (
    <fieldset className="tool-mode-selector" disabled={disabled}>
      <legend>{t("tools.defaultMode.title")}</legend>
      <p className="tool-mode-selector__help">{t("tools.defaultMode.help")}</p>
      {detailKey ? <p className="tool-mode-selector__help">{t(detailKey)}</p> : null}
      <div
        aria-label={t("aria.defaultToolMode", { name })}
        className="tool-mode-selector__options"
        role="radiogroup"
      >
        {modes.map((mode) => (
          <label className="tool-mode-selector__option" key={mode}>
            <input
              checked={mode === selectedMode}
              name={`tool-mode-${toolId}`}
              onChange={() => onChange(mode)}
              type="radio"
              value={mode}
            />
            <span>{localizedToolModeLabel(mode, t)}</span>
          </label>
        ))}
      </div>
      <p className="tool-mode-selector__current-session">
        {t("tools.defaultMode.currentSession")} <code>/{toolId} {selectedMode}</code>
      </p>
    </fieldset>
  );
}
