import { describe, expect, it } from "vitest";

import {
  activationScopeCopyByScope,
  getActivationScopeCopy,
  groupToolsByCategory,
  TOOL_CATEGORY_ORDER,
} from "./workflowCatalog";

describe("groupToolsByCategory", () => {
  it("creates every group in the documented order and maps unknown categories to other", () => {
    const tools = [
      { id: "unknown", category: "future" },
      { id: "workflow", category: "workflow" },
      { id: "core", category: "core" },
      { id: "missing", category: null },
    ];

    const groups = groupToolsByCategory(tools);

    expect([...groups.keys()]).toEqual(TOOL_CATEGORY_ORDER);
    expect(groups.get("core")).toEqual([{ id: "core", category: "core" }]);
    expect(groups.get("workflow")).toEqual([{ id: "workflow", category: "workflow" }]);
    expect(groups.get("other")).toEqual([
      { id: "unknown", category: "future" },
      { id: "missing", category: null },
    ]);
  });
});

describe("activation scope copy", () => {
  it("explains immediate, new-session, and client-restart activation in Chinese", () => {
    expect(getActivationScopeCopy("immediate")["zh-CN"]).toContain("即时生效");
    expect(getActivationScopeCopy("new_session")["zh-CN"]).toContain("新 Codex 会话生效");
    expect(getActivationScopeCopy("client_restart")["zh-CN"]).toContain("重启 Codex");
  });

  it("falls back to an explicit unknown scope without claiming a restart", () => {
    expect(getActivationScopeCopy("unexpected")).toEqual(activationScopeCopyByScope.unknown);
    expect(getActivationScopeCopy(null)["zh-CN"]).toContain("取决于具体工具");
    expect(getActivationScopeCopy(undefined)["zh-CN"]).not.toContain("重启");
  });
});
