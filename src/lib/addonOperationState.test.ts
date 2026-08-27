import { describe, expect, it } from "vitest";

import {
  clearAddonOperationMessage,
  setAddonOperationMessage,
  type AddonOperationMessages,
} from "./addonOperationState";

describe("addon operation state", () => {
  it("keeps progress and results for concurrent addon operations independent", () => {
    let messages: AddonOperationMessages = {};
    messages = setAddonOperationMessage(messages, "serena", "Installing Serena…");
    messages = setAddonOperationMessage(messages, "context7", "Installing Context7…");

    expect(messages).toEqual({
      serena: "Installing Serena…",
      context7: "Installing Context7…",
    });

    messages = clearAddonOperationMessage(messages, "serena");
    expect(messages).toEqual({ context7: "Installing Context7…" });
  });
});
