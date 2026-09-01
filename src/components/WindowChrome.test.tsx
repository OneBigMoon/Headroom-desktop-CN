import { beforeEach, describe, expect, it, vi } from "vitest";
import { fireEvent, render, screen, within } from "@testing-library/react";
import userEvent from "@testing-library/user-event";

const hideMock = vi.fn();
const minimizeMock = vi.fn();
const startDraggingMock = vi.fn();
const toggleMaximizeMock = vi.fn();

vi.mock("@tauri-apps/api/window", () => ({
  getCurrentWindow: () => ({
    hide: hideMock,
    minimize: minimizeMock,
    startDragging: startDraggingMock,
    toggleMaximize: toggleMaximizeMock,
  }),
}));

import { WindowChrome } from "./WindowChrome";

describe("WindowChrome", () => {
  beforeEach(() => {
    hideMock.mockReset().mockResolvedValue(undefined);
    minimizeMock.mockReset().mockResolvedValue(undefined);
    startDraggingMock.mockReset().mockResolvedValue(undefined);
    toggleMaximizeMock.mockReset().mockResolvedValue(undefined);
  });

  it("starts a native drag from the non-interactive title-bar surface", () => {
    render(<WindowChrome platform="macos" title="Headroom · Home" />);

    fireEvent.mouseDown(screen.getByText("Headroom · Home"), { button: 0 });

    expect(startDraggingMock).toHaveBeenCalledTimes(1);
  });

  it("does not start dragging from an interactive window control", () => {
    render(<WindowChrome platform="macos" />);

    fireEvent.mouseDown(screen.getByRole("button", { name: "Minimize window" }), {
      button: 0,
    });

    expect(startDraggingMock).not.toHaveBeenCalled();
  });

  it("wires hide, minimize, and maximize controls to the current window", async () => {
    const user = userEvent.setup();
    render(<WindowChrome platform="macos" />);

    await user.click(screen.getByRole("button", { name: "Hide window" }));
    await user.click(screen.getByRole("button", { name: "Minimize window" }));
    await user.click(
      screen.getByRole("button", { name: "Maximize or restore window" })
    );

    expect(hideMock).toHaveBeenCalledTimes(1);
    expect(minimizeMock).toHaveBeenCalledTimes(1);
    expect(toggleMaximizeMock).toHaveBeenCalledTimes(1);
  });

  it("renders macOS controls in keyboard order", () => {
    render(<WindowChrome platform="macos" />);

    const controls = screen.getByText("Headroom").previousElementSibling;
    expect(controls).not.toBeNull();
    expect(
      within(controls as HTMLElement)
        .getAllByRole("button")
        .map((button) => button.getAttribute("aria-label"))
    ).toEqual(["Hide window", "Minimize window", "Maximize or restore window"]);
  });

  it("renders Windows controls in keyboard order", () => {
    render(<WindowChrome platform="windows" />);

    const controls = screen.getByText("Headroom").previousElementSibling;
    expect(controls).not.toBeNull();
    expect(
      within(controls as HTMLElement)
        .getAllByRole("button")
        .map((button) => button.getAttribute("aria-label"))
    ).toEqual(["Minimize window", "Maximize or restore window", "Close window"]);
  });
});
