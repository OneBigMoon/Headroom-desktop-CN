export type ToolStatus = "not_installed" | "installing" | "healthy" | "degraded";

export interface ManagedTool {
  id: string;
  name: string;
  description: string;
  runtime: "python" | "binary" | "plugin";
  required: boolean;
  enabled: boolean;
  status: ToolStatus;
  sourceUrl: string;
  version: string;
  checksum?: string | null;
  savingsLabel?: string | null;
}

export interface PipelineStageMetric {
  stageId: string;
  stageName: string;
  applied: boolean;
  estimatedTokensSaved: number;
  addedLatencyMs: number;
  notes: string[];
}

export interface UsageEvent {
  id: string;
  timestamp: string;
  client: string;
  workspace: string;
  upstreamTarget: string;
  stages: PipelineStageMetric[];
  estimatedInputTokens: number;
  estimatedOutputTokens: number;
  estimatedCostSavingsUsd: number;
  latencyMs: number;
  outcome: "success" | "bypassed" | "error";
}

export interface DailyInsight {
  id: string;
  category: "savings" | "workflow" | "health";
  severity: "info" | "warning" | "critical";
  title: string;
  recommendation: string;
  evidence: string;
  relatedWorkspace?: string | null;
}

export interface ClientStatus {
  id: string;
  name: string;
  installed: boolean;
  configured: boolean;
  health: "healthy" | "attention" | "not_detected";
  notes: string[];
}

export type LaunchExperience = "first_run" | "resume" | "dashboard";

export interface DailySavingsPoint {
  date: string;
  estimatedSavingsUsd: number;
  estimatedTokensSaved: number;
  actualCostUsd: number;
  totalTokensSent: number;
  // Output-shaping layer, tracked separately from compression because it is a
  // counterfactual estimate. Zero for buckets the backend rolled up before the
  // layer existed, or that came from the local tracker.
  outputSavingsUsd?: number;
  outputTokensSaved?: number;
}

// Counterfactual output-token reduction from the proxy's output shaper.
// `method` is "estimated" (synthetic control vs a learned baseline) or
// "measured" (A/B holdout); the percentage carries a 95% confidence band.
export interface OutputReduction {
  method: string;
  reductionPercent: number;
  ciLowPercent: number;
  ciHighPercent: number;
  requests: number;
}

// Lifetime savings decomposition behind the headline card. cacheSavingsUsd is
// the provider cache discount earned by the client's own prompt caching --
// shown as its own labelled row, never summed into Headroom's savings.
export interface SavingsBreakdown {
  compressionSavingsUsd: number;
  outputSavingsUsd: number;
  // Optional: older payloads predate the tool-schema layer, matching the
  // container-level serde default on the Rust side.
  toolSchemaSavingsUsd?: number;
  toolSchemaTokensSaved?: number;
  cacheSavingsUsd: number;
  cacheReadTokens: number;
  totalInputTokens: number;
  totalInputCostUsd: number;
  // Optional for the same reason as the tool-schema fields above.
  modelRates?: ModelSavingsRate[];
}

// Rate only, no dollars: by_model tracking started well after the lifetime
// counters, so its totals cover a fraction of history. See ModelSavingsRate in
// models.rs.
export interface ModelSavingsRate {
  model: string;
  requests: number;
  savingsPercent: number;
}

export interface ProviderSavingsPoint {
  provider: string;
  estimatedSavingsUsd: number;
  estimatedTokensSaved: number;
  actualCostUsd: number;
  totalTokensSent: number;
}

export interface HourlySavingsPoint {
  hour: string;
  estimatedSavingsUsd: number;
  estimatedTokensSaved: number;
  actualCostUsd: number;
  totalTokensSent: number;
  outputSavingsUsd?: number;
  outputTokensSaved?: number;
  byProvider: ProviderSavingsPoint[];
}

export interface DashboardState {
  appVersion: string;
  launchExperience: LaunchExperience;
  bootstrapComplete: boolean;
  pythonRuntimeInstalled: boolean;
  lifetimeRequests: number;
  firstPromptRequestSeen: boolean;
  lifetimeEstimatedSavingsUsd: number;
  lifetimeEstimatedTokensSaved: number;
  sessionRequests: number;
  sessionEstimatedSavingsUsd: number;
  sessionEstimatedTokensSaved: number;
  sessionSavingsPct: number;
  outputReduction: OutputReduction | null;
  savingsBreakdown: SavingsBreakdown | null;
  dailySavings: DailySavingsPoint[];
  hourlySavings: HourlySavingsPoint[];
  savingsHistoryLoaded: boolean;
  tools: ManagedTool[];
  clients: ClientStatus[];
  recentUsage: UsageEvent[];
  insights: DailyInsight[];
  requiredTermsVersion: number;
  acceptedTermsVersion: number;
  termsUrl: string;
}

