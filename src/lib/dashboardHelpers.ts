import type {
  ClientConnectorStatus,
  DailySavingsPoint,
  HourlySavingsPoint,
  ProviderSavingsPoint
} from "./types";

export interface SavingsChartDatum {
  bucketKey: string;
  bucketLabel: string;
  estimatedSavingsUsd: number;
  estimatedTokensSaved: number;
  // Full bucket spend, cache reads included. Kept for the per-provider
  // pro-rating in the tooltip; the bars plot the compressible figures below.
  actualCostUsd: number;
  totalTokensSent: number;
  // What the bars plot: spend with the provider cache-read portion removed.
  // See `compressibleSpend`.
  compressibleCostUsd: number;
  compressibleTokensSent: number;
  // Output-shaping savings, stacked on top of compression in the chart. Zero
  // for buckets predating the layer, so the bar simply shows one segment.
  outputSavingsUsd: number;
  outputTokensSaved: number;
  totalCostBeforeOptimization: number;
  totalTokensBeforeOptimization: number;
  // Per-provider attribution, only populated for hourly buckets (day view).
  // Undefined for monthly buckets, which have no provider dimension.
  byProvider?: ProviderSavingsPoint[];
}

export function currencyExact(value: number) {
  // Avoid "-$0.00" from tiny negatives that round to zero at 2 decimals.
  if (value > -0.005 && value <= 0) value = 0;
  return new Intl.NumberFormat("en-US", {
    style: "currency",
    currency: "USD",
    minimumFractionDigits: 2,
    maximumFractionDigits: 2
  }).format(value);
}

export function currency(value: number) {
  // Avoid "-$0" from tiny negatives that round to zero at 0 decimals.
  if (value > -0.5 && value <= 0) value = 0;
  if (value >= 10_000) {
    return new Intl.NumberFormat("en-US", {
      style: "currency",
      currency: "USD",
      notation: "compact",
      maximumFractionDigits: 1
    }).format(value);
  }
  return new Intl.NumberFormat("en-US", {
    style: "currency",
    currency: "USD",
    maximumFractionDigits: 0
  }).format(value);
}

export function compactNumber(value: number) {
  return new Intl.NumberFormat("en-US", {
    notation: "compact",
    maximumFractionDigits: 1
  }).format(value);
}

export function percent1(value: number) {
  return new Intl.NumberFormat("en-US", {
    minimumFractionDigits: 1,
    maximumFractionDigits: 1
  }).format(value);
}

/** Share of the would-be total that was saved, as a whole percent. Null when there is nothing to compare. */
export function savingsRate(saved: number, spent: number) {
  const baseline = Math.max(0, saved) + Math.max(0, spent);
  if (baseline <= 0) return null;
  return Math.round((Math.max(0, saved) / baseline) * 100);
}

/** Cache-hit / compression pair over a window of savings buckets.
 *
 * Only buckets that carry cache data count (backend rollup coverage; local
 * tracker buckets and days aged out of history retention have none), so both
 * rates describe the same covered slice of the window. Null when no bucket in
 * the window has coverage. `hitPct` is the share of forwarded input served
 * from the provider's prompt cache; `compressedPct` is the share of the
 * REMAINING (compressible) input Headroom removed.
 *
 * Priced in dollars for the reason documented on
 * `compressibleInputSavingsRate` below: cacheReadTokens (provider count) and
 * totalTokensSent (our tokenizer) must never be differenced or ratioed, and
 * on real data reads exceed forwarded input, which pinned the token form of
 * this pair at a meaningless "100% hits / 100% compressed". The read
 * discount (`cacheSavingsUsd`) and the bucket's input cost come from one
 * pricing function: reads bill at ~0.1x, so read cost = discount / 9 and the
 * reads' full-price value = discount * 10/9. */
export function cacheHitPair(
  points: Array<{
    cacheSavingsUsd?: number | null;
    actualCostUsd: number;
    estimatedSavingsUsd: number;
  }>
) {
  let cacheSavings = 0;
  let actual = 0;
  let saved = 0;
  for (const point of points) {
    if (point.cacheSavingsUsd == null) continue;
    cacheSavings += Math.max(0, point.cacheSavingsUsd);
    actual += Math.max(0, point.actualCostUsd);
    saved += Math.max(0, point.estimatedSavingsUsd);
  }
  // Full-price value of the window's input: what was paid plus the discount
  // the cache earned.
  const fullPriceInput = actual + cacheSavings;
  if (fullPriceInput <= 0) return null;
  const hitPct = Math.min(100, (((cacheSavings * 10) / 9) / fullPriceInput) * 100);
  // What survived the cache and was paid at full input price.
  const rest = Math.max(0, actual - cacheSavings / 9);
  const baseline = saved + rest;
  // A fully-cached window leaves nothing to compress: report 0% of an empty
  // remainder rather than hiding the (excellent) hit rate.
  const compressedPct = baseline > 0 ? Math.min(100, (saved / baseline) * 100) : 0;
  return { hitPct, compressedPct };
}

