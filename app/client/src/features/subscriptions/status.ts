/**
 * The five a contract's status column permits, as the check constraint on it
 * writes them — `subscription::STATUSES` in the crate is the same list, and
 * the API refuses anything else.
 *
 * Its own file because a screen may only export components.
 */
export const SUBSCRIPTION_STATUS = [
  "active",
  "past_due",
  "cancelled",
  "expired",
  "paused",
] as const

export type SubscriptionStatus = (typeof SUBSCRIPTION_STATUS)[number]