export interface BootstrapProgress {
  running: boolean;
  complete: boolean;
  failed: boolean;
  currentStep: string;
  message: string;
  currentStepEtaSeconds: number;
  overallPercent: number;
}

export interface ResearchCandidate {
  name: string;
  category: string;
  repository: string;
  runtime: string;
  license: string;
  localOnlyFit: string;
  installMethod: string;
  maintenance: string;
  decision: "include" | "defer" | "research";
  notes: string;
}

export interface ClientSetupResult {
  clientId: string;
  applied: boolean;
  alreadyConfigured: boolean;
  summary: string;
  changedFiles: string[];
  backupFiles: string[];
  nextSteps: string[];
  verification: ClientSetupVerification;
  shellProfileUnwritable?: boolean;
  replacedBaseUrl?: string | null;
}

export interface ClientSetupVerification {
  clientId: string;
  verified: boolean;
  proxyReachable: boolean;
  checks: string[];
  failures: string[];
}

export interface ClientConnectorStatus {
  clientId: string;
  name: string;
  installed: boolean;
  enabled: boolean;
  verified: boolean;
  lastConfiguredAt?: string | null;
}

export interface RuntimeStatus {
  platform: string;
  supportTier: string;
  installed: boolean;
  running: boolean;
  starting: boolean;
  paused: boolean;
  /** True when the watchdog auto-paused after giving up on a wedged proxy,
   *  distinct from a deliberate user pause. Drives the "stopped unexpectedly"
   *  banner + Resume button. */
  autoPaused: boolean;
  /** True when the proxy is intentionally bypassed (pricing gate on an
   *  unentitled account, or watchdog give-up). The backend is deliberately not
   *  started, so `running` will never become true — the first-run screen treats
   *  this as a terminal state and lets the user into the app. */
  bypassed: boolean;
  proxyReachable: boolean;
  headroomPid?: number | null;
  mcpConfigured?: boolean | null;
  mcpError?: string | null;
  mlInstalled?: boolean | null;
  kompressEnabled?: boolean | null;
  headroomLearnSupported: boolean;
  headroomLearnDisabledReason?: string | null;
  startupError?: string | null;
  startupErrorHint?: string | null;
  runtimeUpgradeFailure?: RuntimeUpgradeFailure | null;
  rtk: {
    installed: boolean;
    enabled: boolean;
    version?: string | null;
    pathConfigured: boolean;
    hookConfigured: boolean;
    totalCommands?: number | null;
    totalSaved?: number | null;
    avgSavingsPct?: number | null;
  };
}

export interface RuntimeUpgradeProgress {
  running: boolean;
  complete: boolean;
  failed: boolean;
  currentStep: string;
  message: string;
  overallPercent: number;
  fromVersion?: string | null;
  toVersion?: string | null;
}

export type UpgradeFailurePhase = "install" | "boot_validation";

export interface RuntimeUpgradeFailure {
  appVersion: string;
  targetHeadroomVersion: string;
  fallbackHeadroomVersion?: string | null;
  failurePhase: UpgradeFailurePhase;
  attempts: number;
  firstAttemptAt: string;
  lastAttemptAt: string;
  errorMessage: string;
  errorHint?: string | null;
  rollbackRestored: boolean;
}

export interface AppUpdateConfiguration {
  enabled: boolean;
  currentVersion: string;
  endpointCount: number;
  configurationError?: string | null;
  betaChannelEnabled: boolean;
}

export interface AvailableAppUpdate {
  currentVersion: string;
  version: string;
  publishedAt?: string | null;
  notes?: string | null;
}

export interface ClaudeCodeProject {
  id: string;
  projectPath: string;
  displayName: string;
  lastWorkedAt: string;
  sessionCount: number;
  lastLearnRanAt: string | null;
  hasPersistedLearnings: boolean;
  activeDaysSinceLastLearn: number;
  lastLearnPatternCount: number | null;
}

export interface HeadroomLearnStatus {
  running: boolean;
  projectPath?: string | null;
  projectDisplayName?: string | null;
  startedAt?: string | null;
  finishedAt?: string | null;
  elapsedSeconds?: number | null;
  progressPercent: number;
  summary: string;
  success?: boolean | null;
  error?: string | null;
  lastRunAt?: string | null;
  outputTail: string[];
}

export interface HeadroomLearnPrereqStatus {
  claudeCliAvailable: boolean;
  claudeCliPath?: string | null;
  codexCliAvailable: boolean;
  codexCliPath?: string | null;
  codexLoggedIn: boolean;
}

