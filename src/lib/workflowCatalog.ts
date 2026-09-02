import type { ResolvedLocale } from "./i18n";

export type ToolCategory =
  | "core"
  | "guardrails"
  | "learning"
  | "workflow"
  | "automation"
  | "code_intelligence"
  | "documents"
  | "efficiency"
  | "other";

type Localized = Record<ResolvedLocale, string>;

export const TOOL_CATEGORY_ORDER: ToolCategory[] = [
  "core",
  "guardrails",
  "learning",
  "workflow",
  "automation",
  "code_intelligence",
  "documents",
  "efficiency",
  "other",
];

export const toolCategoryCopy: Record<
  ToolCategory,
  { title: Localized; description: Localized }
> = {
  core: {
    title: { en: "Core", "zh-CN": "核心", "zh-TW": "核心", ja: "コア", ko: "핵심" },
    description: {
      en: "The local routing and runtime foundation.",
      "zh-CN": "本地路由与运行时基础。",
      "zh-TW": "本機路由與執行階段基礎。",
      ja: "ローカルルーティングとランタイムの基盤。",
      ko: "로컬 라우팅과 런타임 기반입니다.",
    },
  },
  guardrails: {
    title: {
      en: "Safety constraints",
      "zh-CN": "安全约束",
      "zh-TW": "安全約束",
      ja: "安全制約",
      ko: "안전 제약",
    },
    description: {
      en: "Independent guardrails can stack and coexist with workflow tools.",
      "zh-CN": "独立安全约束可以叠加，也能与工作流工具共存。",
      "zh-TW": "獨立安全約束可以疊加，也能與工作流程工具共存。",
      ja: "独立したガードレールは併用でき、ワークフローツールとも共存できます。",
      ko: "독립 안전 장치는 함께 사용할 수 있고 워크플로 도구와도 공존합니다.",
    },
  },
  learning: {
    title: { en: "Learning", "zh-CN": "学习理解", "zh-TW": "學習理解", ja: "学習", ko: "학습" },
    description: {
      en: "Tools for understanding code and stress-testing assumptions.",
      "zh-CN": "用于理解代码和检验假设的工具。",
      "zh-TW": "用於理解程式碼和檢驗假設的工具。",
      ja: "コード理解と前提検証のためのツール。",
      ko: "코드 이해와 가정 검증을 위한 도구입니다.",
    },
  },
  workflow: {
    title: {
      en: "Primary workflow",
      "zh-CN": "主工作流",
      "zh-TW": "主要工作流程",
      ja: "主ワークフロー",
      ko: "주 워크플로",
    },
    description: {
      en: "OpenSpec, Superpowers, and gstack are mutually exclusive. Enabling one disables the active peer.",
      "zh-CN": "OpenSpec、Superpowers、gstack 互斥单选；启用一个会关闭同组已启用项。",
      "zh-TW": "OpenSpec、Superpowers、gstack 互斥單選；啟用一個會停用同組已啟用項。",
      ja: "OpenSpec、Superpowers、gstack は排他的です。1つを有効にすると同じグループの有効項目を無効にします。",
      ko: "OpenSpec, Superpowers, gstack은 상호 배타적이며 하나를 켜면 같은 그룹의 활성 항목을 끕니다.",
    },
  },
  automation: {
    title: {
      en: "Automation executor",
      "zh-CN": "自动执行器",
      "zh-TW": "自動執行器",
      ja: "自動実行",
      ko: "자동 실행기",
    },
    description: {
      en: "All in Luna and Ralph Loop are mutually exclusive. Both require an explicit user start.",
      "zh-CN": "All in Luna 与 Ralph Loop 互斥单选，且都必须由用户明确启动。",
      "zh-TW": "All in Luna 與 Ralph Loop 互斥單選，且都必須由使用者明確啟動。",
      ja: "All in Luna と Ralph Loop は排他的で、どちらもユーザーの明示的な開始が必要です。",
      ko: "All in Luna와 Ralph Loop는 상호 배타적이며 모두 사용자가 명시적으로 시작해야 합니다.",
    },
  },
  code_intelligence: {
    title: {
      en: "Code intelligence",
      "zh-CN": "代码智能",
      "zh-TW": "程式碼智慧",
      ja: "コードインテリジェンス",
      ko: "코드 인텔리전스",
    },
    description: {
      en: "Complementary code navigation, memory, and documentation tools.",
      "zh-CN": "可共同使用的代码导航、记忆与文档工具。",
      "zh-TW": "可共同使用的程式碼導覽、記憶與文件工具。",
      ja: "併用できるコードナビゲーション、メモリ、ドキュメントツール。",
      ko: "함께 사용할 수 있는 코드 탐색, 메모리, 문서 도구입니다.",
    },
  },
  documents: {
    title: { en: "Documents", "zh-CN": "文档", "zh-TW": "文件", ja: "文書", ko: "문서" },
    description: {
      en: "Converters that make local documents easier for agents to read.",
      "zh-CN": "让代理更容易读取本地文档的转换工具。",
      "zh-TW": "讓代理更容易讀取本機文件的轉換工具。",
      ja: "ローカル文書をエージェントが読みやすくする変換ツール。",
      ko: "에이전트가 로컬 문서를 읽기 쉽게 변환합니다.",
    },
  },
  efficiency: {
    title: { en: "Efficiency", "zh-CN": "效率", "zh-TW": "效率", ja: "効率化", ko: "효율" },
    description: {
      en: "Small local improvements that reduce cost or friction.",
      "zh-CN": "降低成本或操作摩擦的小型本地优化。",
      "zh-TW": "降低成本或操作摩擦的小型本機最佳化。",
      ja: "コストや摩擦を減らす小さなローカル改善。",
      ko: "비용과 마찰을 줄이는 작은 로컬 개선입니다.",
    },
  },
  other: {
    title: { en: "Other", "zh-CN": "其他", "zh-TW": "其他", ja: "その他", ko: "기타" },
    description: {
      en: "Tools without a recognized category yet.",
      "zh-CN": "尚未识别分类的工具。",
      "zh-TW": "尚未識別分類的工具。",
      ja: "まだ分類を認識できないツール。",
      ko: "아직 분류가 확인되지 않은 도구입니다.",
    },
  },
};

