import { describe, expect, it, vi } from "vitest";
import { render, screen } from "@testing-library/react";
import userEvent from "@testing-library/user-event";

import { SetupStallModal } from "./SetupStallModal";
import { setupStallNoTrafficMinutes } from "../lib/setupHealthAlert";

describe("SetupStallModal", () => {
  it("tells a no-traffic user to restart their agent", () => {
    render(<SetupStallModal kind="no_traffic" onClose={vi.fn()} onOpenSettings={vi.fn()} />);

    expect(
      screen.getByText(new RegExp(`${setupStallNoTrafficMinutes()} minutes`))
    ).toBeInTheDocument();
    expect(
      screen.getByText(/Quit your terminal, editor, or coding agent completely/i)
    ).toBeInTheDocument();
  });

  // The restart-your-terminal steps are actively wrong for someone whose only
  // Claude Code is the one inside the Claude desktop app.
  it("warns the no-traffic user about Claude Code in the Claude desktop app", () => {
    render(<SetupStallModal kind="no_traffic" onClose={vi.fn()} onOpenSettings={vi.fn()} />);

    expect(screen.getByText(/Claude desktop app/i)).toBeInTheDocument();
    expect(screen.getByText(/can't be optimized/i)).toBeInTheDocument();
    // Naming whose limitation this is is the point: without it the note reads
    // as Headroom being broken.
    expect(screen.getByText(/design decision on Anthropic's side/i)).toBeInTheDocument();
  });

  it("leaves that warning off the no-savings branch, where traffic already flows", () => {
    render(<SetupStallModal kind="no_savings" onClose={vi.fn()} onOpenSettings={vi.fn()} />);

    expect(screen.queryByText(/Claude desktop app/i)).not.toBeInTheDocument();
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
