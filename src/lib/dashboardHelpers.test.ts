import { afterEach, describe, expect, it, vi } from "vitest";
import {
  aggregateClientConnectors,
  buildHourlySavingsChartData,
  buildHourlySavingsWindow,
  buildMonthlySavingsChartData,
  buildMonthlySavingsWindow,
  compactNumber,
  connectorDashboardStatus,
  currency,
  currencyExact,
  dayOfMonthTickFormatter,
  earliestHourlyDay,
  earliestSavingsMonth,
  formatDateTime,
  formatDayKey,
  formatLearnStatus,
  getEnabledSupportedConnectors,
  hasEnabledConnector,
  hourOfDayTickFormatter,
  mergeProviderSavingsForDisplay,
  percent1,
  savingsRate,
  sortClientConnectors
} from "./dashboardHelpers";
import type {
  ClientConnectorStatus,
  DailySavingsPoint,
  HourlySavingsPoint
} from "./types";

describe("dashboard helpers", () => {
  afterEach(() => {
    vi.useRealTimers();
  });

  it("formats stable numeric summaries", () => {
    expect(currencyExact(12.345)).toBe("$12.35");
    expect(currency(9999)).toBe("$9,999");
    expect(currency(15_432)).toContain("K");
    expect(compactNumber(12_345)).toBe("12.3K");
    expect(percent1(18)).toBe("18.0");
  });

  it("computes savings rate against the would-be total", () => {
    expect(savingsRate(15, 85)).toBe(15);
    expect(savingsRate(0, 0)).toBeNull();
    expect(savingsRate(0, 100)).toBe(0);
    expect(savingsRate(-5, 100)).toBe(0);
  });

  it("renders near-zero negatives as clean zero, not -$0", () => {
    expect(currency(-0.003)).toBe("$0");
    expect(currencyExact(-0.003)).toBe("$0.00");
    expect(currency(-1.5)).toBe("-$2"); // genuine negatives still show
  });

  it("builds full monthly windows with zero-filled gaps", () => {
    const data: DailySavingsPoint[] = [
      {
        date: "2024-02-02",
        estimatedSavingsUsd: 2.5,
        estimatedTokensSaved: 250,
        actualCostUsd: 1.5,
        totalTokensSent: 1000
      }
    ];

    const month = new Date(2024, 1, 18);
    const windowed = buildMonthlySavingsWindow(data, month);

    expect(windowed).toHaveLength(29);
    expect(windowed[0]).toEqual({
      date: "2024-02-01",
      estimatedSavingsUsd: 0,
      estimatedTokensSaved: 0,
      actualCostUsd: 0,
      totalTokensSent: 0
    });
    expect(windowed[1]).toEqual(data[0]);
    expect(windowed[28].date).toBe("2024-02-29");
  });

  it("builds hourly windows and chart data with derived totals", () => {
    const data: HourlySavingsPoint[] = [
      {
        hour: "2024-03-05T04:00",
        estimatedSavingsUsd: 1.25,
        estimatedTokensSaved: 125,
        actualCostUsd: 0.75,
        totalTokensSent: 500,
        byProvider: [
          {
            provider: "anthropic",
            estimatedSavingsUsd: 1.25,
            estimatedTokensSaved: 125,
            actualCostUsd: 0.75,
            totalTokensSent: 500
          }
        ]
      }
    ];

    const windowed = buildHourlySavingsWindow(data, new Date(2024, 2, 5, 12));
    const chartData = buildHourlySavingsChartData(windowed);

    expect(windowed).toHaveLength(24);
    expect(windowed[4]).toEqual(data[0]);
    expect(windowed[3].hour).toBe("2024-03-05T03:00");
    expect(chartData[4]).toMatchObject({
      bucketKey: "2024-03-05T04:00",
      estimatedSavingsUsd: 1.25,
      estimatedTokensSaved: 125,
      actualCostUsd: 0.75,
      totalTokensSent: 500,
      totalCostBeforeOptimization: 2,
      totalTokensBeforeOptimization: 625
    });
    // Per-provider breakdown carries through; padded hours default to empty.
    expect(chartData[4].byProvider).toEqual(data[0].byProvider);
    expect(chartData[3].byProvider).toEqual([]);
  });

  it("builds monthly chart data and finds earliest visible history", () => {
    const dailyData: DailySavingsPoint[] = [
      {
        date: "2024-01-30",
        estimatedSavingsUsd: 1,
        estimatedTokensSaved: 100,
        actualCostUsd: 3,
        totalTokensSent: 1000
      },
      {
        date: "2024-03-01",
        estimatedSavingsUsd: 2,
        estimatedTokensSaved: 200,
        actualCostUsd: 4,
        totalTokensSent: 2000
      }
    ];
    const hourlyData: HourlySavingsPoint[] = [
      {
        hour: "2024-02-14T21:00",
        estimatedSavingsUsd: 0.5,
        estimatedTokensSaved: 50,
        actualCostUsd: 1,
        totalTokensSent: 300,
        byProvider: []
      }
    ];

    const chartData = buildMonthlySavingsChartData(dailyData);

    expect(chartData[0]).toMatchObject({
      bucketKey: "2024-01-30",
      totalCostBeforeOptimization: 4,
      totalTokensBeforeOptimization: 1100
    });
    expect(formatDayKey(earliestSavingsMonth(dailyData) as Date)).toBe("2024-01-01");
    expect(formatDayKey(earliestHourlyDay(hourlyData) as Date)).toBe("2024-02-14");
  });

  it("formats chart ticks predictably", () => {
    expect(dayOfMonthTickFormatter("2024-02-01")).toBe("1");
    expect(dayOfMonthTickFormatter("2024-02-02")).toBe("");
    expect(dayOfMonthTickFormatter("2024-02-29")).toBe("29");
    expect(hourOfDayTickFormatter("2024-02-01T04:00")).toBe("04");
    expect(hourOfDayTickFormatter("2024-02-01T05:00")).toBe("");
    expect(hourOfDayTickFormatter("2024-02-01T23:00")).toBe("23");
  });

  it("filters and sorts client connectors", () => {
    const connectors: ClientConnectorStatus[] = [
      { clientId: "zed", name: "Zed", installed: false, enabled: false, verified: false },
      { clientId: "claude_code", name: "Claude Code", installed: true, enabled: true, verified: true },
      { clientId: "cursor", name: "Cursor", installed: true, enabled: false, verified: false }
    ];

    expect(aggregateClientConnectors(connectors)).toEqual([connectors[1]]);
    expect(sortClientConnectors(connectors).map((connector) => connector.clientId)).toEqual([
      "claude_code",
      "cursor",
      "zed"
    ]);
  });

  it("keeps codex, grok_build and opencode alongside claude_code as supported connectors", () => {
    const connectors: ClientConnectorStatus[] = [
      { clientId: "codex", name: "Codex", installed: true, enabled: false, verified: false },
      { clientId: "grok_build", name: "Grok Build", installed: true, enabled: false, verified: false },
      { clientId: "opencode", name: "OpenCode", installed: true, enabled: true, verified: false },
      { clientId: "claude_code", name: "Claude Code", installed: true, enabled: true, verified: true },
      { clientId: "cursor", name: "Cursor", installed: true, enabled: false, verified: false }
    ];

    expect(
      aggregateClientConnectors(connectors).map((connector) => connector.clientId).sort()
    ).toEqual(["claude_code", "codex", "grok_build", "opencode"]);
  });

  it("reports enabled supported connectors regardless of which tool", () => {
    const connectors: ClientConnectorStatus[] = [
      { clientId: "claude_code", name: "Claude Code", installed: true, enabled: false, verified: false },
      { clientId: "codex", name: "Codex", installed: true, enabled: true, verified: true },
      { clientId: "cursor", name: "Cursor", installed: true, enabled: true, verified: true }
    ];

    expect(getEnabledSupportedConnectors(connectors).map((c) => c.clientId)).toEqual(["codex"]);
    expect(hasEnabledConnector(connectors)).toBe(true);
    expect(
      hasEnabledConnector([
        { clientId: "claude_code", name: "Claude Code", installed: true, enabled: false, verified: false }
      ])
    ).toBe(false);
  });

  it("derives a dashboard status label/tone per connector state", () => {
    expect(
      connectorDashboardStatus({ clientId: "codex", name: "Codex", installed: false, enabled: false, verified: false })
    ).toEqual({ label: "Not installed", tone: "off" });
    expect(
      connectorDashboardStatus({ clientId: "codex", name: "Codex", installed: true, enabled: false, verified: false })
    ).toEqual({ label: "Off", tone: "off" });
    expect(
      connectorDashboardStatus({ clientId: "codex", name: "Codex", installed: true, enabled: true, verified: false })
    ).toEqual({ label: "Verifying", tone: "pending" });
    expect(
      connectorDashboardStatus({ clientId: "codex", name: "Codex", installed: true, enabled: true, verified: true })
    ).toEqual({ label: "Active", tone: "active" });
    // Red is reserved for enabled-but-broken: proxy down while a connector is on.
    expect(
      connectorDashboardStatus(
        { clientId: "codex", name: "Codex", installed: true, enabled: true, verified: true },
        { proxyReachable: false }
      )
    ).toEqual({ label: "Proxy unreachable", tone: "idle" });
  });

  it("formats timestamps and learn recency with clear fallbacks", () => {
    vi.useFakeTimers();
    vi.setSystemTime(new Date("2026-03-27T12:00:00Z"));

    expect(formatDateTime(null)).toBe("Never");
    expect(formatDateTime("not-a-date")).toBe("Unknown");
    expect(formatLearnStatus({ lastLearnRanAt: null })).toBe("never scan");
    expect(formatLearnStatus({ lastLearnRanAt: "invalid" })).toBe("never scan");
    expect(formatLearnStatus({ lastLearnRanAt: "2026-03-27T08:00:00Z" })).toBe("last scan: today");
    expect(formatLearnStatus({ lastLearnRanAt: "2026-03-26T08:00:00Z" })).toBe("last scan: yesterday");
    expect(formatLearnStatus({ lastLearnRanAt: "2026-03-22T08:00:00Z" })).toBe("last scan: 5 days ago");
  });
});

