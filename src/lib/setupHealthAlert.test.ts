import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";

import { mockDashboard } from "./mockData";
import type { ClientConnectorStatus, DashboardState } from "./types";
import {
  __resetSetupStallThrottle,
  evaluateSetupStall,
  maybeFireSetupStallAlert,
  setupStallNoTrafficMinutes,
  SETUP_STALL_NO_SAVINGS_AFTER_MS,
  SETUP_STALL_NO_SAVINGS_MIN_REQUESTS,
  SETUP_STALL_NO_TRAFFIC_AFTER_MS,
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

// Enough traffic to rule out a quiet start, so only the savings are missing.
function busyDashboard(overrides: Partial<DashboardState> = {}): DashboardState {
  return stalledDashboard({
    lifetimeRequests: SETUP_STALL_NO_SAVINGS_MIN_REQUESTS,
    ...overrides,
  });
}

function connector(overrides: Partial<ClientConnectorStatus> = {}): ClientConnectorStatus {
  return {
    clientId: "claude_code",
    name: "Claude Code",
    installed: true,
    enabled: true,
    verified: false,
    ...overrides,
  };
}

// A connector that was configured but has never seen traffic come back. This is
// what makes the no-traffic branch fire at all.
const UNVERIFIED = [connector()];

const PAST_WINDOW = SETUP_STALL_NO_TRAFFIC_AFTER_MS + 1_000;

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
  it("stays quiet before either branch's window elapses", () => {
    expect(
      evaluateSetupStall(busyDashboard(), SETUP_STALL_NO_SAVINGS_AFTER_MS - 1, {
        connectors: UNVERIFIED,
      })
    ).toBeNull();
  });

  it("stays quiet while the install is still in progress", () => {
    expect(
      evaluateSetupStall(busyDashboard({ bootstrapComplete: false }), PAST_WINDOW)
    ).toBeNull();
    expect(
      evaluateSetupStall(busyDashboard({ pythonRuntimeInstalled: false }), PAST_WINDOW)
    ).toBeNull();
  });

  it("stays quiet while the Terms gate is still blocking the app", () => {
    expect(
      evaluateSetupStall(
        busyDashboard({ requiredTermsVersion: 2, acceptedTermsVersion: 1 }),
        PAST_WINDOW
      )
    ).toBeNull();
  });

  it("stays quiet when the account gate has optimization switched off", () => {
    expect(
      evaluateSetupStall(busyDashboard(), PAST_WINDOW, { optimizationBlocked: true })
    ).toBeNull();
    // Unknown gate state (pricing status not loaded yet) must not suppress it.
    expect(
      evaluateSetupStall(busyDashboard(), PAST_WINDOW, { optimizationBlocked: undefined })
    ).not.toBeNull();
  });

  it("stays quiet once any savings are on record", () => {
    expect(
      evaluateSetupStall(busyDashboard({ lifetimeEstimatedTokensSaved: 10 }), PAST_WINDOW)
    ).toBeNull();
    // Sub-cent savings still round to 0 tokens on some paths, so the USD field
    // has to retire the alert on its own.
    expect(
      evaluateSetupStall(busyDashboard({ lifetimeEstimatedSavingsUsd: 0.004 }), PAST_WINDOW)
    ).toBeNull();
  });

  describe("no_traffic branch", () => {
    it("fires once a configured connector has never seen traffic come back", () => {
      const alert = evaluateSetupStall(stalledDashboard(), PAST_WINDOW, {
        connectors: UNVERIFIED,
      });
      expect(alert?.kind).toBe("no_traffic");
      expect(alert?.body).toContain(`${setupStallNoTrafficMinutes()} minutes`);
    });

    it("holds the full no-traffic window even though no_savings fires sooner", () => {
      expect(
        evaluateSetupStall(stalledDashboard(), SETUP_STALL_NO_TRAFFIC_AFTER_MS - 1, {
          connectors: UNVERIFIED,
        })
      ).toBeNull();
    });

    // Uptime is not the same as time spent coding, so the clock alone must
    // never be enough to accuse the setup.
    it("stays quiet when nothing is connected at all", () => {
      expect(evaluateSetupStall(stalledDashboard(), PAST_WINDOW, { connectors: [] })).toBeNull();
    });

    it("stays quiet when connector status hasn't loaded yet", () => {
      expect(evaluateSetupStall(stalledDashboard(), PAST_WINDOW, {})).toBeNull();
    });

    it("stays quiet when the connector is already verified or switched off", () => {
      expect(
        evaluateSetupStall(stalledDashboard(), PAST_WINDOW, {
          connectors: [connector({ verified: true })],
        })
      ).toBeNull();
      expect(
        evaluateSetupStall(stalledDashboard(), PAST_WINDOW, {
          connectors: [connector({ enabled: false })],
        })
      ).toBeNull();
      expect(
        evaluateSetupStall(stalledDashboard(), PAST_WINDOW, {
          connectors: [connector({ installed: false })],
        })
      ).toBeNull();
    });
  });

  describe("forceKind test override", () => {
    it("fires immediately regardless of uptime, savings, gate or connectors", () => {
      const healthy = stalledDashboard({
        lifetimeEstimatedTokensSaved: 5_000_000,
        lifetimeEstimatedSavingsUsd: 42,
        lifetimeRequests: 900,
      });
      const alert = evaluateSetupStall(healthy, 0, {
        forceKind: "no_traffic",
        optimizationBlocked: true,
        connectors: [],
      });

      expect(alert?.kind).toBe("no_traffic");
    });

    it("honours the requested branch", () => {
      expect(
        evaluateSetupStall(stalledDashboard(), 0, { forceKind: "no_savings" })?.kind
      ).toBe("no_savings");
    });

    // Eyeballing the modal must not silence the real alert for the rest of the
    // day, so a forced fire neither reads nor writes the throttle key.
    it("never consumes the once-per-day slot", async () => {
      expect(
        await maybeFireSetupStallAlert(stalledDashboard(), 0, { forceKind: "no_traffic" })
      ).not.toBeNull();
      expect(
        await maybeFireSetupStallAlert(stalledDashboard(), 0, { forceKind: "no_traffic" })
      ).not.toBeNull();
      expect(localStorage.setItem).not.toHaveBeenCalled();

      // The genuine alert still has its slot available afterwards.
      expect(await maybeFireSetupStallAlert(busyDashboard(), PAST_WINDOW)).not.toBeNull();
    });
  });

  describe("no_savings branch", () => {
    it("fires on request volume rather than waiting out the no-traffic clock", () => {
      const alert = evaluateSetupStall(busyDashboard(), SETUP_STALL_NO_SAVINGS_AFTER_MS);
      expect(alert?.kind).toBe("no_savings");
      expect(alert?.body).toContain("none are being optimized");
    });

    it("needs no connector context, since the traffic itself is the evidence", () => {
      expect(evaluateSetupStall(busyDashboard(), PAST_WINDOW, { connectors: [] })).not.toBeNull();
    });

    it("waits for enough requests to rule out a quiet start", () => {
      expect(
        evaluateSetupStall(
          busyDashboard({ lifetimeRequests: SETUP_STALL_NO_SAVINGS_MIN_REQUESTS - 1 }),
          PAST_WINDOW
        )
      ).toBeNull();
    });
  });
});