/** Canonical input-compression rate for a window of buckets: the share of the
 * COMPRESSIBLE input spend Headroom removed. Cache reads are excluded from the
 * denominator (they bill at ~0.1x and Headroom deliberately never touches the
 * cached prefix); output shaping is never part of this rate.
 *
 * Always priced in dollars, whatever unit the chart is showing, because only
 * the dollar figures are on one scale. `totalTokensSent` is our own tokenizer's
 * count of the forwarded prompt while `cacheReadTokens` is the provider's own
 * count of part of it -- upstream keeps them apart on purpose ("must never be
 * differenced", proxy/outcome.py) and on 2026-08-14 a day's derived cache reads
 * (195.8M) genuinely exceeded its forwarded input (163.4M), pinning the token
 * form of this rate at a meaningless 100%. Read cost is recovered from the read
 * discount (`cacheSavingsUsd / 9`) and subtracted from the bucket's actual input
 * cost -- both from the same pricing function, so the subtraction is sound.
 *
 * Only buckets with cache coverage count, so numerator and denominator always
 * describe the same slice; null when the window has no coverage. */
export function compressibleInputSavingsRate(
  points: Array<{
    cacheSavingsUsd?: number | null;
    actualCostUsd: number;
    estimatedSavingsUsd: number;
  }>
) {
  let saved = 0;
  // What survived compression and was still paid for at full input price.
  let remaining = 0;
  for (const point of points) {
    if (point.cacheSavingsUsd == null) continue;
    const readCostUsd = Math.max(0, point.cacheSavingsUsd) / 9;
    saved += Math.max(0, point.estimatedSavingsUsd);
    remaining += Math.max(0, point.actualCostUsd - readCostUsd);
  }
  const baseline = saved + remaining;
  if (baseline <= 0) return null;
  return { pct: Math.min(100, (saved / baseline) * 100), saved, remaining };
}

/** Output-shaper reduction over a window of buckets, from the locally-sampled
 * saved/baseline deltas. Only buckets with samples count; null when the window
 * has no coverage or the sampled baseline is zero. */
export function outputReductionForWindow(
  points: Array<{
    outputSampledTokensSaved?: number | null;
    outputBaselineTokens?: number | null;
  }>
) {
  let saved = 0;
  let baseline = 0;
  for (const point of points) {
    if (point.outputSampledTokensSaved == null || point.outputBaselineTokens == null) continue;
    saved += Math.max(0, point.outputSampledTokensSaved);
    baseline += Math.max(0, point.outputBaselineTokens);
  }
  if (baseline <= 0) return null;
  return { pct: Math.min(100, (saved / baseline) * 100), savedTokens: saved, baselineTokens: baseline };
}

export function formatDayLabel(dayKey: string) {
  const parsed = new Date(`${dayKey}T00:00:00`);
  if (Number.isNaN(parsed.getTime())) {
    return dayKey;
  }
  return new Intl.DateTimeFormat(undefined, {
    month: "short",
    day: "numeric"
  }).format(parsed);
}

export function formatHourLabel(hourKey: string) {
  const parsed = new Date(`${hourKey}:00`);
  if (Number.isNaN(parsed.getTime())) {
    return hourKey;
  }
  return new Intl.DateTimeFormat(undefined, {
    month: "short",
    day: "numeric",
    hour: "2-digit",
    minute: "2-digit"
  }).format(parsed);
}

export function formatDayKey(date: Date) {
  const year = date.getFullYear();
  const month = `${date.getMonth() + 1}`.padStart(2, "0");
  const day = `${date.getDate()}`.padStart(2, "0");
  return `${year}-${month}-${day}`;
}

export function startOfDay(date: Date) {
  return new Date(date.getFullYear(), date.getMonth(), date.getDate());
}

export function startOfMonth(date: Date) {
  return new Date(date.getFullYear(), date.getMonth(), 1);
}

