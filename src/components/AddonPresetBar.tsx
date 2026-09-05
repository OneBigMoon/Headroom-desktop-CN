import type { DashboardState } from "../lib/types";

export type AddonPresetTarget = { enabled: boolean; mode?: string };

export const RECOMMENDED_ADDON_PRESET: Record<string, AddonPresetTarget> = {
  openspec: { enabled: false },
  superpowers: { enabled: true },
  gstack: { enabled: false },
  "allinluna": { enabled: false },
  "ralph-loop": { enabled: false },
  "stop-that-shit": { enabled: true },
  "agent-guard": { enabled: true },
  serena: { enabled: true },
  "codebase-memory": { enabled: true },
  context7: { enabled: true },
  ponytail: { enabled: true, mode: "full" },
  caveman: { enabled: false },
  rtk: { enabled: true },
  markitdown: { enabled: true },
  "grill-me": { enabled: true },
};

function matchesPreset(dashboard: DashboardState): boolean {
  const matches = Object.entries(RECOMMENDED_ADDON_PRESET).every(([id, target]) => {
    const tool = dashboard.tools.find((candidate) => candidate.id === id);
    if (!tool) return false;
    if (target.enabled !== (tool.status !== "not_installed" && tool.enabled)) return false;
    return !target.mode || target.mode === tool.defaultMode;
  });
  return matches && dashboard.tools.every((tool) => tool.required || RECOMMENDED_ADDON_PRESET[tool.id] || !tool.enabled);
}

export function AddonPresetBar({
  dashboard,
  busy,
  mode,
  onApplyRecommended,
  onSelectCustom,
}: {
  dashboard: DashboardState;
  busy: boolean;
  mode: "recommended" | "custom";
  onApplyRecommended: () => void;
  onSelectCustom: () => void;
}) {
  const recommended = matchesPreset(dashboard);
  return (
    <div className="addon-preset-bar" role="group" aria-label="工具档位">
      <button
        type="button"
        className={`addon-preset-bar__button${mode === "recommended" && recommended ? " is-active" : ""}`}
        disabled={busy || mode === "recommended"}
        onClick={onApplyRecommended}
        aria-busy={busy}
      >
        {busy ? "应用中…" : "推荐"}
      </button>
      <button
        type="button"
        className={`addon-preset-bar__button${mode === "custom" || !recommended ? " is-active" : ""}`}
        disabled={busy}
        onClick={onSelectCustom}
        aria-pressed={mode === "custom" || !recommended}
      >
        自定义
      </button>
      <span className="addon-preset-bar__hint">
        {mode === "recommended" ? "推荐档位已锁定设置；先点自定义才能调整。" : "自定义档位可单独调整工具和节约强度。"}
      </span>
      {busy ? <span className="addon-preset-bar__status">正在应用推荐档位并安装缺失工具…</span> : null}
    </div>
  );
}
