import { render, screen, waitFor, within } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { beforeEach, describe, expect, it, vi } from "vitest";

const { invokeMock } = vi.hoisted(() => ({
  invokeMock: vi.fn(),
}));

vi.mock("@tauri-apps/api/core", () => ({
  invoke: (...args: unknown[]) => invokeMock(...args),
}));

import { CommunityApp } from "./CommunityApp";
import { I18nProvider, LOCALE_STORAGE_KEY, resolveSystemLocale } from "./lib/i18n";
import { LOCAL_COMMUNITY_NAME } from "./lib/localEdition";
import { mockDashboard } from "./lib/mockData";
import type { ClientConnectorStatus, DashboardState, HeadroomLearnStatus, RuntimeStatus } from "./lib/types";

let connectorEnabled = false;
let rtkInstalled = false;
let rtkEnabled = false;
let cavemanInstalled = true;
let cavemanEnabled = true;
let cavemanStatus: "not_installed" | "installing" | "healthy" | "degraded" = "healthy";
let cavemanUnavailableReason: string | null = null;
let cavemanMode = "wenyan-full";
let ponytailInstalled = true;
let ponytailEnabled = true;
let ponytailMode = "ultra";
let autoLearnEnabled = true;
let autostartEnabled = false;
let learnStatus: HeadroomLearnStatus = {
  running: false,
  projectPath: "/Users/example/project",
  projectDisplayName: "Example project",
  progressPercent: 0,
  summary: "No local scan is running.",
  outputTail: [] as string[],
};

const activityFeed = {
  proxyReachable: true,
  tiles: {
    transformation: null,
    record: null,
    rtkToday: null,
    serenaToday: null,
    learningsMilestone: null,
    weeklyRecap: null,
    trainSuggestion: null,
  },
};

const claudeProjects = [
  {
    id: "example-project",
    projectPath: "/Users/example/project",
    displayName: "Example project",
    lastWorkedAt: "2026-08-26T08:00:00Z",
    sessionCount: 8,
    lastLearnRanAt: null,
    hasPersistedLearnings: false,
    activeDaysSinceLastLearn: 3,
    lastLearnPatternCount: null,
  },
];

const runtime: RuntimeStatus = {
  platform: "macOS",
  supportTier: "native",
  installed: true,
  running: true,
  starting: false,
  paused: false,
  autoPaused: false,
  bypassed: false,
  proxyReachable: true,
  headroomLearnSupported: true,
  mcpConfigured: true,
  mcpError: null,
  kompressEnabled: true,
  rtk: {
    installed: true,
    enabled: true,
    pathConfigured: true,
    hookConfigured: true,
  },
};

function dashboardState(): DashboardState {
  return {
    ...mockDashboard,
    appVersion: "0.8.7",
    bootstrapComplete: true,
    lifetimeRequests: 42,
    lifetimeEstimatedTokensSaved: 18_500,
    lifetimeEstimatedSavingsUsd: 3.5,
    tools: [
      {
        id: "rtk",
        name: "RTK",
        description: "Compact local command output.",
        runtime: "binary",
        required: false,
        enabled: rtkEnabled,
        status: rtkInstalled ? "healthy" : "not_installed",
        sourceUrl: "https://example.invalid/rtk",
        version: "1.0.0",
      },
      {
        id: "caveman",
        name: "Caveman",
        description: "Local prompt compression modes.",
        runtime: "plugin",
        required: false,
        enabled: cavemanEnabled,
        status: cavemanInstalled ? cavemanStatus : "not_installed",
        sourceUrl: "https://example.invalid/caveman",
        version: "1.0.0",
        defaultMode: cavemanMode,
        supportedModes: ["lite", "full", "ultra", "wenyan-lite", "wenyan-full", "wenyan-ultra"],
        unavailableReason: cavemanUnavailableReason,
      },
      {
        id: "ponytail",
        name: "Ponytail",
        description: "Local output minimization modes.",
        runtime: "plugin",
        required: false,
        enabled: ponytailEnabled,
        status: ponytailInstalled ? "healthy" : "not_installed",
        sourceUrl: "https://example.invalid/ponytail",
        version: "1.0.0",
        defaultMode: ponytailMode,
        supportedModes: ["lite", "full", "ultra"],
      },
    ],
  };
}

