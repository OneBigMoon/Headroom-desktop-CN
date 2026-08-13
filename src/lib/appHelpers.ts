import type {
  BillingPeriod,
  ClientConnectorStatus,
  RuntimeStatus,
  DailySavingsPoint,
  HeadroomPricingStatus,
  HeadroomSubscriptionTier,
  IntroOffer,
  PlanPrices,
  TierRecommendationSource,
} from "./types";
import { currencyExact } from "./dashboardHelpers";
// Type-only, so this stays free of setupHealthAlert's Tauri imports at runtime.
import type { SetupStallKind } from "./setupHealthAlert";

export type PricingAudience = "individual" | "teamEnterprise";
export type { BillingPeriod };

/// Fallback price table, compiled into the build. headroom-web serves the
/// live table (`planPrices`); this is what we quote when the server predates
/// that field, is unreachable, or omits a tier. Keep it in sync with
/// Polar::ProductCatalog::LIST_CENTS on the server.
const PLAN_PRICES: Record<
  "pro" | "max5x" | "max20x",
  Record<BillingPeriod, { full: string; fullCents: number }>
> = {
  pro:   { annual: { full: "$3",  fullCents: 300  }, monthly: { full: "$4",  fullCents: 400  } },
  max5x: { annual: { full: "$15",   fullCents: 1500 }, monthly: { full: "$20", fullCents: 2000 } },
  max20x:{ annual: { full: "$30",   fullCents: 3000 }, monthly: { full: "$40", fullCents: 4000 } },
};

/// The server's price table, mirrored here at render time by
/// `setServerPlanPrices`. Module-level rather than threaded through the
/// pricing helpers because all of them (plan cards, payback anchor, renewal
/// projections) need it, and it is server config rather than per-call input.
let serverPlanPrices: PlanPrices | null = null;

/// Point the price helpers at the server's table (null to fall back). Called
/// during render from the pricing status, so a price change reaches the UI
/// without an app release.
export function setServerPlanPrices(prices: PlanPrices | null | undefined): void {
  serverPlanPrices = prices ?? null;
}

/// Price for a tier/period: the server's number when it sent a usable one,
/// else the compiled-in fallback. Non-finite or negative server values are
/// rejected rather than rendered as "$NaN".
function planPrice(
  tier: "pro" | "max5x" | "max20x",
  billingPeriod: BillingPeriod
): { full: string; fullCents: number } {
  const cents = serverPlanPrices?.[tier]?.[billingPeriod];
  if (typeof cents === "number" && Number.isFinite(cents) && cents >= 0) {
    return { full: formatCents(cents), fullCents: cents };
  }
  return PLAN_PRICES[tier][billingPeriod];
}

// Discounted price for a tier, mirroring the web `sale_price_cents` rounding
// (half-up to the cent) so desktop and marketing prices never disagree.
function discountedPriceLabel(fullCents: number, percentOff: number): string {
  return formatCents(Math.round((fullCents * (100 - percentOff)) / 100));
}
const TIER_RANK: Record<HeadroomSubscriptionTier, number> = { pro: 1, max5x: 2, max20x: 3 };

/// Whether the billing-period tab being viewed is the one the subscriber
/// actually bought. An unknown/absent server value means "can't tell" - treat
/// it as a match so the active-plan chrome never vanishes on older servers.
export function matchesSubscriptionPeriod(
  billingPeriod: BillingPeriod,
  subscriptionBillingPeriod?: string | null
): boolean {
  if (subscriptionBillingPeriod !== "annual" && subscriptionBillingPeriod !== "monthly") {
    return true;
  }
  return subscriptionBillingPeriod === billingPeriod;
}

export function isTierDowngrade(
  fromTier: HeadroomSubscriptionTier,
  toTier: HeadroomSubscriptionTier
): boolean {
  return TIER_RANK[toTier] < TIER_RANK[fromTier];
}

