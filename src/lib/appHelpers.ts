import type {
  BillingPeriod,
  DailySavingsPoint,
  HeadroomPricingStatus,
  HeadroomSubscriptionTier,
  IntroOffer,
  TierRecommendationSource,
} from "./types";
import { currencyExact } from "./dashboardHelpers";

export type PricingAudience = "individual" | "teamEnterprise";
export type { BillingPeriod };

const PLAN_PRICES: Record<
  "pro" | "max5x" | "max20x",
  Record<BillingPeriod, { full: string; fullCents: number }>
> = {
  pro:   { annual: { full: "$5",  fullCents: 500  }, monthly: { full: "$7.50", fullCents: 750  } },
  max5x: { annual: { full: "$20", fullCents: 2000 }, monthly: { full: "$30",   fullCents: 3000 } },
  max20x:{ annual: { full: "$40", fullCents: 4000 }, monthly: { full: "$60",   fullCents: 6000 } },
};

// Discounted price for a tier, mirroring the web `sale_price_cents` rounding
// (half-up to the cent) so desktop and marketing prices never disagree.
function discountedPriceLabel(fullCents: number, percentOff: number): string {
  return formatCents(Math.round((fullCents * (100 - percentOff)) / 100));
}
const TIER_RANK: Record<HeadroomSubscriptionTier, number> = { pro: 1, max5x: 2, max20x: 3 };

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
  const toFullPerMonth = PLAN_PRICES[toTier][billingPeriod].fullCents;
  const fromTier = options?.fromTier;
  const currentPaidCents = options?.currentPaidCents ?? null;
  if (!fromTier || currentPaidCents === null) return toFullPerMonth;
  const fromFullPerMonth = PLAN_PRICES[fromTier][billingPeriod].fullCents;
  if (fromFullPerMonth <= 0) return toFullPerMonth;
  // Polar reports subscription_amount_cents per full billing cycle (12x
  // per-month for annual), so normalize to per-month before the ratio math.
  const cycleMonths = billingPeriod === "annual" ? 12 : 1;
  const currentPaidPerMonth = currentPaidCents / cycleMonths;
  return Math.round(toFullPerMonth * (currentPaidPerMonth / fromFullPerMonth));
}

function formatCents(cents: number): string {
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

/// Intro-offer step prices for the plan matched to the user's Claude/Codex
/// tier: the discounted intro rate and the list rate it reverts to. Returns
/// null for plans without a fixed price (free / team / enterprise) or when
/// the offer is off, so the promo panel can simply not render.
export function getIntroStepPricing(
  planId: UpgradePlanId,
  billingPeriod: BillingPeriod,
  introOffer: IntroOffer | null | undefined
): { introLabel: string; intro: string; after: string } | null {
  if (planId !== "pro" && planId !== "max5x" && planId !== "max20x") return null;
  const pct = introPercentOff(introOffer);
  if (pct <= 0 || !introOffer) return null;
  const prices = PLAN_PRICES[planId][billingPeriod];
  return {
    introLabel: `First ${introOffer.durationMonths} months`,
    intro: discountedPriceLabel(prices.fullCents, pct),
    after: prices.full,
  };
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
  const monthly = PLAN_PRICES[planId][billingPeriod].fullCents / 100;
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
  introOffer: IntroOffer | null = null
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

    // Compute purchase info for the active plan card when data is available.
    const activePurchaseInfo = ((): UpgradePlanPurchaseInfo | undefined => {
      if (!activeHeadroomPlanId || subscriptionAmountCents == null) {
        return undefined;
      }
      const purchasePeriod = (subscriptionBillingPeriod === "annual" || subscriptionBillingPeriod === "monthly")
        ? subscriptionBillingPeriod
        : billingPeriod;
      const fullCents = PLAN_PRICES[activeHeadroomPlanId][purchasePeriod].fullCents;

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

      // If the discount won't apply at renewal, show full price for the renewal.
      const renewalCentsPerMonth = discountAppliesAtRenewal ? paidCentsPerMonth : fullCents;
      const discountPct = discountAppliesAtRenewal && fullCents > 0
        ? Math.round((1 - paidCentsPerMonth / fullCents) * 100)
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
      const prices = PLAN_PRICES[id][billingPeriod];
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
        ...(id === activeHeadroomPlanId && activePurchaseInfo ? { purchaseInfo: activePurchaseInfo } : {}),
        // Intro cards spell out the reversion so "$10/mo billed annually"
        // can't be misread as the full-year rate.
        billingLines:
          showDiscount && introPct > 0 && introOffer
            ? [
                `per month for your first ${introOffer.durationMonths} months`,
                `then ${prices.full}/mo · ${billingLabel}`,
              ]
            : ["USD / month", billingLabel],
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
        return {
          ...plan,
          ctaLabel: `Stay on ${plan.name} plan`,
          ctaVariant: "secondary",
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