// A single entry in `requestMessages`. Intentionally loose — the proxy passes
// through whatever shape the upstream provider uses (Anthropic: `content` is a
// string or structured blocks list; OpenAI: string-only). The UI extracts
// displayable text in `ActivityFeed.tsx`.
export interface TransformationRequestMessage {
  role?: string;
  content?: string | Array<{ type?: string; text?: string; [k: string]: unknown }>;
  [k: string]: unknown;
}

export interface TransformationFeedEvent {
  requestId?: string | null;
  timestamp?: string | null;
  provider?: string | null;
  model?: string | null;
  inputTokensOriginal?: number | null;
  inputTokensOptimized?: number | null;
  tokensSaved?: number | null;
  savingsPercent?: number | null;
  transformsApplied: string[];
  workspace?: string | null;
  turnId?: string | null;
  // Populated only when the proxy was started with `--log-messages` (or
  // `HEADROOM_LOG_MESSAGES=1`), reflected in
  // `TransformationFeedResponse.logFullMessages`. Both fields are
  // pass-through from the proxy's `RequestLogger` — the desktop renders
  // them, it does not reinterpret them.
  //
  // `compressedMessages` is the post-compression message list that was
  // actually sent upstream; paired with `requestMessages` it lets consumers
  // see what Headroom's pipeline stripped, replaced, or kept. Absent on
  // proxies that predate the field.
  requestMessages?: TransformationRequestMessage[] | null;
  compressedMessages?: TransformationRequestMessage[] | null;
}

export interface TransformationFeedResponse {
  logFullMessages: boolean;
  proxyReachable: boolean;
  transformations: TransformationFeedEvent[];
}

export interface LiveLearning {
  id: string;
  content: string;
  category: string;
  importance: number;
  evidenceCount: number;
  createdAt: string;
}

export interface AppliedSection {
  title: string;
  bullets: string[];
}

export interface AppliedPatterns {
  claudeMd: AppliedSection[];
  memoryMd: AppliedSection[];
}

export interface RtkTodayStats {
  date: string;
  savedTokens: number;
  commands: number;
}

export type RecordTag = "daily" | "weekly" | "allTime";

export interface RecordEvent {
  observedAt: string;
  tags: RecordTag[];
  tokensSaved: number;
  savingsPercent: number | null;
  model: string | null;
  provider: string | null;
  requestId: string | null;
  previousRecord: number | null;
  day: string | null;
  workspace?: string | null;
  inputTokensOriginal?: number | null;
  inputTokensOptimized?: number | null;
  // Carried forward from the record-setting transformation so the record row
  // can surface the same request/compressed detail as the compression card.
  // Populated only when the proxy's `log_full_messages` is enabled;
  // `compressedMessages` additionally requires a proxy that carries the
  // field (see TransformationFeedEvent above).
  requestMessages?: TransformationRequestMessage[] | null;
  compressedMessages?: TransformationRequestMessage[] | null;
}

export interface WeeklyRecapEvent {
  observedAt: string;
  weekStart: string;
  weekEnd: string;
  totalTokensSaved: number;
  totalSavingsUsd: number;
  activeDays: number;
}

export interface LearningsMilestoneEvent {
  observedAt: string;
  patternsToday: number;
  remindersToday: number;
  learningsToday: number;
  projectPath: string | null;
  projectDisplayName: string | null;
}

export interface TrainSuggestionEvent {
  observedAt: string;
  projectPath: string;
  projectDisplayName: string;
  sessionCount: number;
  activeDaysSinceLastLearn: number;
  // "never_trained" | "stale"
  kind: string;
}

export interface ActivityFeedSnapshot {
  transformation: TransformationFeedEvent | null;
  record: RecordEvent | null;
  rtkToday: RtkTodayStats | null;
  learningsMilestone: LearningsMilestoneEvent | null;
  weeklyRecap: WeeklyRecapEvent | null;
  trainSuggestion: TrainSuggestionEvent | null;
}

export interface ActivityFeedResponse {
  tiles: ActivityFeedSnapshot;
  proxyReachable: boolean;
}

export type ClaudeAuthMethod = "claude_ai_oauth" | "api_key" | "unknown";

export type ClaudePlanTier = "free" | "pro" | "max5x" | "max20x" | "unknown";

export type HeadroomSubscriptionTier = "pro" | "max5x" | "max20x";

export type CodexPlanTier =
  | "free"
  | "go"
  | "plus"
  | "pro"
  | "team"
  | "business"
  | "self_serve_business_usage_based"
  | "enterprise"
  | "enterprise_cbp_usage_based"
  | "edu"
  | "unknown";