describe("mergeProviderSavingsForDisplay", () => {
  it("folds anthropic and unknown into Claude Code, openai into Codex, and xai into Grok Build", () => {
    const merged = mergeProviderSavingsForDisplay([
      {
        provider: "openai",
        estimatedSavingsUsd: 0.04,
        estimatedTokensSaved: 40,
        actualCostUsd: 0.16,
        totalTokensSent: 80
      },
      {
        provider: "anthropic",
        estimatedSavingsUsd: 0.1,
        estimatedTokensSaved: 100,
        actualCostUsd: 0.24,
        totalTokensSent: 120
      },
      {
        provider: "unknown",
        estimatedSavingsUsd: 0.01,
        estimatedTokensSaved: 15,
        actualCostUsd: 0.03,
        totalTokensSent: 20
      },
      {
        provider: "xai",
        estimatedSavingsUsd: 0.02,
        estimatedTokensSaved: 25,
        actualCostUsd: 0.08,
        totalTokensSent: 50
      }
    ]);

    expect(merged).toEqual([
      {
        label: "Claude Code",
        estimatedSavingsUsd: 0.1 + 0.01,
        estimatedTokensSaved: 115,
        actualCostUsd: 0.24 + 0.03,
        totalTokensSent: 140
      },
      {
        label: "Codex",
        estimatedSavingsUsd: 0.04,
        estimatedTokensSaved: 40,
        actualCostUsd: 0.16,
        totalTokensSent: 80
      },
      {
        label: "Grok Build",
        estimatedSavingsUsd: 0.02,
        estimatedTokensSaved: 25,
        actualCostUsd: 0.08,
        totalTokensSent: 50
      }
    ]);
  });

  it("omits a connector with no attributed providers", () => {
    const merged = mergeProviderSavingsForDisplay([
      {
        provider: "anthropic",
        estimatedSavingsUsd: 0.1,
        estimatedTokensSaved: 100,
        actualCostUsd: 0.24,
        totalTokensSent: 120
      }
    ]);

    expect(merged).toHaveLength(1);
    expect(merged[0].label).toBe("Claude Code");
  });

  it("returns nothing for an empty breakdown", () => {
    expect(mergeProviderSavingsForDisplay([])).toEqual([]);
  });

});
