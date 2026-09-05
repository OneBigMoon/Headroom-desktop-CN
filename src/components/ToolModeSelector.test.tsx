import { describe, expect, it, vi } from "vitest";
import { render, screen, within } from "@testing-library/react";
import userEvent from "@testing-library/user-event";

import { ToolModeSelector } from "./ToolModeSelector";

describe("ToolModeSelector", () => {
  it("shows Ponytail intensity details and saves a different mode", async () => {
    const onChange = vi.fn();
    const user = userEvent.setup();
    render(
      <ToolModeSelector
        toolId="ponytail"
        name="Ponytail"
        installed
        enabled
        defaultMode="full"
        supportedModes={["lite", "full", "ultra"]}
        disabled={false}
        onChange={onChange}
      />
    );

    const modes = screen.getByRole("radiogroup", { name: "Default mode for Ponytail" });
    expect(within(modes).getAllByRole("radio")).toHaveLength(3);
    expect(within(modes).getByRole("radio", { name: "Full" })).toBeChecked();
    expect(screen.getByText(/Full \(recommended\)/i)).toBeInTheDocument();
    expect(screen.getByText("/ponytail full")).toBeInTheDocument();

    await user.click(within(modes).getByRole("radio", { name: "Ultra" }));
    expect(onChange).toHaveBeenCalledWith("ultra");
  });

  it("keeps the selector hidden until an installed tool is enabled", () => {
    render(
      <ToolModeSelector
        toolId="caveman"
        name="Caveman"
        installed
        enabled={false}
        defaultMode="full"
        supportedModes={["lite", "full", "ultra"]}
        disabled={false}
        onChange={vi.fn()}
      />
    );

    expect(screen.queryByRole("radiogroup")).not.toBeInTheDocument();
    expect(screen.getByText(/Enable this tool to set/i)).toBeInTheDocument();
  });

  it("shows every Caveman prose and Wenyan level", () => {
    render(
      <ToolModeSelector
        toolId="caveman"
        name="Caveman"
        installed
        enabled
        defaultMode="wenyan-full"
        supportedModes={[
          "lite",
          "full",
          "ultra",
          "wenyan-lite",
          "wenyan-full",
          "wenyan-ultra",
        ]}
        disabled={false}
        onChange={vi.fn()}
      />
    );

    const modes = screen.getByRole("radiogroup", { name: "Default mode for Caveman" });
    expect(within(modes).getAllByRole("radio")).toHaveLength(6);
    expect(within(modes).getByRole("radio", { name: "Wenyan Full" })).toBeChecked();
    expect(screen.getByText(/Wenyan variants/i)).toBeInTheDocument();
  });
});