// Mirrors `LaunchFlags` in lib.rs; served cached-or-default, never blocking.
export interface LaunchFlags {
  paywallFirst: boolean;
}

export type BillingPeriod = "annual" | "monthly";

export type PricingGateReason =
  | "sign_in_required"
  | "weekly_usage_limit_reached"
  | "codex_weekly_usage_limit_reached"
  | "trial_ended";

export interface ClaudeAccountProfile {
  authMethod: ClaudeAuthMethod;
  email?: string | null;
  displayName?: string | null;
  accountUuid?: string | null;
  organizationUuid?: string | null;
  billingType?: string | null;
  accountCreatedAt?: string | null;
  subscriptionCreatedAt?: string | null;
  hasExtraUsageEnabled: boolean;
  planTier: ClaudePlanTier;
  planDetectionSource?: string | null;
  weeklyUtilizationPct?: number | null;
  weeklyResetsAt?: string | null;
  fiveHourUtilizationPct?: number | null;
  extraUsageMonthlyLimit?: number | null;
  profileFetchError?: string | null;
}

export interface CodexUsageWindow {
  usedPercent: number;
  windowLabel?: string | null;
  windowMinutes?: number | null;
  secondsUntilReset?: number | null;
}

export interface CodexUsage {
  limitName?: string | null;
  primary?: CodexUsageWindow | null;
  secondary?: CodexUsageWindow | null;
  creditsBalance?: string | null;
  creditsUnlimited: boolean;
  optimizationAllowed: boolean;
  shouldNudge: boolean;
  nudgeLevel: number;
  gateReason?: PricingGateReason | null;
  recommendedSubscriptionTier?: HeadroomSubscriptionTier | null;
  weeklyUsedPercent?: number | null;
  gateMessage: string;
  effectiveNudgeThresholdsPercent?: number[] | null;
  effectiveDisableThresholdPercent?: number | null;
}

export interface HeadroomAccountProfile {
  email: string;
  trialStartedAt?: string | null;
  trialEndsAt?: string | null;
  trialActive: boolean;
  subscriptionActive: boolean;
  subscriptionTier?: HeadroomSubscriptionTier | null;
  subscriptionStartedAt?: string | null;
  subscriptionRenewsAt?: string | null;
  subscriptionAmountCents?: number | null;
  subscriptionBillingPeriod?: string | null;
  subscriptionDiscountDuration?: string | null;
  subscriptionDiscountDurationInMonths?: number | null;
  subscriptionCancelAtPeriodEnd?: boolean;
  subscriptionEndsAt?: string | null;
  inviteCode?: string | null;
  acceptedInvitesCount: number;
  inviteBonusPercent: number;
}

export interface HeadroomPricingStatus {
  authenticated: boolean;
  localGraceStartedAt: string;
  localGraceEndsAt: string;
  localGraceActive: boolean;
  accountSyncError?: string | null;
  needsAuthentication: boolean;
  optimizationAllowed: boolean;
  shouldNudge: boolean;
  nudgeLevel: number;
  gateReason?: PricingGateReason | null;
  gateMessage: string;
  nudgeThresholdPercent?: number | null;
  effectiveNudgeThresholdsPercent?: number[] | null;
  disableThresholdPercent?: number | null;
  effectiveDisableThresholdPercent?: number | null;
  recommendedSubscriptionTier?: HeadroomSubscriptionTier | null;
  tierMismatch?: TierMismatch | null;
  claude: ClaudeAccountProfile;
  codex?: CodexUsage | null;
  codexPlanTier?: CodexPlanTier | null;
  account?: HeadroomAccountProfile | null;
  launchDiscountActive: boolean;
  activePercentOff?: number;
  pricingCohorts?: PricingCohort[];
  introOffer?: IntroOffer | null;
}

export interface PricingCohort {
  key: string;
  label: string;
  percentOff: number;
  capacity?: number | null;
  status: "sold_out" | "active" | "upcoming";
  spotsLeft?: number | null;
}

/// Slack-style intro offer surfaced by headroom-web: percentOff for the first
/// durationMonths months on every plan.
export interface IntroOffer {
  active: boolean;
  percentOff: number;
  durationMonths: number;
}

export type TierRecommendationSource = "claude" | "codex" | "both";

export interface TierMismatch {
  paidTier: HeadroomSubscriptionTier;
  recommendedTier: HeadroomSubscriptionTier;
  recommendedSource: TierRecommendationSource;
  graceEndsAt: string;
  clamped: boolean;
}

export interface HeadroomAuthCodeRequest {
  email: string;
  expiresInSeconds: number;
}