describe("maybeFireSetupStallAlert", () => {
  it("sends a native notification and returns the alert when the tray is hidden", async () => {
    const alert = await maybeFireSetupStallAlert(busyDashboard(), PAST_WINDOW);

    expect(alert?.kind).toBe("no_savings");
    expect(invokeMock).toHaveBeenCalledWith("show_notification", {
      title: alert?.title,
      body: alert?.body,
      action: "setup",
    });
  });

  it("carries the no_traffic branch through the same path", async () => {
    const alert = await maybeFireSetupStallAlert(stalledDashboard(), PAST_WINDOW, {
      connectors: UNVERIFIED,
    });

    expect(alert?.kind).toBe("no_traffic");
    expect(invokeMock).toHaveBeenCalledTimes(1);
  });

  it("skips the native notification when the tray window is already open", async () => {
    isVisibleMock.mockResolvedValue(true);

    const alert = await maybeFireSetupStallAlert(busyDashboard(), PAST_WINDOW);

    expect(alert).not.toBeNull();
    expect(invokeMock).not.toHaveBeenCalled();
  });

  it("fires at most once per local day", async () => {
    expect(await maybeFireSetupStallAlert(busyDashboard(), PAST_WINDOW)).not.toBeNull();
    expect(await maybeFireSetupStallAlert(busyDashboard(), PAST_WINDOW)).toBeNull();
    expect(invokeMock).toHaveBeenCalledTimes(1);
  });

  it("fires again the next local day while savings are still zero", async () => {
    vi.useFakeTimers();
    vi.setSystemTime(new Date(2026, 0, 5, 9, 0, 0));
    expect(await maybeFireSetupStallAlert(busyDashboard(), PAST_WINDOW)).not.toBeNull();

    vi.setSystemTime(new Date(2026, 0, 6, 9, 0, 0));
    expect(await maybeFireSetupStallAlert(busyDashboard(), PAST_WINDOW)).not.toBeNull();
    expect(invokeMock).toHaveBeenCalledTimes(2);
  });

  it("consumes the day slot even if the notification call fails", async () => {
    invokeMock.mockRejectedValue(new Error("notification center unavailable"));

    expect(await maybeFireSetupStallAlert(busyDashboard(), PAST_WINDOW)).not.toBeNull();
    expect(await maybeFireSetupStallAlert(busyDashboard(), PAST_WINDOW)).toBeNull();
  });

  it("does not consume the day slot when the account gate is the cause", async () => {
    expect(
      await maybeFireSetupStallAlert(busyDashboard(), PAST_WINDOW, {
        optimizationBlocked: true,
      })
    ).toBeNull();
    expect(invokeMock).not.toHaveBeenCalled();
    // The slot is still available once the gate clears.
    expect(await maybeFireSetupStallAlert(busyDashboard(), PAST_WINDOW)).not.toBeNull();
  });

  it("does not consume the day slot when the user simply hasn't started coding", async () => {
    expect(await maybeFireSetupStallAlert(stalledDashboard(), PAST_WINDOW)).toBeNull();
    expect(invokeMock).not.toHaveBeenCalled();
    expect(
      await maybeFireSetupStallAlert(stalledDashboard(), PAST_WINDOW, {
        connectors: UNVERIFIED,
      })
    ).not.toBeNull();
  });

  it("returns null without touching storage when the stall isn't due", async () => {
    expect(await maybeFireSetupStallAlert(busyDashboard(), 1_000)).toBeNull();
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

    expect(await maybeFireSetupStallAlert(busyDashboard(), PAST_WINDOW)).not.toBeNull();
    expect(() => __resetSetupStallThrottle()).not.toThrow();
  });

  it("re-arms after the throttle is reset", async () => {
    expect(await maybeFireSetupStallAlert(busyDashboard(), PAST_WINDOW)).not.toBeNull();
    __resetSetupStallThrottle();
    expect(await maybeFireSetupStallAlert(busyDashboard(), PAST_WINDOW)).not.toBeNull();
  });

  it("falls back to hidden when the visibility probe rejects", async () => {
    isVisibleMock.mockRejectedValue(new Error("no window"));

    expect(await maybeFireSetupStallAlert(busyDashboard(), PAST_WINDOW)).not.toBeNull();
    expect(invokeMock).toHaveBeenCalledTimes(1);
  });
});