export const activationScopeCopy: Localized = {
  en: "Enable or disable takes effect in new sessions. No Headroom or 6867 proxy restart is needed.",
  "zh-CN": "启用或关闭后，新会话生效；无需重启 Headroom 或 6867 代理。",
  "zh-TW": "啟用或停用後，新工作階段生效；無需重新啟動 Headroom 或 6867 代理。",
  ja: "有効化または無効化は新しいセッションで反映されます。Headroom や 6867 プロキシの再起動は不要です。",
  ko: "활성화 또는 비활성화는 새 세션부터 적용되며 Headroom이나 6867 프록시를 다시 시작할 필요가 없습니다.",
};

export const workflowGroupCopy: Record<string, Localized> = {
  primary_workflow: {
    en: "Single-select: primary workflow",
    "zh-CN": "单选：主工作流",
    "zh-TW": "單選：主要工作流程",
    ja: "単一選択：主ワークフロー",
    ko: "단일 선택: 주 워크플로",
  },
  execution_engine: {
    en: "Single-select: automation executor",
    "zh-CN": "单选：自动执行器",
    "zh-TW": "單選：自動執行器",
    ja: "単一選択：自動実行",
    ko: "단일 선택: 자동 실행기",
  },
};

export const conflictHeadingCopy: Localized = {
  en: "Conflict and switching rules",
  "zh-CN": "冲突与切换规则",
  "zh-TW": "衝突與切換規則",
  ja: "競合と切り替えルール",
  ko: "충돌 및 전환 규칙",
};

export const sourceLinkCopy: Localized = {
  en: "Source and acknowledgements",
  "zh-CN": "源码与致谢",
  "zh-TW": "原始碼與致謝",
  ja: "ソースと謝辞",
  ko: "소스 및 감사",
};