function connectorsState(): ClientConnectorStatus[] {
  return [
    {
      clientId: "codex",
      name: "Codex",
      installed: true,
      enabled: connectorEnabled,
      verified: connectorEnabled,
    },
  ];
}

function renderCommunityApp() {
  return render(
    <I18nProvider>
      <CommunityApp />
    </I18nProvider>,
  );
}

function setNavigatorLanguages(languages: string[]) {
  Object.defineProperty(navigator, "languages", {
    configurable: true,
    value: languages,
  });
  Object.defineProperty(navigator, "language", {
    configurable: true,
    value: languages[0] ?? "en-US",
  });
}

beforeEach(() => {
  localStorage.removeItem(LOCALE_STORAGE_KEY);
  document.documentElement.lang = "";
  setNavigatorLanguages(["en-US"]);
  connectorEnabled = false;
  rtkInstalled = false;
  rtkEnabled = false;
  cavemanInstalled = true;
  cavemanEnabled = true;
  cavemanStatus = "healthy";
  cavemanUnavailableReason = null;
  cavemanMode = "wenyan-full";
  ponytailInstalled = true;
  ponytailEnabled = true;
  ponytailMode = "ultra";
  autoLearnEnabled = true;
  autostartEnabled = false;
  runtime.headroomLearnSupported = true;
  runtime.headroomLearnDisabledReason = null;
  learnStatus = {
    running: false,
    projectPath: "/Users/example/project",
    projectDisplayName: "Example project",
    progressPercent: 0,
    summary: "No local scan is running.",
    outputTail: [],
  };
  invokeMock.mockReset();
  invokeMock.mockImplementation((command: string, args?: unknown) => {
    switch (command) {
      case "get_dashboard_state":
        return Promise.resolve(dashboardState());
      case "get_runtime_status":
        return Promise.resolve(runtime);
      case "get_client_connectors":
        return Promise.resolve(connectorsState());
      case "get_activity_feed":
        return Promise.resolve(activityFeed);
      case "get_claude_code_projects":
        return Promise.resolve(claudeProjects);
      case "get_headroom_learn_prereq_status":
        return Promise.resolve({
          claudeCliAvailable: true,
          claudeCliPath: "/usr/local/bin/claude",
          codexCliAvailable: true,
          codexCliPath: "/usr/local/bin/codex",
          codexLoggedIn: true,
        });
      case "get_headroom_learn_status":
        return Promise.resolve(learnStatus);
      case "start_headroom_learn":
        learnStatus = {
          ...learnStatus,
          running: true,
          progressPercent: 8,
          summary: "Local scan is running.",
          currentStep: "Collecting local sessions",
        };
        return Promise.resolve();
      case "get_auto_learn_enabled":
        return Promise.resolve(autoLearnEnabled);
      case "set_auto_learn_enabled":
        autoLearnEnabled = Boolean((args as { enabled?: boolean } | undefined)?.enabled);
        return Promise.resolve(autoLearnEnabled);
      case "get_headroom_logs":
        return Promise.resolve(["runtime online", "proxy listening on 127.0.0.1:6867"]);
      case "get_autostart_enabled":
        return Promise.resolve(autostartEnabled);
      case "set_autostart_enabled":
        autostartEnabled = Boolean((args as { enabled?: boolean } | undefined)?.enabled);
        return Promise.resolve(autostartEnabled);
      case "show_notification":
      case "quit_headroom":
        return Promise.resolve();
      case "apply_client_setup":
        connectorEnabled = true;
        return Promise.resolve({
          clientId: "codex",
          applied: true,
          alreadyConfigured: false,
          summary: "Codex is connected locally.",
          changedFiles: [],
          backupFiles: [],
          nextSteps: [],
          verification: {
            clientId: "codex",
            verified: true,
            proxyReachable: true,
            checks: [],
            failures: [],
          },
        });
      case "disable_client_setup":
        connectorEnabled = false;
        return Promise.resolve();
      case "install_addon":
        rtkInstalled = true;
        rtkEnabled = true;
        return Promise.resolve(dashboardState());
      case "set_addon_enabled":
        rtkEnabled = !rtkEnabled;
        return Promise.resolve(dashboardState());
      case "uninstall_addon":
        rtkInstalled = false;
        rtkEnabled = false;
        return Promise.resolve(dashboardState());
      case "set_addon_mode": {
        const { id, mode } = (args ?? {}) as { id?: string; mode?: string };
        if (id === "caveman" && mode) cavemanMode = mode;
        if (id === "ponytail" && mode) ponytailMode = mode;
        return Promise.resolve(dashboardState());
      }
      case "start_headroom":
      case "pause_headroom":
      case "force_restart_headroom":
        return Promise.resolve();
      default:
        return Promise.resolve();
    }
  });
});

