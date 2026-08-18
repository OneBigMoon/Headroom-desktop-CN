import { act, render } from "@testing-library/react";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import { LearnScanStatusLine } from "./LearnScanStatusLine";

function visibleText(container: HTMLElement): string {
  return container.querySelector('[aria-hidden="true"]')?.textContent ?? "";
}

describe("LearnScanStatusLine", () => {
  beforeEach(() => {
    vi.useFakeTimers();
  });
  afterEach(() => {
    vi.useRealTimers();
  });

  it("types the step out character by character with the timer suffix", () => {
    const { container } = render(
      <LearnScanStatusLine elapsedSeconds={12} step="Reading sessions" />
    );

    expect(visibleText(container)).toBe(" · 12s");
    act(() => {
      vi.advanceTimersByTime(18 * 4);
    });
    expect(visibleText(container)).toBe("Read · 12s");
    act(() => {
      vi.advanceTimersByTime(18 * 100);
    });
    expect(visibleText(container)).toBe("Reading sessions · 12s");
  });

  it("rotates through analysis phrases while the analyzing step is active", () => {
    const { container } = render(
      <LearnScanStatusLine step="Analyzing with Claude Code" />
    );

    act(() => {
      vi.advanceTimersByTime(18 * 100);
    });
    expect(visibleText(container)).toBe("Analyzing with Claude Code");

    act(() => {
      vi.advanceTimersByTime(7000);
    });
    act(() => {
      vi.advanceTimersByTime(18 * 100);
    });
    expect(visibleText(container)).toBe("Reading through your sessions");

    // A full cycle returns to the real step so the backend name resurfaces.
    act(() => {
      vi.advanceTimersByTime(7000 * 6);
    });
    act(() => {
      vi.advanceTimersByTime(18 * 100);
    });
    expect(visibleText(container)).toBe("Analyzing with Claude Code");
  });

  it("does not rotate on non-analyzing steps and exposes the full phrase to aria-live", () => {
    const { container } = render(<LearnScanStatusLine step="Found 7 patterns" />);

    act(() => {
      vi.advanceTimersByTime(7000 + 18 * 100);
    });
    expect(visibleText(container)).toBe("Found 7 patterns");
    expect(container.querySelector(".visually-hidden")?.textContent).toBe(
      "Found 7 patterns"
    );
  });

  it("falls back to the scanning label before the first step arrives", () => {
    const { container } = render(<LearnScanStatusLine step={null} />);

    act(() => {
      vi.advanceTimersByTime(18 * 100);
    });
    expect(visibleText(container)).toBe("Scanning sessions");
  });
});
