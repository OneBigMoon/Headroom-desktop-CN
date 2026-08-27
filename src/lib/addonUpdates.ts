import type { ManagedTool } from "./types";

export interface AddonUpdateCheck {
  id: string;
  latestVersion: string | null;
  error: string | null;
}

export interface CheckedManagedTool extends ManagedTool {
  upstreamVersion?: string | null;
  upstreamUpdateAvailable?: boolean;
  updateRequiresAppUpdate?: boolean;
}

const DIRECT_MARKETPLACE_UPDATE_IDS = new Set(["ponytail", "caveman"]);

function numericVersion(value: string | null | undefined): number[] | null {
  if (!value) return null;
  const normalized = value.trim().replace(/^[vV]/, "").split(/[+-]/, 1)[0];
  if (!/^\d+\.\d+(?:\.\d+)?$/.test(normalized)) return null;
  return normalized.split(".").map(Number);
}

export function compareAddonVersions(left: string, right: string): number | null {
  const a = numericVersion(left);
  const b = numericVersion(right);
  if (!a || !b) return null;
  for (let index = 0; index < Math.max(a.length, b.length); index += 1) {
    const difference = (a[index] ?? 0) - (b[index] ?? 0);
    if (difference !== 0) return Math.sign(difference);
  }
  return 0;
}

export function applyAddonUpdateChecks(
  tools: ManagedTool[],
  checks: AddonUpdateCheck[],
): CheckedManagedTool[] {
  const byId = new Map(checks.map((check) => [check.id, check]));
  return tools.map((tool) => {
    const check = byId.get(tool.id);
    const installed = tool.status !== "not_installed";
    if (!installed || !check?.latestVersion) return tool;

    const comparison = compareAddonVersions(check.latestVersion, tool.version);
    const upstreamAhead = comparison === 1;
    if (DIRECT_MARKETPLACE_UPDATE_IDS.has(tool.id) && comparison !== null) {
      return {
        ...tool,
        updateAvailable: upstreamAhead && tool.enabled,
        availableVersion: upstreamAhead && tool.enabled ? check.latestVersion : null,
        upstreamVersion: check.latestVersion,
        upstreamUpdateAvailable: upstreamAhead,
        updateRequiresAppUpdate: false,
      };
    }
    if (!upstreamAhead) {
      return { ...tool, upstreamVersion: check.latestVersion };
    }

    const supportedVersion = tool.supportedVersion;
    const beyondSupported =
      !supportedVersion || compareAddonVersions(check.latestVersion, supportedVersion) === 1;
    return {
      ...tool,
      upstreamVersion: check.latestVersion,
      upstreamUpdateAvailable: true,
      updateRequiresAppUpdate: beyondSupported,
    };
  });
}
