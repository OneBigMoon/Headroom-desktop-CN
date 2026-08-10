import { invoke } from "@tauri-apps/api/core";
import { getCurrentWindow } from "@tauri-apps/api/window";

import { needsTermsAcceptance } from "./launcherHelpers";
import { formatDayKey } from "./dashboardHelpers";
import type { DashboardState } from "./types";

// How long the app must run with nothing to show before we tell the user the
// setup is probably broken. Long enough that a user who launched Headroom and
// then went to a meeting isn't accused of a bad install on a normal quiet
// stretch, short enough that a genuinely broken hookup surfaces the same day
// it was installed.
export const SETUP_STALL_AFTER_MS = 30 * 60 * 1000;

// Cadence of the background check. The dashboard read is local (in-memory Rust
// state), so this is cheap; 5 min just keeps it off the hot path while the tray
// is hidden. It also means the alert lands anywhere in the 30-35 minute range,
// which is well inside the tolerance of a "you have been running a while"
// signal.
export const SETUP_STALL_CHECK_INTERVAL_MS = 5 * 60 * 1000;

// Local day, not UTC, for the same reason the other urgent notifications use a
// local key: a UTC key flips mid-afternoon for US users and lets two alerts
// land in one local day.
const SETUP_STALL_DAY_KEY = "headroom_setup_stall_date";

// "no_traffic": Headroom never saw a request, so the hookup itself is suspect
// (terminal still running the pre-install environment is the classic cause).
// "no_savings": requests are arriving but nothing is being trimmed, which is a
// different failure - optimization paused, gated, or misconfigured.
export type SetupStallKind = "no_traffic" | "no_savings";

export interface SetupStallAlert {
  kind: SetupStallKind;
  title: string;
  body: string;
}

export function setupStallMinutes(): number {
  return Math.round(SETUP_STALL_AFTER_MS / 60_000);
}

const STALL_TITLE = "Headroom hasn't saved anything yet";

function stallBody(kind: SetupStallKind): string {
  if (kind === "no_traffic") {
    return `Headroom has been running for ${setupStallMinutes()} minutes without seeing a single Claude Code or Codex request. Your setup likely needs another step. Open Headroom to check.`;
  }
  return "Requests are reaching Headroom but none are being optimized. Something is likely misconfigured. Open Headroom to check.";
}

export interface SetupStallContext {
  /// True when the account gate has optimization switched off (unpaid plan,
  /// signed out, weekly cap hit). Zero savings is the expected outcome then,
  /// and those states already have their own daily notifications. Undefined
  /// means pricing status hasn't loaded yet, which is treated as allowed.
  optimizationBlocked?: boolean;
}

/// Pure decision: is this session's silence worth alerting about? Returns null
/// whenever the silence is expected (still installing, runtime not there yet,
/// blocked behind a gate, not enough uptime) or whenever savings have landed.
export function evaluateSetupStall(
  dashboard: DashboardState,
  uptimeMs: number,
  context: SetupStallContext = {}
): SetupStallAlert | null {
  if (uptimeMs < SETUP_STALL_AFTER_MS) {
    return null;
  }
  // A half-finished install has its own progress UI and its own failure
  // reporting. Alerting here would just duplicate it with worse copy.
  if (!dashboard.bootstrapComplete || !dashboard.pythonRuntimeInstalled) {
    return null;
  }
  // Nothing routes through Headroom until the Terms gate is cleared, so zero
  // savings there says nothing about the setup.
  if (needsTermsAcceptance(dashboard.requiredTermsVersion, dashboard.acceptedTermsVersion)) {
    return null;
  }
  if (context.optimizationBlocked) {
    return null;
  }
  const savingsRecorded =
    dashboard.lifetimeEstimatedTokensSaved > 0 || dashboard.lifetimeEstimatedSavingsUsd > 0;
  if (savingsRecorded) {
    return null;
  }
  const kind: SetupStallKind = dashboard.lifetimeRequests > 0 ? "no_savings" : "no_traffic";
  return { kind, title: STALL_TITLE, body: stallBody(kind) };
}

/// Fire the alert at most once per local day, and never once savings exist.
/// Returns the alert when this call consumed the day's slot (the caller should
/// then show the modal), null when throttled or not due.
///
/// The native notification is skipped when the tray window is already visible
/// - the modal is the better surface in that case - but the day slot is
/// consumed either way so the user gets one interruption, not two.
export async function maybeFireSetupStallAlert(
  dashboard: DashboardState,
  uptimeMs: number,
  context: SetupStallContext = {}
): Promise<SetupStallAlert | null> {
  const alert = evaluateSetupStall(dashboard, uptimeMs, context);
  if (!alert) {
    return null;
  }

  const today = formatDayKey(new Date());
  if (readDayKey() === today) {
    return null;
  }
  writeDayKey(today);

  if (!(await isWindowVisible())) {
    try {
      await invoke("show_notification", {
        title: alert.title,
        body: alert.body,
        action: "setup",
      });
    } catch {
      // Best effort. The modal still carries the message.
    }
  }

  return alert;
}

function readDayKey(): string | null {
  try {
    return localStorage.getItem(SETUP_STALL_DAY_KEY);
  } catch {
    return null;
  }
}

function writeDayKey(day: string): void {
  try {
    localStorage.setItem(SETUP_STALL_DAY_KEY, day);
  } catch {
    // Private-mode / quota failures shouldn't suppress the alert itself.
  }
}

/// Test affordance: clear the once-per-day throttle.
export function __resetSetupStallThrottle(): void {
  try {
    localStorage.removeItem(SETUP_STALL_DAY_KEY);
  } catch {
    // no-op
  }
}

async function isWindowVisible(): Promise<boolean> {
  return getCurrentWindow()
    .isVisible()
    .catch(() => false);
}