export function endOfMonth(date: Date) {
  return new Date(date.getFullYear(), date.getMonth() + 1, 0);
}

export function addMonths(date: Date, delta: number) {
  return new Date(date.getFullYear(), date.getMonth() + delta, 1);
}

export function addDays(date: Date, delta: number) {
  return new Date(date.getFullYear(), date.getMonth(), date.getDate() + delta);
}

export function parseDayKey(dayKey: string) {
  const parsed = new Date(`${dayKey}T00:00:00`);
  return Number.isNaN(parsed.getTime()) ? null : parsed;
}

export function parseHourKey(hourKey: string) {
  const parsed = new Date(`${hourKey}:00`);
  return Number.isNaN(parsed.getTime()) ? null : parsed;
}

export function formatMonthLabel(date: Date) {
  return new Intl.DateTimeFormat(undefined, {
    month: "long",
    year: "numeric"
  }).format(date);
}

export function formatSelectedDayLabel(date: Date) {
  return new Intl.DateTimeFormat(undefined, {
    month: "short",
    day: "numeric"
  }).format(date);
}

export function buildMonthlySavingsWindow(data: DailySavingsPoint[], month: Date) {
  const monthStart = startOfMonth(month);
  const monthEnd = endOfMonth(month);
  const totalDays = monthEnd.getDate();
  const dataByDate = new Map(data.map((point) => [point.date, point]));

  return Array.from({ length: totalDays }, (_, index) => {
    const day = new Date(monthStart);
    day.setDate(index + 1);
    const date = formatDayKey(day);
    return dataByDate.get(date) ?? {
      date,
      estimatedSavingsUsd: 0,
      estimatedTokensSaved: 0,
      actualCostUsd: 0,
      totalTokensSent: 0,
      outputSavingsUsd: 0,
      outputTokensSaved: 0
    };
  });
}

export function buildHourlySavingsWindow(data: HourlySavingsPoint[], day: Date) {
  const dayKey = formatDayKey(day);
  const dataByHour = new Map(data.map((point) => [point.hour, point]));

  return Array.from({ length: 24 }, (_, hour) => {
    const hourKey = `${dayKey}T${String(hour).padStart(2, "0")}:00`;
    return dataByHour.get(hourKey) ?? {
      hour: hourKey,
      estimatedSavingsUsd: 0,
      estimatedTokensSaved: 0,
      actualCostUsd: 0,
      totalTokensSent: 0,
      outputSavingsUsd: 0,
      outputTokensSaved: 0,
      byProvider: []
    };
  });
}

/** Bucket spend with the provider cache-read portion stripped out: the slice
 * of the bill Headroom can actually act on. Cache reads bill at ~0.1x and the
 * cached prefix is deliberately left intact, so counting them makes every bar
 * dwarf its own savings segment and contradicts the compression-rate chip
 * above it (which uses the same denominator, see `compressibleInputSavingsRate`).
 * Read cost is recovered from the read discount the same way: `usd / 9`.
 *
 * Falls back to the full figure on buckets with no cache coverage - local
 * tracker buckets, and days aged out of the backend's checkpoint history. */
export function compressibleSpend(point: {
  cacheReadTokens?: number | null;
  cacheSavingsUsd?: number | null;
  actualCostUsd: number;
  totalTokensSent: number;
}) {
  const readCostUsd = point.cacheSavingsUsd == null ? 0 : Math.max(0, point.cacheSavingsUsd) / 9;
  return {
    compressibleCostUsd: Math.max(0, point.actualCostUsd - readCostUsd),
    compressibleTokensSent: Math.max(0, point.totalTokensSent - (point.cacheReadTokens ?? 0))
  };
}

export function buildMonthlySavingsChartData(data: DailySavingsPoint[]): SavingsChartDatum[] {
  return data.map((point) => ({
    bucketKey: point.date,
    bucketLabel: formatDayLabel(point.date),
    estimatedSavingsUsd: point.estimatedSavingsUsd,
    estimatedTokensSaved: point.estimatedTokensSaved,
    actualCostUsd: point.actualCostUsd,
    totalTokensSent: point.totalTokensSent,
    ...compressibleSpend(point),
    outputSavingsUsd: point.outputSavingsUsd ?? 0,
    outputTokensSaved: point.outputTokensSaved ?? 0,
    totalCostBeforeOptimization: point.actualCostUsd + point.estimatedSavingsUsd,
    totalTokensBeforeOptimization: point.totalTokensSent + point.estimatedTokensSaved
  }));
}

