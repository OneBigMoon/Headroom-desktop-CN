import { act, render } from "@testing-library/react";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import { LearnScanStatusLine } from "./LearnScanStatusLine";

function typedText(container: HTMLElement): string {
  return container.querySelector(".learn-scan-status__text")?.textContent ?? "";
}

describe("LearnScanStatusLine", () => {
  beforeEach(() => {
    vi.useFakeTimers();
  });
  afterEach(() => {
    vi.useRealTimers();
  });

  it("types the step out character by character with the timer pinned throughout", () => {
    const { container } = render(
      <LearnScanStatusLine elapsedSeconds={12} step="Reading sessions" />
    );

    // The timer never rides the typing edge: present from the first frame.
    expect(container.querySelector(".learn-scan-status__timer")?.textContent).toBe("12s");
    expect(typedText(container)).toBe("");
    act(() => {
      vi.advanceTimersByTime(18 * 4);
    });
    expect(typedText(container)).toBe("Read");
    act(() => {
      vi.advanceTimersByTime(18 * 100);
    });
    expect(typedText(container)).toBe("Reading sessions");
    expect(container.querySelector(".learn-scan-status__timer")?.textContent).toBe("12s");
  });

  it("omits the timer when no elapsed time is known", () => {
    const { container } = render(<LearnScanStatusLine step="Reading sessions" />);

    expect(container.querySelector(".learn-scan-status__timer")).toBeNull();
  });

  it("reserves the width of the longest phrase so siblings never reflow", () => {
    const { container } = render(
      <LearnScanStatusLine elapsedSeconds={12} step="Reading sessions" />
    );

    const box = container.querySelector<HTMLElement>(".learn-scan-status");
    expect(box?.style.minWidth).toMatch(/^\d+ch$/);
    const reserved = parseInt(box?.style.minWidth ?? "0", 10);
    expect(reserved).toBeGreaterThanOrEqual("Distilling durable patterns".length);
  });

  it("rotates through analysis phrases while the analyzing step is active", () => {
    const { container } = render(
      <LearnScanStatusLine step="Analyzing with Claude Code" />
    );

    act(() => {
      vi.advanceTimersByTime(18 * 100);
    });
    expect(typedText(container)).toBe("Analyzing with Claude Code");

    act(() => {
      vi.advanceTimersByTime(7000);
    });
    act(() => {
      vi.advanceTimersByTime(18 * 100);
    });
    expect(typedText(container)).toBe("Reading your sessions");

    // A full cycle returns to the real step so the backend name resurfaces.
    act(() => {
      vi.advanceTimersByTime(7000 * 6);
    });
    act(() => {
      vi.advanceTimersByTime(18 * 100);
    });
    expect(typedText(container)).toBe("Analyzing with Claude Code");
  });

  it("does not rotate on non-analyzing steps and exposes the full phrase to aria-live", () => {
    const { container } = render(<LearnScanStatusLine step="Found 7 patterns" />);

    act(() => {
      vi.advanceTimersByTime(7000 + 18 * 100);
    });
    expect(typedText(container)).toBe("Found 7 patterns");
    expect(container.querySelector(".visually-hidden")?.textContent).toBe(
      "Found 7 patterns"
    );
  });

  it("falls back to the scanning label before the first step arrives", () => {
    const { container } = render(<LearnScanStatusLine step={null} />);

    act(() => {
      vi.advanceTimersByTime(18 * 100);
    });
    expect(typedText(container)).toBe("Scanning sessions");
  });
});