function projectPerMonthCents(
  toTier: HeadroomSubscriptionTier,
  billingPeriod: BillingPeriod,
  options?: { fromTier?: HeadroomSubscriptionTier; currentPaidCents?: number | null }
): number {
  // PLAN_PRICES.fullCents is per-month even on annual cycles.
  const toFullPerMonth = planPrice(toTier, billingPeriod).fullCents;
  const fromTier = options?.fromTier;
  const currentPaidCents = options?.currentPaidCents ?? null;
  if (!fromTier || currentPaidCents === null) return toFullPerMonth;
  const fromFullPerMonth = planPrice(fromTier, billingPeriod).fullCents;
  if (fromFullPerMonth <= 0) return toFullPerMonth;
  // Polar reports subscription_amount_cents per full billing cycle (12x
  // per-month for annual), so normalize to per-month before the ratio math.
  const cycleMonths = billingPeriod === "annual" ? 12 : 1;
  const currentPaidPerMonth = currentPaidCents / cycleMonths;
  return Math.round(toFullPerMonth * (currentPaidPerMonth / fromFullPerMonth));
}

export function formatCents(cents: number): string {
  const dollars = cents / 100;
  return cents % 100 === 0 ? `$${dollars}` : `$${dollars.toFixed(2)}`;
}

/// Per-month price label for the target tier (e.g. `$20 / month`), with the
/// user's current discount ratio carried forward. Matches the upgrade view
/// convention where annual prices are shown per-month for tier comparison.
export function getPlanRenewalPriceLabel(
  toTier: HeadroomSubscriptionTier,
  billingPeriod: BillingPeriod,
  options?: { fromTier?: HeadroomSubscriptionTier; currentPaidCents?: number | null }
): string {
  return `${formatCents(projectPerMonthCents(toTier, billingPeriod, options))} / month`;
}

/// The intro percent while the offer runs, else 0. Both billing periods show
/// the straight percent off the period's sticker price; on annual the charge
/// still works out identically (6 half-price + 6 full months per invoice).
export function introPercentOff(introOffer: IntroOffer | null | undefined): number {
  if (!introOffer?.active || introOffer.percentOff <= 0) return 0;
  return introOffer.percentOff;
}

/// Sale-badge copy for the intro offer, or null when it is not running.
export function introSaleBadgeLabel(introOffer: IntroOffer | null | undefined): string | null {
  const pct = introPercentOff(introOffer);
  if (pct <= 0 || !introOffer) return null;
  return `${pct}% off first ${introOffer.durationMonths} months`;
}

/// Average daily savings over the trailing `days` window (default 7), used to
/// project realized/forgone savings for upgrade copy. Returns 0 with no history.
export function recentDailySavingsUsd(daily: DailySavingsPoint[], days = 7): number {
  if (daily.length === 0) return 0;
  const window = daily.slice(-days);
  const total = window.reduce((sum, p) => sum + p.estimatedSavingsUsd, 0);
  return total / window.length;
}

/// Item 1 - "pays for itself" anchor. Compares the user's recent monthly
/// savings rate against the per-month price of `planId`. Only surfaces at a
/// genuine value-add (>= 2x the price); returns null below that so a weak claim
/// never deters an upgrade. Floors the multiple so it never overstates.
export function paybackLabel(
  recentMonthlySavingsUsd: number,
  planId: HeadroomSubscriptionTier,
  billingPeriod: BillingPeriod
): string | null {
  const monthly = planPrice(planId, billingPeriod).fullCents / 100;
  if (monthly <= 0) return null;
  const multiple = recentMonthlySavingsUsd / monthly;
  if (multiple < 2) return null;
  return `You're saving about ${currencyExact(recentMonthlySavingsUsd)} a month. Upgrading pays for itself ${Math.floor(multiple)}x over.`;
}

/// Item 2 - counterfactual for the weekly gate. Projects the savings forgone
/// while optimization is paused until the weekly limit resets. `daysUntilReset`
/// may be fractional. Returns null below $1 so we don't nag over trivial sums.
export function forgoneSavingsLabel(
  recentDailySavingsUsd: number,
  daysUntilReset: number
): string | null {
  if (recentDailySavingsUsd <= 0 || daysUntilReset <= 0) return null;
  const forgone = recentDailySavingsUsd * daysUntilReset;
  if (forgone < 1) return null;
  return `You'll miss out on about ${currencyExact(forgone)} in savings this week unless you upgrade.`;
}