describe("CommunityApp", () => {
  it("resolves supported system languages and falls back to English", () => {
    expect(resolveSystemLocale(["zh-CN"])).toBe("zh-CN");
    expect(resolveSystemLocale(["zh-SG"])).toBe("zh-CN");
    expect(resolveSystemLocale(["zh"])).toBe("zh-CN");
    expect(resolveSystemLocale(["zh-Hant-HK"])).toBe("zh-TW");
    expect(resolveSystemLocale(["zh-TW"])).toBe("zh-TW");
    expect(resolveSystemLocale(["ja-JP"])).toBe("ja");
    expect(resolveSystemLocale(["ko-KR"])).toBe("ko");
    expect(resolveSystemLocale(["fr-FR", "zh-CN"])).toBe("zh-CN");
    expect(resolveSystemLocale(["fr-FR"])).toBe("en");
  });

  it("renders Community branding, live local proxy state, and local activity", async () => {
    renderCommunityApp();

    expect(await screen.findByText("Proxy online")).toBeInTheDocument();
    expect(screen.getByText(LOCAL_COMMUNITY_NAME)).toBeInTheDocument();
    expect(screen.getByText("Requests processed")).toBeInTheDocument();
    expect(screen.getByText("Tokens saved")).toBeInTheDocument();
    expect(screen.getByText("42")).toBeInTheDocument();
    expect(screen.queryByText(/Sign in|Upgrade|Pricing|Subscription/i)).not.toBeInTheDocument();
  });

  it("connects and disconnects a local client connector", async () => {
    const user = userEvent.setup();
    renderCommunityApp();

    await screen.findByText("Proxy online");
    await user.click(screen.getByRole("button", { name: "Connections" }));
    await user.click(await screen.findByRole("button", { name: "Connect" }));

    await waitFor(() => {
      expect(invokeMock).toHaveBeenCalledWith("apply_client_setup", { clientId: "codex" });
    });

    await user.click(await screen.findByRole("button", { name: "Disconnect" }));
    await waitFor(() => {
      expect(invokeMock).toHaveBeenCalledWith("disable_client_setup", { clientId: "codex" });
    });
  });

  it("installs and toggles a local tool", async () => {
    const user = userEvent.setup();
    renderCommunityApp();

    await screen.findByText("Proxy online");
    await user.click(screen.getByRole("button", { name: "Tools" }));
    const rtkCard = screen.getByText("RTK").closest("article");
    expect(rtkCard).not.toBeNull();
    await user.click(await within(rtkCard as HTMLElement).findByRole("button", { name: "Install" }));

    await waitFor(() => {
      expect(invokeMock).toHaveBeenCalledWith("install_addon", { id: "rtk" });
    });

    await user.click(await within(rtkCard as HTMLElement).findByRole("button", { name: "Disable" }));
    await waitFor(() => {
      expect(invokeMock).toHaveBeenCalledWith("set_addon_enabled", { id: "rtk", enabled: false });
    });
  });

  it("shows each add-on's actual default-mode choices and saves a selected mode", async () => {
    const user = userEvent.setup();
    renderCommunityApp();

    await screen.findByText("Proxy online");
    await user.click(screen.getByRole("button", { name: "Tools" }));

    const cavemanModes = screen.getByRole("radiogroup", { name: "Default mode for Caveman" });
    const ponytailModes = screen.getByRole("radiogroup", { name: "Default mode for Ponytail" });
    expect(within(cavemanModes).getAllByRole("radio")).toHaveLength(6);
    expect(within(ponytailModes).getAllByRole("radio")).toHaveLength(3);
    expect(within(cavemanModes).getByRole("radio", { name: "Wenyan Full" })).toBeChecked();
    expect(within(ponytailModes).getByRole("radio", { name: "Ultra" })).toBeChecked();
    expect(within(ponytailModes).queryByRole("radio", { name: /Wenyan/i })).not.toBeInTheDocument();

    await user.click(within(cavemanModes).getByRole("radio", { name: "Wenyan Ultra" }));

    await waitFor(() => {
      expect(invokeMock).toHaveBeenCalledWith("set_addon_mode", { id: "caveman", mode: "wenyan-ultra" });
    });
    expect(await screen.findByText("Caveman default mode set to Wenyan Ultra. New agent sessions use it.")).toBeInTheDocument();
  });

  it("keeps default-mode controls unavailable until an add-on is installed and enabled", async () => {
    ponytailInstalled = false;
    ponytailEnabled = false;
    cavemanEnabled = false;
    renderCommunityApp();

    await screen.findByText("Proxy online");
    await userEvent.setup().click(screen.getByRole("button", { name: "Tools" }));

    const cavemanModes = screen.getByRole("radiogroup", { name: "Default mode for Caveman" });
    const ponytailModes = screen.getByRole("radiogroup", { name: "Default mode for Ponytail" });
    for (const mode of within(cavemanModes).getAllByRole("radio")) {
      expect(mode).toBeDisabled();
    }
    for (const mode of within(ponytailModes).getAllByRole("radio")) {
      expect(mode).toBeDisabled();
    }
  });

  it("loads the local Activity feed only after the Activity view is opened", async () => {
    const user = userEvent.setup();
    renderCommunityApp();

    await screen.findByText("Proxy online");
    expect(invokeMock).not.toHaveBeenCalledWith("get_activity_feed");
    await user.click(screen.getByRole("button", { name: "Activity" }));

    expect(await screen.findByRole("heading", { name: "Recent local activity" })).toBeInTheDocument();
    await waitFor(() => {
      expect(invokeMock).toHaveBeenCalledWith("get_activity_feed");
    });
  });

  it("starts a local Claude Learn scan and exposes every supported session source", async () => {
    const user = userEvent.setup();
    renderCommunityApp();

    await screen.findByText("Proxy online");
    await user.click(screen.getByRole("button", { name: "Optimize" }));
    expect(await screen.findByRole("heading", { name: "Scan local coding sessions" })).toBeInTheDocument();
    expect(screen.getByRole("option", { name: "Claude Code" })).toBeInTheDocument();
    expect(screen.getByRole("option", { name: "Codex" })).toBeInTheDocument();
    expect(screen.getByRole("option", { name: "OpenCode" })).toBeInTheDocument();
    expect(screen.getByRole("option", { name: "Grok" })).toBeInTheDocument();

    await waitFor(() => {
      expect(screen.getByRole("button", { name: "Start Learn" })).toBeEnabled();
    });
    await user.click(screen.getByRole("button", { name: "Start Learn" }));

    await waitFor(() => {
      expect(invokeMock).toHaveBeenCalledWith("start_headroom_learn", {
        agent: "claude",
        projectPath: "/Users/example/project",
      });
    });
  });

  it("shows a local unsupported state instead of starting Learn on an unsupported platform", async () => {
    runtime.headroomLearnSupported = false;
    runtime.headroomLearnDisabledReason = null;
    const user = userEvent.setup();
    renderCommunityApp();

    await screen.findByText("Proxy online");
    await user.click(screen.getByRole("button", { name: "Optimize" }));
    expect(await screen.findByText("Learn is unavailable on this platform.")).toBeInTheDocument();
    expect(screen.getByRole("button", { name: "Start Learn" })).toBeDisabled();
  });

  it("shows local diagnostics logs and can refresh them", async () => {
    const user = userEvent.setup();
    renderCommunityApp();

    await screen.findByText("Proxy online");
    await user.click(screen.getByRole("button", { name: "Diagnostics" }));
    expect(await screen.findByRole("heading", { name: "Runtime and local logs" })).toBeInTheDocument();
    expect(await screen.findByText(/runtime online/)).toBeInTheDocument();
    expect(invokeMock).toHaveBeenCalledWith("get_headroom_logs", { maxLines: 120 });

    await user.click(screen.getByRole("button", { name: "Refresh logs" }));
    await waitFor(() => {
      expect(invokeMock.mock.calls.filter(([command]) => command === "get_headroom_logs")).toHaveLength(2);
    });
  });

  it("changes local autostart and sends only an explicit local test notification", async () => {
    const user = userEvent.setup();
    renderCommunityApp();

    await screen.findByText("Proxy online");
    await user.click(screen.getByRole("button", { name: "Local settings" }));
    const autostart = await screen.findByLabelText("Start at login");
    expect(autostart).not.toBeChecked();
    await user.click(autostart);

    await waitFor(() => {
      expect(invokeMock).toHaveBeenCalledWith("set_autostart_enabled", { enabled: true });
    });
    await user.click(screen.getByRole("button", { name: "Send local test notification" }));

    await waitFor(() => {
      expect(invokeMock).toHaveBeenCalledWith("show_notification", {
        title: "Headroom Local Community test",
        body: "This is a local notification test from Headroom Local Community.",
        action: "local-test",
      });
    });
  });

  it("prevents competing default-mode writes while one mode save is busy", async () => {
    const user = userEvent.setup();
    let resolveModeSave: ((state: DashboardState) => void) | undefined;
    const pendingModeSave = new Promise<DashboardState>((resolve) => {
      resolveModeSave = resolve;
    });
    invokeMock.mockImplementation((command: string, args?: unknown) => {
      if (command === "set_addon_mode") {
        const { mode } = (args ?? {}) as { mode?: string };
        if (mode) cavemanMode = mode;
        return pendingModeSave;
      }
      if (command === "get_dashboard_state") return Promise.resolve(dashboardState());
      if (command === "get_runtime_status") return Promise.resolve(runtime);
      if (command === "get_client_connectors") return Promise.resolve(connectorsState());
      return Promise.resolve(args);
    });
    renderCommunityApp();

    await screen.findByText("Proxy online");
    await user.click(screen.getByRole("button", { name: "Tools" }));
    const cavemanModes = screen.getByRole("radiogroup", { name: "Default mode for Caveman" });
    await user.click(within(cavemanModes).getByRole("radio", { name: "Lite" }));

    await waitFor(() => {
      expect(invokeMock).toHaveBeenCalledWith("set_addon_mode", { id: "caveman", mode: "lite" });
    });
    for (const mode of within(cavemanModes).getAllByRole("radio")) {
      expect(mode).toBeDisabled();
    }
    await user.click(within(cavemanModes).getByRole("radio", { name: "Ultra" }));
    expect(invokeMock.mock.calls.filter(([command]) => command === "set_addon_mode")).toHaveLength(1);

    resolveModeSave?.(dashboardState());
    await screen.findByText("Caveman default mode set to Lite. New agent sessions use it.");
  });

  it("localizes the user-level default-mode explanation in every supported language", async () => {
    const locales = [
      { locale: "en", toolsLabel: "Tools", help: "User-level default. New agent sessions use it; environment variables can override it." },
      { locale: "zh-CN", toolsLabel: "工具", help: "用户级默认档位，仅对新的 agent 会话生效；环境变量可能覆盖它。" },
      { locale: "zh-TW", toolsLabel: "工具", help: "使用者層級的預設檔位，只對新的 agent 工作階段生效；環境變數可能覆蓋它。" },
      { locale: "ja", toolsLabel: "ツール", help: "ユーザー既定値です。新しい agent セッションで有効になり、環境変数が上書きする場合があります。" },
      { locale: "ko", toolsLabel: "도구", help: "사용자 기본 모드이며 새 agent 세션에 적용됩니다. 환경 변수가 이를 덮어쓸 수 있습니다." },
    ] as const;

    for (const { locale, toolsLabel, help } of locales) {
      localStorage.setItem(LOCALE_STORAGE_KEY, locale);
      const user = userEvent.setup();
      const { unmount } = renderCommunityApp();
      await screen.findByText(locale === "en" ? "Proxy online" : /代理已在线|代理已上線|プロキシはオンラインです|프록시 온라인/);
      await user.click(screen.getByRole("button", { name: toolsLabel }));
      const cavemanCard = screen.getByText("Caveman").closest("article");
      expect(cavemanCard).not.toBeNull();
      expect(await within(cavemanCard as HTMLElement).findByText(help)).toBeInTheDocument();
      unmount();
    }
  });

  it("changes the language immediately and persists the selected locale", async () => {
    const user = userEvent.setup();
    renderCommunityApp();

    await screen.findByText("Proxy online");
    await user.click(screen.getByRole("button", { name: "Local settings" }));
    await user.selectOptions(screen.getByLabelText("Language"), "zh-CN");

    await waitFor(() => {
      expect(screen.getByRole("heading", { name: "独立的社区版" })).toBeInTheDocument();
      expect(document.documentElement.lang).toBe("zh-CN");
    });
    expect(localStorage.getItem(LOCALE_STORAGE_KEY)).toBe("zh-CN");
  });

  it("uses a persisted locale when the Community app starts", async () => {
    localStorage.setItem(LOCALE_STORAGE_KEY, "ja");
    const user = userEvent.setup();
    renderCommunityApp();

    expect(await screen.findByText("プロキシはオンラインです")).toBeInTheDocument();
    await user.click(screen.getByRole("button", { name: "ローカル設定" }));

    await waitFor(() => expect(document.documentElement.lang).toBe("ja"));
    expect(screen.getByLabelText("言語")).toHaveValue("ja");
  });

  it("falls back to the system locale for an invalid stored value", async () => {
    localStorage.setItem(LOCALE_STORAGE_KEY, "not-a-supported-locale");
    setNavigatorLanguages(["zh-HK"]);
    const user = userEvent.setup();
    renderCommunityApp();

    expect(await screen.findByText("代理已上線")).toBeInTheDocument();
    await user.click(screen.getByRole("button", { name: "本機設定" }));

    await waitFor(() => expect(document.documentElement.lang).toBe("zh-TW"));
    expect(screen.getByLabelText("語言")).toHaveValue("system");
  });

  it("does not render account or paid-product controls in any supported language", async () => {
    const forbiddenLabels = [
      /Sign in|Account|Upgrade|Pricing|Subscription/i,
      /登录|登入|账户|帳戶|注册|註冊|升级|升級|定价|定價|订阅|訂閱/,
      /サインイン|アカウント|登録|アップグレード|料金|サブスクリプション/,
      /로그인|계정|가입|업그레이드|요금|구독/,
    ];

    const proxyTitles = {
      system: "Proxy online",
      en: "Proxy online",
      "zh-CN": "代理已在线",
      "zh-TW": "代理已上線",
      ja: "プロキシはオンラインです",
      ko: "프록시 온라인",
    } as const;

    for (const locale of ["system", "en", "zh-CN", "zh-TW", "ja", "ko"] as const) {
      localStorage.removeItem(LOCALE_STORAGE_KEY);
      if (locale !== "system") localStorage.setItem(LOCALE_STORAGE_KEY, locale);
      const { unmount } = renderCommunityApp();
      await screen.findByText(proxyTitles[locale]);
      const renderedDom = document.body.innerHTML;
      for (const forbiddenLabel of forbiddenLabels) {
        expect(renderedDom).not.toMatch(forbiddenLabel);
      }
      unmount();
    }
  });
});
