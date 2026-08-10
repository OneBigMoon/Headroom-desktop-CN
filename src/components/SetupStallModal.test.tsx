import { describe, expect, it, vi } from "vitest";
import { render, screen } from "@testing-library/react";
import userEvent from "@testing-library/user-event";

import { SetupStallModal } from "./SetupStallModal";
import { setupStallMinutes } from "../lib/setupHealthAlert";

describe("SetupStallModal", () => {
  it("tells a no-traffic user to restart their agent", () => {
    render(<SetupStallModal kind="no_traffic" onClose={vi.fn()} onOpenSettings={vi.fn()} />);

    expect(
      screen.getByText(new RegExp(`${setupStallMinutes()} minutes`))
    ).toBeInTheDocument();
    expect(
      screen.getByText(/Restart your terminal, editor, or coding agent/i)
    ).toBeInTheDocument();
  });

  it("points a no-savings user at the optimization controls instead", () => {
    render(<SetupStallModal kind="no_savings" onClose={vi.fn()} onOpenSettings={vi.fn()} />);

    expect(screen.getByText(/none of them have been optimized/i)).toBeInTheDocument();
    expect(screen.getByText(/optimization is not paused/i)).toBeInTheDocument();
  });

  it("dismisses via the Dismiss button", async () => {
    const onClose = vi.fn();
    render(<SetupStallModal kind="no_traffic" onClose={onClose} onOpenSettings={vi.fn()} />);

    await userEvent.click(screen.getByRole("button", { name: "Dismiss" }));

    expect(onClose).toHaveBeenCalledTimes(1);
  });

  it("dismisses when the backdrop is clicked but not the card itself", async () => {
    const onClose = vi.fn();
    render(<SetupStallModal kind="no_traffic" onClose={onClose} onOpenSettings={vi.fn()} />);

    await userEvent.click(screen.getByRole("heading", { level: 3 }));
    expect(onClose).not.toHaveBeenCalled();

    await userEvent.click(screen.getByRole("dialog"));
    expect(onClose).toHaveBeenCalledTimes(1);
  });

  it("hands the user to settings", async () => {
    const onOpenSettings = vi.fn();
    render(
      <SetupStallModal kind="no_savings" onClose={vi.fn()} onOpenSettings={onOpenSettings} />
    );

    await userEvent.click(screen.getByRole("button", { name: "Open settings" }));

    expect(onOpenSettings).toHaveBeenCalledTimes(1);
  });
});
