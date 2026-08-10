import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";

import { mockDashboard } from "./mockData";
import type { DashboardState } from "./types";
import {
  __resetSetupStallThrottle,
  evaluateSetupStall,
  maybeFireSetupStallAlert,
  setupStallMinutes,
  SETUP_STALL_AFTER_MS,
} from "./setupHealthAlert";

const { invokeMock, isVisibleMock } = vi.hoisted(() => ({
  invokeMock: vi.fn(),
  isVisibleMock: vi.fn(),
}));

vi.mock("@tauri-apps/api/core", () => ({
  invoke: invokeMock,
}));

vi.mock("@tauri-apps/api/window", () => ({
  getCurrentWindow: () => ({ isVisible: isVisibleMock }),
}));

function installStorage(initial: Record<string, string> = {}) {
  const values = new Map(Object.entries(initial));
  Object.defineProperty(globalThis, "localStorage", {
    configurable: true,
    value: {
      getItem: vi.fn((key: string) => values.get(key) ?? null),
      setItem: vi.fn((key: string, value: string) => {
        values.set(key, value);
      }),
      removeItem: vi.fn((key: string) => {
        values.delete(key);
      }),
    },
  });
  return values;
}

// A healthy, fully installed app that simply has nothing to show yet.
function stalledDashboard(overrides: Partial<DashboardState> = {}): DashboardState {
  return {
    ...mockDashboard,
    bootstrapComplete: true,
    pythonRuntimeInstalled: true,
    lifetimeRequests: 0,
    lifetimeEstimatedSavingsUsd: 0,
    lifetimeEstimatedTokensSaved: 0,
    ...overrides,
  };
}

const PAST_WINDOW = SETUP_STALL_AFTER_MS + 1_000;

beforeEach(() => {
  installStorage();
  invokeMock.mockReset();
  invokeMock.mockResolvedValue(undefined);
  isVisibleMock.mockReset();
  isVisibleMock.mockResolvedValue(false);
});

afterEach(() => {
  vi.useRealTimers();
});

describe("evaluateSetupStall", () => {
  it("stays quiet before the stall window elapses", () => {
    expect(evaluateSetupStall(stalledDashboard(), SETUP_STALL_AFTER_MS - 1)).toBeNull();
  });

  it("reports no_traffic when Headroom has never seen a request", () => {
    const alert = evaluateSetupStall(stalledDashboard(), PAST_WINDOW);
    expect(alert?.kind).toBe("no_traffic");
    expect(alert?.body).toContain(`${setupStallMinutes()} minutes`);
  });

  it("reports no_savings when requests arrive but nothing is optimized", () => {
    const alert = evaluateSetupStall(stalledDashboard({ lifetimeRequests: 42 }), PAST_WINDOW);
    expect(alert?.kind).toBe("no_savings");
    expect(alert?.body).toContain("none are being optimized");
  });

  it("stays quiet while the install is still in progress", () => {
    expect(
      evaluateSetupStall(stalledDashboard({ bootstrapComplete: false }), PAST_WINDOW)
    ).toBeNull();
    expect(
      evaluateSetupStall(stalledDashboard({ pythonRuntimeInstalled: false }), PAST_WINDOW)
    ).toBeNull();
  });

  it("stays quiet while the Terms gate is still blocking the app", () => {
    expect(
      evaluateSetupStall(
        stalledDashboard({ requiredTermsVersion: 2, acceptedTermsVersion: 1 }),
        PAST_WINDOW
      )
    ).toBeNull();
  });

  it("stays quiet when the account gate has optimization switched off", () => {
    expect(
      evaluateSetupStall(stalledDashboard(), PAST_WINDOW, { optimizationBlocked: true })
    ).toBeNull();
    // Unknown gate state (pricing status not loaded yet) must not suppress it.
    expect(
      evaluateSetupStall(stalledDashboard(), PAST_WINDOW, { optimizationBlocked: undefined })
    ).not.toBeNull();
  });

  it("stays quiet once any savings are on record", () => {
    expect(
      evaluateSetupStall(stalledDashboard({ lifetimeEstimatedTokensSaved: 10 }), PAST_WINDOW)
    ).toBeNull();
    // Sub-cent savings still round to 0 tokens on some paths, so the USD field
    // has to retire the alert on its own.
    expect(
      evaluateSetupStall(stalledDashboard({ lifetimeEstimatedSavingsUsd: 0.004 }), PAST_WINDOW)
    ).toBeNull();
  });
});

