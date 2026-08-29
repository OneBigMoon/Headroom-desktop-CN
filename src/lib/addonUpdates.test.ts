import { describe, expect, it } from "vitest";

import { applyAddonUpdateChecks, compareAddonVersions } from "./addonUpdates";
import type { ManagedTool } from "./types";

function tool(overrides: Partial<ManagedTool>): ManagedTool {
  return {
    id: "context7",
    name: "Context7",
    description: "docs",
    runtime: "plugin",
    required: false,
    enabled: true,
    status: "healthy",
    sourceUrl: "https://example.test",
    version: "4.0.2",
    supportedVersion: "4.0.2",
    ...overrides,
  };
}

describe("addon update checks", () => {
  it("compares normalized numeric versions", () => {
    expect(compareAddonVersions("v4.0.4", "4.0.3")).toBe(1);
    expect(compareAddonVersions("4.0", "4.0.0")).toBe(0);
    expect(compareAddonVersions("latest", "4.0.0")).toBeNull();
  });

  it("distinguishes direct plugin updates from releases needing app support", () => {
    const checked = applyAddonUpdateChecks(
      [
        tool({ id: "context7" }),
        tool({ id: "ponytail", version: "4.8.0", supportedVersion: "latest" }),
      ],
      [
        { id: "context7", latestVersion: "4.0.4", error: null },
        { id: "ponytail", latestVersion: "4.9.0", error: null },
      ],
    );

    expect(checked[0]).toMatchObject({
      upstreamVersion: "4.0.4",
      upstreamUpdateAvailable: true,
      updateRequiresAppUpdate: true,
    });
    expect(checked[0].updateAvailable).toBeUndefined();
    expect(checked[1]).toMatchObject({
      availableVersion: "4.9.0",
      updateAvailable: true,
      updateRequiresAppUpdate: false,
    });
  });

  it("does not show update notices for uninstalled tools", () => {
    const [checked] = applyAddonUpdateChecks(
      [tool({ status: "not_installed" })],
      [{ id: "context7", latestVersion: "4.0.4", error: null }],
    );
    expect(checked.upstreamUpdateAvailable).toBeUndefined();
  });

  it("removes the marketplace's optimistic update action after a current version is confirmed", () => {
    const [checked] = applyAddonUpdateChecks(
      [tool({ id: "ponytail", version: "4.9.0", supportedVersion: "latest", updateAvailable: true })],
      [{ id: "ponytail", latestVersion: "4.9.0", error: null }],
    );
    expect(checked.updateAvailable).toBe(false);
    expect(checked.availableVersion).toBeNull();
  });

  it("keeps a compatible pinned update actionable after the upstream check", () => {
    const [checked] = applyAddonUpdateChecks(
      [
        tool({
          version: "4.0.2",
          supportedVersion: "4.0.4",
          updateAvailable: true,
          availableVersion: "4.0.4",
        }),
      ],
      [{ id: "context7", latestVersion: "4.0.4", error: null }],
    );

    expect(checked).toMatchObject({
      updateAvailable: true,
      availableVersion: "4.0.4",
      upstreamVersion: "4.0.4",
      upstreamUpdateAvailable: true,
      updateRequiresAppUpdate: false,
    });
  });

  it("keeps the supported Headroom CLI update actionable", () => {
    const [checked] = applyAddonUpdateChecks(
      [
        tool({
          id: "headroom",
          name: "Headroom CLI",
          required: true,
          version: "0.35.0",
          supportedVersion: "0.36.5",
          updateAvailable: true,
          availableVersion: "0.36.5",
        }),
      ],
      [{ id: "headroom", latestVersion: "0.36.5", error: null }],
    );

    expect(checked).toMatchObject({
      updateAvailable: true,
      availableVersion: "0.36.5",
      upstreamVersion: "0.36.5",
      updateRequiresAppUpdate: false,
    });
  });
});
