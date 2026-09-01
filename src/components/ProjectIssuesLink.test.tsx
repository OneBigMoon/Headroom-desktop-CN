import { afterEach, describe, expect, it, vi } from "vitest";
import { render, screen } from "@testing-library/react";
import userEvent from "@testing-library/user-event";

import { PROJECT_ISSUES_URL, ProjectIssuesLink } from "./ProjectIssuesLink";

describe("ProjectIssuesLink", () => {
  afterEach(() => {
    vi.restoreAllMocks();
  });

  it("opens the project's new-issue page", async () => {
    const onOpen = vi.fn();
    render(<ProjectIssuesLink onOpen={onOpen}>Request an add-on</ProjectIssuesLink>);

    await userEvent.click(screen.getByRole("button", { name: "Request an add-on" }));

    expect(onOpen).toHaveBeenCalledWith(PROJECT_ISSUES_URL);
  });

  it("reports a rejected open without leaving an unhandled promise", async () => {
    const error = new Error("shell unavailable");
    const onOpen = vi.fn().mockRejectedValue(error);
    const consoleError = vi.spyOn(console, "error").mockImplementation(() => {});
    render(<ProjectIssuesLink onOpen={onOpen}>Request an add-on</ProjectIssuesLink>);

    await userEvent.click(screen.getByRole("button", { name: "Request an add-on" }));

    expect(await screen.findByRole("alert")).toHaveTextContent(
      "Could not open GitHub Issues. Please try again."
    );
    expect(consoleError).toHaveBeenCalledWith("Failed to open project issues", error);
  });

  it("reports a synchronous open failure", async () => {
    const error = new Error("invalid shell state");
    const onOpen = vi.fn(() => {
      throw error;
    });
    const consoleError = vi.spyOn(console, "error").mockImplementation(() => {});
    render(<ProjectIssuesLink onOpen={onOpen}>Request an add-on</ProjectIssuesLink>);

    await userEvent.click(screen.getByRole("button", { name: "Request an add-on" }));

    expect(await screen.findByRole("alert")).toHaveTextContent(
      "Could not open GitHub Issues. Please try again."
    );
    expect(consoleError).toHaveBeenCalledWith("Failed to open project issues", error);
  });
});