export const toolCopy: Record<string, Localized> = {
  openspec: {
    en: "Best for specification-driven, cross-module changes with explicit requirements and acceptance evidence. Includes the OpenSpec CLI. Thanks to Fission AI and contributors.",
    "zh-CN": "适合需求、规格和验收证据清楚的跨模块改动；同时安装 OpenSpec CLI。感谢 Fission AI 与贡献者。",
    "zh-TW": "適合需求、規格和驗收證據清楚的跨模組改動；同時安裝 OpenSpec CLI。感謝 Fission AI 與貢獻者。",
    ja: "明確な要件・仕様・受け入れ証拠を伴う複数モジュール変更向け。OpenSpec CLI も導入します。Fission AI と貢献者に感謝します。",
    ko: "명확한 요구사항, 사양, 수용 증거가 있는 모듈 간 변경에 적합하며 OpenSpec CLI도 설치합니다. Fission AI와 기여자에게 감사합니다.",
  },
  superpowers: {
    en: "Uses Codex's official curated plugin. Best for disciplined brainstorming, planning, TDD, debugging, review, and delivery. Thanks to Jesse Vincent and contributors.",
    "zh-CN": "复用 Codex 官方 curated 插件，避免重复 marketplace。适合强调 brainstorm、计划、TDD、系统调试、审查与交付纪律的开发。感谢 Jesse Vincent 与贡献者。",
    "zh-TW": "重用 Codex 官方 curated 外掛，避免重複 marketplace。適合強調 brainstorm、規劃、TDD、系統除錯、審查與交付紀律的開發。感謝 Jesse Vincent 與貢獻者。",
    ja: "Codex 公式 curated プラグインを再利用し、marketplace の重複を避けます。計画、TDD、体系的デバッグ、レビュー、納品を重視する開発向け。Jesse Vincent と貢献者に感謝します。",
    ko: "Codex 공식 curated 플러그인을 재사용해 marketplace 중복을 피합니다. 계획, TDD, 체계적 디버깅, 리뷰, 전달 규율을 중시하는 개발에 적합합니다. Jesse Vincent와 기여자에게 감사합니다.",
  },
  gstack: {
    en: "Best for a broad product-to-ship workflow with browser, design, QA, review, and release skills. Requires Bun and Node.js. Thanks to Garry Tan and contributors.",
    "zh-CN": "适合覆盖产品、设计、浏览器验证、QA、审查到发布的完整流程；需要 Bun 与 Node.js。感谢 Garry Tan 与贡献者。",
    "zh-TW": "適合涵蓋產品、設計、瀏覽器驗證、QA、審查到發布的完整流程；需要 Bun 與 Node.js。感謝 Garry Tan 與貢獻者。",
    ja: "プロダクト、デザイン、ブラウザ検証、QA、レビュー、リリースまでの広い工程向け。Bun と Node.js が必要です。Garry Tan と貢献者に感謝します。",
    ko: "제품, 디자인, 브라우저 검증, QA, 리뷰, 릴리스까지 폭넓은 흐름에 적합하며 Bun과 Node.js가 필요합니다. Garry Tan과 기여자에게 감사합니다.",
  },
  "ralph-loop": {
    en: "Best for bounded unattended loops with explicit completion signals. It never starts automatically. Review Ralph's .ralph/.env before running because upstream defaults may bypass agent approvals. Thanks to Santander AI and contributors.",
    "zh-CN": "适合有明确完成信号的有界无人值守循环；不会自动启动。运行前请检查 Ralph 的 .ralph/.env，因为上游默认配置可能绕过代理审批。感谢 Santander AI 与贡献者。",
    "zh-TW": "適合有明確完成訊號的有界無人值守循環；不會自動啟動。執行前請檢查 Ralph 的 .ralph/.env，因為上游預設設定可能略過代理審批。感謝 Santander AI 與貢獻者。",
    ja: "明確な完了シグナルを持つ制限付き無人ループ向け。自動起動はしません。上流の既定値が承認を回避する場合があるため、実行前に .ralph/.env を確認してください。Santander AI と貢献者に感謝します。",
    ko: "명확한 완료 신호가 있는 제한된 무인 루프에 적합하며 자동 시작되지 않습니다. 업스트림 기본값이 승인 절차를 우회할 수 있으므로 실행 전에 .ralph/.env를 확인하세요. Santander AI와 기여자에게 감사합니다.",
  },
  "stop-that-shit": {
    en: "Checks explicit scope and execution boundaries. Review and trust its guard in /hooks before automatic hook execution. Thanks to @lennney and contributors.",
    "zh-CN": "按明确范围和执行边界进行检查；自动执行 Hook 前，请先在 /hooks 中审查并信任防护。感谢 @lennney 与贡献者。",
    "zh-TW": "依明確範圍和執行邊界進行檢查；自動執行 Hook 前，請先在 /hooks 中審查並信任防護。感謝 @lennney 與貢獻者。",
    ja: "明示した範囲と実行境界を検査します。Hook の自動実行前に /hooks で確認して信頼してください。@lennney と貢献者に感謝します。",
    ko: "명시한 범위와 실행 경계를 검사합니다. Hook 자동 실행 전에 /hooks에서 검토하고 신뢰하세요. @lennney와 기여자에게 감사합니다.",
  },
  "agent-guard": {
    en: "Runs local secret and safety checks. Run $setup-agent-guard, then review and trust it in /hooks. Thanks to @JeongJaeSoon and contributors.",
    "zh-CN": "执行本地密钥与安全检查。先运行 $setup-agent-guard，再通过 /hooks 审查并信任。感谢 @JeongJaeSoon 与贡献者。",
    "zh-TW": "執行本機密鑰與安全檢查。先執行 $setup-agent-guard，再透過 /hooks 審查並信任。感謝 @JeongJaeSoon 與貢獻者。",
    ja: "ローカルの秘密情報と安全性を検査します。$setup-agent-guard を実行し、/hooks で確認して信頼してください。@JeongJaeSoon と貢献者に感謝します。",
    ko: "로컬 비밀정보와 안전을 검사합니다. $setup-agent-guard를 실행한 뒤 /hooks에서 검토하고 신뢰하세요. @JeongJaeSoon과 기여자에게 감사합니다.",
  },
  "grill-me": {
    en: "Read-only Q&A that checks whether the developer understands maintained code. Trigger with $grill-me or /grill-me; it does not auto-run. Thanks to Joshua Wheelock.",
    "zh-CN": "通过只读问答检查开发者是否理解正在维护的代码。显式使用 $grill-me 或 /grill-me；默认不会自动触发。感谢 Joshua Wheelock。",
    "zh-TW": "透過唯讀問答檢查開發者是否理解正在維護的程式碼。請明確使用 $grill-me 或 /grill-me；預設不會自動觸發。感謝 Joshua Wheelock。",
    ja: "保守中のコード理解を読み取り専用 Q&A で確認します。$grill-me または /grill-me で明示的に起動し、自動実行はしません。Joshua Wheelock に感謝します。",
    ko: "유지 관리 중인 코드 이해도를 읽기 전용 Q&A로 확인합니다. $grill-me 또는 /grill-me로 명시적으로 실행하며 자동 실행되지 않습니다. Joshua Wheelock에게 감사합니다.",
  },
};

