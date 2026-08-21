/**
 * The three a promotion can be in, as the check constraint on the column
 * writes them. Its own file because a screen may only export components —
 * a constant beside one turns off fast refresh for the whole file.
 */
export const PROMOTION_STATUS = ["draft", "active", "inactive"] as const

export type PromotionStatus = (typeof PROMOTION_STATUS)[number]