export type UpgradePlanId = "free" | "pro" | "max5x" | "max20x" | "team" | "enterprise";
type IndividualUpgradePlanId = "free" | "pro" | "max5x" | "max20x";
type PaidUpgradePlanId = HeadroomSubscriptionTier;

const INDIVIDUAL_PLAN_ORDER: IndividualUpgradePlanId[] = ["free", "pro", "max5x", "max20x"];

export interface UpgradePlanPurchaseInfo {
  renewsOn: string;
  paidPerMonthLabel: string;
  discountPct: number;
  cancelAtPeriodEnd?: boolean;
  endsOn?: string;
}

export interface UpgradePlan {
  id: UpgradePlanId;
  name: string;
  tagline: string;
  price: string;
  originalPrice?: string;
  billingLines: [string, string];
  /// Full-width line under the price row spelling out the post-intro rate.
  reversionLine?: string;
  centeredPriceLabel?: string;
  featureIntro: string;
  features: string[];
  ctaLabel: string;
  ctaVariant: "primary" | "secondary";
  ctaTone?: "default" | "downgrade";
  disabled?: boolean;
  purchaseInfo?: UpgradePlanPurchaseInfo;
}

export function upgradePlanIntentLabel(planId: UpgradePlanId | null) {
  switch (planId) {
    case "pro":
      return "Pro";
    case "max5x":
      return "Max x5";
    case "max20x":
      return "Max x20";
    default:
      return null;
  }
}

// Connector(s) whose detected plan drives a tier-mismatch recommendation, for
// the upgrade banner copy.
export function tierRecommendationSourceLabel(source: TierRecommendationSource) {
  switch (source) {
    case "codex":
      return "Codex";
    case "both":
      return "Claude and Codex";
    default:
      return "Claude";
  }
}

export function describeInvokeError(error: unknown, fallback: string) {
  if (error instanceof Error && error.message.trim()) {
    return error.message;
  }
  if (typeof error === "string" && error.trim()) {
    return error;
  }
  if (
    typeof error === "object" &&
    error !== null &&
    "message" in error &&
    typeof error.message === "string" &&
    error.message.trim()
  ) {
    return error.message;
  }
  if (
    typeof error === "object" &&
    error !== null &&
    "error" in error &&
    typeof error.error === "string" &&
    error.error.trim()
  ) {
    return error.error;
  }
  return fallback;
}

export function getNextLowerUpgradePlanId(
  planId?: PaidUpgradePlanId | null
): IndividualUpgradePlanId | null {
  switch (planId) {
    case "pro":
      // No free plan to downgrade to post-trial.
      return null;
    case "max5x":
      return "pro";
    case "max20x":
      return "max5x";
    default:
      return null;
  }
}