export const conflictMatrixCopy: Localized = {
  en: "Safety constraints can stack; Grill Me can coexist with every workflow. OpenSpec, Superpowers, and gstack are single-select. All in Luna and Ralph Loop are single-select. Switching is explicit and affects new sessions only; Headroom and port 6867 stay running.",
  "zh-CN": "安全约束可叠加，Grill Me 可与所有工作流共存。OpenSpec、Superpowers、gstack 主工作流单选；All in Luna、Ralph Loop 自动执行器单选。切换必须由用户明确触发，只影响新会话；Headroom 与 6867 端口保持运行。",
  "zh-TW": "安全約束可疊加，Grill Me 可與所有工作流程共存。OpenSpec、Superpowers、gstack 主要工作流程單選；All in Luna、Ralph Loop 自動執行器單選。切換必須由使用者明確觸發，只影響新工作階段；Headroom 與 6867 連接埠保持運行。",
  ja: "安全制約は併用でき、Grill Me はすべてのワークフローと共存できます。OpenSpec、Superpowers、gstack は単一選択、All in Luna と Ralph Loop も単一選択です。切り替えは明示操作で新しいセッションだけに反映され、Headroom と 6867 番ポートは動作を続けます。",
  ko: "안전 제약은 함께 사용할 수 있고 Grill Me는 모든 워크플로와 공존합니다. OpenSpec, Superpowers, gstack은 단일 선택이며 All in Luna와 Ralph Loop도 단일 선택입니다. 전환은 명시적으로 실행되어 새 세션에만 적용되고 Headroom과 6867 포트는 계속 실행됩니다.",
};

export function toolCategory(category?: string | null): ToolCategory {
  return TOOL_CATEGORY_ORDER.includes(category as ToolCategory)
    ? (category as ToolCategory)
    : "other";
}