export interface ProviderSavingsDisplay {
  label: string;
  estimatedSavingsUsd: number;
  estimatedTokensSaved: number;
  actualCostUsd: number;
  totalTokensSent: number;
}

// Fold the upstream per-provider breakdown into the two connectors the desktop
// supports. Anything that isn't OpenAI/Codex is attributed to Claude Code,
// including legacy "unknown" buckets from before per-provider attribution
// existed (a period when Codex wasn't supported, so all traffic was Claude).
// Claude Code is listed first. A group is shown only if at least one source
// provider mapped into it.
export function mergeProviderSavingsForDisplay(
  byProvider: ProviderSavingsPoint[]
): ProviderSavingsDisplay[] {
  // Backend rollups attribute savings by upstream provider only, so OpenCode
  // (and grok until provider "xai" ships) savings silently blend into these
  // rows. Deliberately NOT surfaced in the labels: the honest per-connector
  // split arrives with the backend's by_agent rollups (upstream #2627), at
  // which point display rows switch to agent-keyed.
  const groups = {
    claude: {
      label: "Claude Code",
      count: 0,
      estimatedSavingsUsd: 0,
      estimatedTokensSaved: 0,
      actualCostUsd: 0,
      totalTokensSent: 0
    },
    codex: {
      label: "Codex",
      count: 0,
      estimatedSavingsUsd: 0,
      estimatedTokensSaved: 0,
      actualCostUsd: 0,
      totalTokensSent: 0
    },
    grok: {
      label: "Grok Build",
      count: 0,
      estimatedSavingsUsd: 0,
      estimatedTokensSaved: 0,
      actualCostUsd: 0,
      totalTokensSent: 0
    }
  };
  for (const point of byProvider) {
    const provider = point.provider.toLowerCase();
    const group =
      provider === "openai"
        ? groups.codex
        : provider === "xai"
          ? groups.grok
          : groups.claude;
    group.count += 1;
    group.estimatedSavingsUsd += point.estimatedSavingsUsd;
    group.estimatedTokensSaved += point.estimatedTokensSaved;
    group.actualCostUsd += point.actualCostUsd;
    group.totalTokensSent += point.totalTokensSent;
  }
  return [groups.claude, groups.codex, groups.grok]
    .filter((group) => group.count > 0)
    .map(({ count: _count, ...display }) => display);
}

export function buildHourlySavingsChartData(data: HourlySavingsPoint[]): SavingsChartDatum[] {
  return data.map((point) => ({
    bucketKey: point.hour,
    bucketLabel: formatHourLabel(point.hour),
    estimatedSavingsUsd: point.estimatedSavingsUsd,
    estimatedTokensSaved: point.estimatedTokensSaved,
    actualCostUsd: point.actualCostUsd,
    totalTokensSent: point.totalTokensSent,
    ...compressibleSpend(point),
    outputSavingsUsd: point.outputSavingsUsd ?? 0,
    outputTokensSaved: point.outputTokensSaved ?? 0,
    totalCostBeforeOptimization: point.actualCostUsd + point.estimatedSavingsUsd,
    totalTokensBeforeOptimization: point.totalTokensSent + point.estimatedTokensSaved,
    byProvider: point.byProvider ?? []
  }));
}

export function dayOfMonthTickFormatter(value: string) {
  const parsed = parseDayKey(value);
  if (!parsed) {
    return value;
  }
  const dayOfMonth = parsed.getDate();
  const lastDay = endOfMonth(parsed).getDate();
  return dayOfMonth === 1 || dayOfMonth === lastDay || dayOfMonth % 2 === 1
    ? String(dayOfMonth)
    : "";
}

export function hourOfDayTickFormatter(value: string) {
  const parsed = parseHourKey(value);
  if (!parsed) {
    return value;
  }
  const hour = parsed.getHours();
  return hour === 23 || hour % 4 === 0 ? String(hour).padStart(2, "0") : "";
}

export function earliestSavingsMonth(data: DailySavingsPoint[]) {
  let earliest: Date | null = null;

  for (const point of data) {
    const parsed = parseDayKey(point.date);
    if (!parsed) {
      continue;
    }
    const monthStart = startOfMonth(parsed);
    if (!earliest || monthStart < earliest) {
      earliest = monthStart;
    }
  }

  return earliest;
}