export function getUpgradePlans(
  audience: PricingAudience,
  claudePlanTier?: HeadroomPricingStatus["claude"]["planTier"],
  recommendedSubscriptionTier?: HeadroomPricingStatus["recommendedSubscriptionTier"],
  headroomSubscriptionTier?: HeadroomSubscriptionTier | null,
  hasActiveHeadroomSubscription = false,
  launchDiscountActive = false,
  billingPeriod: BillingPeriod = "annual",
  subscriptionAmountCents?: number | null,
  subscriptionBillingPeriod?: string | null,
  subscriptionRenewsAt?: string | null,
  subscriptionStartedAt?: string | null,
  subscriptionDiscountDuration?: string | null,
  subscriptionDiscountDurationInMonths?: number | null,
  subscriptionCancelAtPeriodEnd: boolean = false,
  subscriptionEndsAt?: string | null,
  activePercentOff: number = 0,
  introOffer: IntroOffer | null = null,
  subscriptionRenewalCents?: number | null,
  subscriptionRenewalEndsAt?: string | null
): {
  plans: UpgradePlan[];
  featuredPlanId: UpgradePlanId;
} {
  if (audience === "individual") {
    // No free plan post-trial: the upgrade sheet only offers paid plans.
    const billingLabel = billingPeriod === "annual" ? "billed annually" : "billed monthly";

    const activeHeadroomPlanId =
      hasActiveHeadroomSubscription && headroomSubscriptionTier
        ? headroomSubscriptionTier
        : null;
    // The subscription lives on one billing period. On the other tab the same
    // tier is a switch offer, not the plan you are on.
    const periodMatches = matchesSubscriptionPeriod(billingPeriod, subscriptionBillingPeriod);

    // Compute purchase info for the active plan card when data is available.
    const activePurchaseInfo = ((): UpgradePlanPurchaseInfo | undefined => {
      if (!activeHeadroomPlanId || subscriptionAmountCents == null) {
        return undefined;
      }
      const purchasePeriod = (subscriptionBillingPeriod === "annual" || subscriptionBillingPeriod === "monthly")
        ? subscriptionBillingPeriod
        : billingPeriod;
      const fullCents = planPrice(activeHeadroomPlanId, purchasePeriod).fullCents;

      // Determine if the discount will still apply at renewal time.
      const discountAppliesAtRenewal = ((): boolean => {
        if (!subscriptionDiscountDuration) return false;
        if (subscriptionDiscountDuration === "forever") return true;
        if (subscriptionDiscountDuration === "once") return false;
        // "repeating": check if renewal falls within the discount window
        if (
          subscriptionDiscountDuration === "repeating" &&
          subscriptionDiscountDurationInMonths != null &&
          subscriptionStartedAt &&
          subscriptionRenewsAt
        ) {
          const discountExpiresAt = new Date(subscriptionStartedAt);
          discountExpiresAt.setMonth(discountExpiresAt.getMonth() + subscriptionDiscountDurationInMonths);
          return new Date(subscriptionRenewsAt) < discountExpiresAt;
        }
        return false;
      })();

      // Amount is stored as per-billing-cycle cents; convert to per-month.
      const paidCentsPerMonth = purchasePeriod === "annual"
        ? subscriptionAmountCents / 12
        : subscriptionAmountCents;

      // The server's own figure wins when it has one: a discount attached
      // mid-subscription (the cancellation save offer) can't be dated from
      // subscriptionStartedAt, so the window check above reads it as expired.
      const serverRenewalCents =
        subscriptionRenewalCents != null &&
        subscriptionRenewalEndsAt != null &&
        new Date(subscriptionRenewalEndsAt) > new Date()
          ? subscriptionRenewalCents
          : null;

      // If the discount won't apply at renewal, show full price for the renewal.
      const renewalCentsPerMonth = serverRenewalCents != null
        ? (purchasePeriod === "annual" ? serverRenewalCents / 12 : serverRenewalCents)
        : discountAppliesAtRenewal ? paidCentsPerMonth : fullCents;
      const discountPct = fullCents > 0 && renewalCentsPerMonth < fullCents
        ? Math.round((1 - renewalCentsPerMonth / fullCents) * 100)
        : 0;
      const paidPerMonthLabel = `$${(renewalCentsPerMonth / 100).toFixed(2).replace(/\.00$/, "")}`;
      const renewsOn = subscriptionRenewsAt
        ? new Date(subscriptionRenewsAt).toLocaleDateString("en-US", { month: "short", day: "numeric", year: "numeric" })
        : null;
      if (!renewsOn) return undefined;
      const endsOn = subscriptionCancelAtPeriodEnd && subscriptionEndsAt
        ? new Date(subscriptionEndsAt).toLocaleDateString("en-US", { month: "short", day: "numeric", year: "numeric" })
        : undefined;
      return {
        renewsOn,
        paidPerMonthLabel,
        discountPct,
        cancelAtPeriodEnd: subscriptionCancelAtPeriodEnd,
        endsOn
      };
    })();

    function paidPlan(
      id: "pro" | "max5x" | "max20x",
      name: string,
      tagline: string,
      featureIntro: string,
      features: string[],
      ctaLabel: string
    ): UpgradePlan {
      const prices = planPrice(id, billingPeriod);
      // Upgrade-target cards show the discounted price because checkout
      // attaches the matching Polar discount server-side; the active plan card
      // uses purchaseInfo (actual paid amount) instead of a generic badge.
      // Subscribers with a discount that survives renewal (forever, or repeating
      // still in window) keep it on plan swaps (Polar carries it over), so their
      // own percent wins over the intro offer. The activePercentOff/50 fallback
      // serves legacy cohort servers that still signal launchDiscountActive.
      const accountDiscountPct = activePurchaseInfo?.discountPct ?? 0;
      const introPct = introPercentOff(introOffer);
      const effectivePercentOff = accountDiscountPct > 0
        ? accountDiscountPct
        : introPct > 0 ? introPct
        : activePercentOff > 0 ? activePercentOff : 50;
      const showDiscount =
        (introPct > 0 || launchDiscountActive || accountDiscountPct > 0) &&
        id !== activeHeadroomPlanId;
      const price = showDiscount
        ? discountedPriceLabel(prices.fullCents, effectivePercentOff)
        : prices.full;
      return {
        id,
        name,
        tagline,
        price,
        ...(showDiscount ? { originalPrice: prices.full } : {}),
        ...(id === activeHeadroomPlanId && periodMatches && activePurchaseInfo
          ? { purchaseInfo: activePurchaseInfo }
          : {}),
        billingLines: ["USD / month", billingLabel],
        // Full-width line under the price so "$10/mo billed annually" can't
        // be misread as the full-year rate; the badge names the duration.
        ...(showDiscount && introPct > 0 && introOffer
          ? { reversionLine: `then ${prices.full}/mo after ${introOffer.durationMonths} months` }
          : {}),
        featureIntro,
        features,
        ctaLabel,
        ctaVariant: "primary",
        ctaTone: "default"
      };
    }

    const paidPlans: Record<"pro" | "max5x" | "max20x", UpgradePlan> = {
      pro: paidPlan("pro", "Pro", "Unlock unlimited savings", "Everything in Free, plus:", [
        "Unlimited use with Claude Pro or ChatGPT Plus",
        "Use on all your devices with one account",
        "Email-based support"
      ], "Get Pro"),
      max5x: paidPlan("max5x", "Max x5", "For Claude Max x5 or ChatGPT Pro x5 accounts", "Includes:", [
        "Unlimited use with Claude Max x5 or ChatGPT Pro x5",
        "Use on all your devices with one account",
        "Email-based support"
      ], "Get Max x5"),
      max20x: paidPlan("max20x", "Max x20", "For Claude Max x20 or ChatGPT Pro x20 accounts", "Includes:", [
        "Unlimited use with Claude Max x20 or ChatGPT Pro x20",
        "Use on all your devices with one account",
        "Priority support"
      ], "Get Max x20"),
    };

    const withRelativeCta = (plan: UpgradePlan): UpgradePlan => {
      if (!activeHeadroomPlanId) {
        return plan;
      }

      // Free card during a scheduled downgrade is the pending target - its
      // CTA was set to "Downgrade scheduled" above and must not be overridden.
      if (plan.purchaseInfo?.cancelAtPeriodEnd && plan.id !== activeHeadroomPlanId) {
        return plan;
      }

      const planRank = INDIVIDUAL_PLAN_ORDER.indexOf(plan.id as IndividualUpgradePlanId);
      const activeRank = INDIVIDUAL_PLAN_ORDER.indexOf(activeHeadroomPlanId);
      if (planRank === -1 || activeRank === -1) {
        return plan;
      }

      if (plan.id === activeHeadroomPlanId) {
        return periodMatches
          ? {
              ...plan,
              ctaLabel: `Stay on ${plan.name} plan`,
              ctaVariant: "secondary",
              ctaTone: "default"
            }
          : {
              ...plan,
              ctaLabel: `Switch to ${billingPeriod === "annual" ? "annual" : "monthly"} billing`,
              ctaVariant: "primary",
              ctaTone: "default"
            };
      }

      if (planRank < activeRank) {
        return {
          ...plan,
          ctaLabel: `Downgrade to ${plan.name} plan`,
          ctaVariant: "secondary",
          ctaTone: "downgrade"
        };
      }

      return {
        ...plan,
        ctaLabel: `Upgrade to ${plan.name}`,
        ctaVariant: "primary",
        ctaTone: "default"
      };
    };

    if (activeHeadroomPlanId) {
      const orderedPaidPlans = [
        paidPlans[activeHeadroomPlanId],
        ...(["pro", "max5x", "max20x"] as const)
          .filter((planId) => planId !== activeHeadroomPlanId)
          .map((planId) => paidPlans[planId])
      ].map(withRelativeCta);
      return {
        plans: orderedPaidPlans,
        featuredPlanId: activeHeadroomPlanId
      };
    }

    const activePaidPlanId = (() => {
      switch (claudePlanTier) {
        case "pro":
          return "pro" as const;
        case "max5x":
          return "max5x" as const;
        case "max20x":
          return "max20x" as const;
        default:
          return headroomSubscriptionTier ?? null;
      }
    })();

    if (activePaidPlanId) {
      const orderedPaidPlans = [
        paidPlans[activePaidPlanId],
        ...(["pro", "max5x", "max20x"] as const)
          .filter((planId) => planId !== activePaidPlanId)
          .map((planId) => paidPlans[planId])
      ];
      return {
        plans: orderedPaidPlans,
        featuredPlanId: activePaidPlanId
      };
    }

    if (recommendedSubscriptionTier) {
      const orderedPaidPlans = [
        paidPlans[recommendedSubscriptionTier],
        ...(["pro", "max5x", "max20x"] as const)
          .filter((planId) => planId !== recommendedSubscriptionTier)
          .map((planId) => paidPlans[planId])
      ];
      return {
        plans: orderedPaidPlans,
        featuredPlanId: recommendedSubscriptionTier
      };
    }

    // No recommendation (e.g. not signed in yet) or an unknown Claude plan:
    // default to Max x5 as the featured plan.
    return {
      plans: [paidPlans.max5x, paidPlans.pro, paidPlans.max20x],
      featuredPlanId: "max5x"
    };
  }

  return {
    plans: [
      {
        id: "enterprise",
        name: "Team & Enterprise",
        tagline: "Shared controls, governance, and private deployment options",
        price: "",
        billingLines: ["", ""],
        centeredPriceLabel: "custom pricing • contact us",
        featureIntro: "",
        features: [],
        ctaLabel: "Submit",
        ctaVariant: "primary",
        ctaTone: "default"
      }
    ],
    featuredPlanId: "enterprise"
  };
}