describe("maybeFireSetupStallAlert", () => {
  it("sends a native notification and returns the alert when the tray is hidden", async () => {
    const alert = await maybeFireSetupStallAlert(stalledDashboard(), PAST_WINDOW);

    expect(alert?.kind).toBe("no_traffic");
    expect(invokeMock).toHaveBeenCalledWith("show_notification", {
      title: alert?.title,
      body: alert?.body,
      action: "setup",
    });
  });

  it("skips the native notification when the tray window is already open", async () => {
    isVisibleMock.mockResolvedValue(true);

    const alert = await maybeFireSetupStallAlert(stalledDashboard(), PAST_WINDOW);

    expect(alert).not.toBeNull();
    expect(invokeMock).not.toHaveBeenCalled();
  });

  it("fires at most once per local day", async () => {
    expect(await maybeFireSetupStallAlert(stalledDashboard(), PAST_WINDOW)).not.toBeNull();
    expect(await maybeFireSetupStallAlert(stalledDashboard(), PAST_WINDOW)).toBeNull();
    expect(invokeMock).toHaveBeenCalledTimes(1);
  });

  it("fires again the next local day while savings are still zero", async () => {
    vi.useFakeTimers();
    vi.setSystemTime(new Date(2026, 0, 5, 9, 0, 0));
    expect(await maybeFireSetupStallAlert(stalledDashboard(), PAST_WINDOW)).not.toBeNull();

    vi.setSystemTime(new Date(2026, 0, 6, 9, 0, 0));
    expect(await maybeFireSetupStallAlert(stalledDashboard(), PAST_WINDOW)).not.toBeNull();
    expect(invokeMock).toHaveBeenCalledTimes(2);
  });

  it("consumes the day slot even if the notification call fails", async () => {
    invokeMock.mockRejectedValue(new Error("notification center unavailable"));

    expect(await maybeFireSetupStallAlert(stalledDashboard(), PAST_WINDOW)).not.toBeNull();
    expect(await maybeFireSetupStallAlert(stalledDashboard(), PAST_WINDOW)).toBeNull();
  });

  it("does not consume the day slot when the account gate is the cause", async () => {
    expect(
      await maybeFireSetupStallAlert(stalledDashboard(), PAST_WINDOW, {
        optimizationBlocked: true,
      })
    ).toBeNull();
    expect(invokeMock).not.toHaveBeenCalled();
    // The slot is still available once the gate clears.
    expect(await maybeFireSetupStallAlert(stalledDashboard(), PAST_WINDOW)).not.toBeNull();
  });

  it("returns null without touching storage when the stall isn't due", async () => {
    expect(await maybeFireSetupStallAlert(stalledDashboard(), 1_000)).toBeNull();
    expect(localStorage.setItem).not.toHaveBeenCalled();
  });

  it("treats an unavailable localStorage as unthrottled rather than crashing", async () => {
    Object.defineProperty(globalThis, "localStorage", {
      configurable: true,
      value: {
        getItem: () => {
          throw new Error("blocked");
        },
        setItem: () => {
          throw new Error("blocked");
        },
        removeItem: () => {
          throw new Error("blocked");
        },
      },
    });

    expect(await maybeFireSetupStallAlert(stalledDashboard(), PAST_WINDOW)).not.toBeNull();
    expect(() => __resetSetupStallThrottle()).not.toThrow();
  });

  it("re-arms after the throttle is reset", async () => {
    expect(await maybeFireSetupStallAlert(stalledDashboard(), PAST_WINDOW)).not.toBeNull();
    __resetSetupStallThrottle();
    expect(await maybeFireSetupStallAlert(stalledDashboard(), PAST_WINDOW)).not.toBeNull();
  });

  it("falls back to hidden when the visibility probe rejects", async () => {
    isVisibleMock.mockRejectedValue(new Error("no window"));

    expect(await maybeFireSetupStallAlert(stalledDashboard(), PAST_WINDOW)).not.toBeNull();
    expect(invokeMock).toHaveBeenCalledTimes(1);
  });
});