export function earliestHourlyDay(data: HourlySavingsPoint[]) {
  let earliest: Date | null = null;

  for (const point of data) {
    const parsed = parseHourKey(point.hour);
    if (!parsed) {
      continue;
    }
    const dayStart = startOfDay(parsed);
    if (!earliest || dayStart < earliest) {
      earliest = dayStart;
    }
  }

  return earliest;
}

export function formatDateTime(timestamp?: string | null) {
  if (!timestamp) {
    return "Never";
  }
  const parsed = new Date(timestamp);
  if (Number.isNaN(parsed.getTime())) {
    return "Unknown";
  }
  return new Intl.DateTimeFormat(undefined, {
    year: "numeric",
    month: "short",
    day: "numeric",
    hour: "2-digit",
    minute: "2-digit"
  }).format(parsed);
}

/**
 * Relative time for high-frequency events in the activity feed. Recent events
 * read as "just now" / "10m ago" / "6h ago" / "3 days ago"; anything older
 * than a week falls back to an absolute date. `now` is injectable so callers
 * with a mocked clock (tests) can get deterministic output.
 */
export function formatRelativeTime(
  timestamp?: string | null,
  now: Date = new Date()
): string {
  if (!timestamp) return "Never";
  const ms = new Date(timestamp).getTime();
  if (Number.isNaN(ms)) return "Unknown";
  const diff = now.getTime() - ms;
  if (diff < 45_000) return "just now";
  if (diff < 60 * 60_000) return `${Math.max(1, Math.floor(diff / 60_000))}m ago`;
  if (diff < 24 * 60 * 60_000) return `${Math.floor(diff / (60 * 60_000))}h ago`;
  if (diff < 7 * 24 * 60 * 60_000) {
    const days = Math.floor(diff / (24 * 60 * 60_000));
    return `${days} day${days === 1 ? "" : "s"} ago`;
  }
  // Older than a week: absolute date.
  const d = new Date(ms);
  const sameYear = d.getFullYear() === now.getFullYear();
  return new Intl.DateTimeFormat(undefined, {
    month: "short",
    day: "numeric",
    year: sameYear ? undefined : "numeric"
  }).format(d);
}

export function formatLearnStatus(project: {
  lastLearnRanAt: string | null;
}): string {
  if (!project.lastLearnRanAt) {
    return "never scan";
  }
  const parsed = new Date(project.lastLearnRanAt);
  if (Number.isNaN(parsed.getTime())) {
    return "never scan";
  }
  const diffMs = Date.now() - parsed.getTime();
  const diffDays = Math.floor(diffMs / 86_400_000);
  if (diffDays === 0) return "last scan: today";
  if (diffDays === 1) return "last scan: yesterday";
  return `last scan: ${diffDays} days ago`;
}

const SUPPORTED_CONNECTOR_IDS = new Set([
  "claude_code",
  "codex",
  "grok_build",
  "opencode"
]);

export function aggregateClientConnectors(connectors: ClientConnectorStatus[]) {
  return connectors.filter((connector) =>
    SUPPORTED_CONNECTOR_IDS.has(connector.clientId)
  );
}

export function sortClientConnectors(connectors: ClientConnectorStatus[]) {
  return [...connectors].sort((left, right) => {
    if (left.installed !== right.installed) {
      return left.installed ? -1 : 1;
    }
    return left.name.localeCompare(right.name);
  });
}

export function getEnabledSupportedConnectors(
  connectors: ClientConnectorStatus[]
) {
  return aggregateClientConnectors(connectors).filter(
    (connector) => connector.enabled
  );
}

export function hasEnabledConnector(connectors: ClientConnectorStatus[]) {
  return getEnabledSupportedConnectors(connectors).length > 0;
}

export type ConnectorDashboardTone = "active" | "pending" | "idle" | "off";

export function connectorDashboardStatus(
  connector: ClientConnectorStatus,
  opts?: { proxyReachable?: boolean }
): {
  label: string;
  tone: ConnectorDashboardTone;
} {
  if (!connector.enabled) {
    // Deliberately off, nothing wrong: gray, not red. Red ("idle") is
    // reserved for enabled-but-broken (see the proxyReachable override).
    return connector.installed
      ? { label: "Off", tone: "off" }
      : { label: "Not installed", tone: "off" };
  }
  if (opts?.proxyReachable === false) {
    return { label: "Proxy unreachable", tone: "idle" };
  }
  if (!connector.verified) {
    return connector.installed
      ? { label: "Verifying", tone: "pending" }
      : { label: "Restart needed", tone: "pending" };
  }
  return { label: "Active", tone: "active" };
}