/// Support mail for the setup-stall alert. Carries the state that decided which
/// branch fired, so a reply doesn't have to start by asking for all of it.
export function buildSetupStallMailto(
  kind: SetupStallKind,
  context: {
    appVersion: string;
    lifetimeRequests: number;
    runtime: RuntimeStatus | null;
    connectors: ClientConnectorStatus[];
  }
): string {
  const subject = `Headroom is not saving anything (${kind})`;
  const connectorLines = context.connectors.length
    ? context.connectors.map(
        (connector) =>
          `  ${connector.name}: installed=${connector.installed} enabled=${connector.enabled} verified=${connector.verified}`
      )
    : ["  (none reported)"];
  const diagnosticLines = [
    `Alert: ${kind}`,
    `App version: ${context.appVersion}`,
    `Lifetime requests seen: ${context.lifetimeRequests}`,
    `Runtime: installed=${context.runtime?.installed ?? "unknown"} running=${
      context.runtime?.running ?? "unknown"
    } paused=${context.runtime?.paused ?? "unknown"} proxyReachable=${
      context.runtime?.proxyReachable ?? "unknown"
    }`,
    "Connectors:",
    ...connectorLines,
  ];
  const body =
    "Which coding agent are you using, and how do you launch it?\n\n\n" +
    "---\n" +
    "Diagnostic info (please keep):\n" +
    diagnosticLines.join("\n");
  return `mailto:support@extraheadroom.com?subject=${encodeURIComponent(
    subject
  )}&body=${encodeURIComponent(body)}`;
}
