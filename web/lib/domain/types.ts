import { z } from "zod";

// ---------------------------------------------------------------------------
// Tenant
// ---------------------------------------------------------------------------

export const tenantSchema = z.object({
  id: z.string(),
  name: z.string(),
  slug: z.string(),
});
export type Tenant = z.infer<typeof tenantSchema>;

// ---------------------------------------------------------------------------
// Source
// ---------------------------------------------------------------------------

export const sourceKindSchema = z.enum(["bank", "ledger", "cross_system"]);
export type SourceKind = z.infer<typeof sourceKindSchema>;

export const formatDialectSchema = z.enum(["generic", "subfielded"]);
export type FormatDialect = z.infer<typeof formatDialectSchema>;

export const sourceSchema = z.object({
  id: z.string(),
  tenantId: z.string(),
  kind: sourceKindSchema,
  name: z.string(),
  currency: z.string(),
  formatDialect: formatDialectSchema.nullable(),
});
export type Source = z.infer<typeof sourceSchema>;

// ---------------------------------------------------------------------------
// CanonicalTransaction (immutable)
// ---------------------------------------------------------------------------

export const directionSchema = z.enum(["debit", "credit"]);
export type Direction = z.infer<typeof directionSchema>;

export const canonicalTransactionSchema = z.object({
  id: z.string(),
  tenantId: z.string(),
  sourceId: z.string(),
  externalRef: z.string(),
  valueDate: z.string(),
  postedAt: z.string(),
  amountMinor: z.number().int(),
  currency: z.string(),
  direction: directionSchema,
  counterparty: z.string().optional(),
  description: z.string(),
  counterpartyBic: z.string().nullable().optional(),
  counterpartyAccount: z.string().nullable().optional(),
});
export type CanonicalTransaction = z.infer<typeof canonicalTransactionSchema>;

// ---------------------------------------------------------------------------
// ReconciliationRun
// ---------------------------------------------------------------------------

export const runStatusSchema = z.enum(["running", "completed", "failed"]);
export type RunStatus = z.infer<typeof runStatusSchema>;

export const runStatsSchema = z.object({
  matched: z.number().int(),
  unmatched: z.number().int(),
  partial: z.number().int(),
  duplicate: z.number().int(),
  breakCount: z.number().int(),
  matchRatePct: z.number(),
  valueAtRiskMinor: z.number().int(),
});
export type RunStats = z.infer<typeof runStatsSchema>;

export const reconciliationRunSchema = z.object({
  id: z.string(),
  tenantId: z.string(),
  name: z.string(),
  sourceAId: z.string(),
  sourceBId: z.string(),
  status: runStatusSchema,
  startedAt: z.string(),
  completedAt: z.string().optional(),
  configVersion: z.string(),
  stats: runStatsSchema,
});
export type ReconciliationRun = z.infer<typeof reconciliationRunSchema>;

// ---------------------------------------------------------------------------
// MatchDecision (immutable)
// ---------------------------------------------------------------------------

export const matchTypeSchema = z.enum(["matched", "partial", "duplicate"]);
export type MatchType = z.infer<typeof matchTypeSchema>;

export const matchDecisionSchema = z.object({
  id: z.string(),
  runId: z.string(),
  type: matchTypeSchema,
  txnIds: z.array(z.string()),
  score: z.number(),
  configVersion: z.string(),
});
export type MatchDecision = z.infer<typeof matchDecisionSchema>;

// ---------------------------------------------------------------------------
// Break
// ---------------------------------------------------------------------------

export const breakTypeSchema = z.enum([
  "unmatched",
  "partial",
  "duplicate",
  "break",
]);
export type BreakType = z.infer<typeof breakTypeSchema>;

export const breakStatusSchema = z.enum([
  "open",
  "investigating",
  "pending_approval",
  "resolved",
  "written_off",
]);
export type BreakStatus = z.infer<typeof breakStatusSchema>;

export const ageingBucketSchema = z.enum(["0-1d", "2-7d", "8-30d", "30d+"]);
export type AgeingBucket = z.infer<typeof ageingBucketSchema>;

export const breakSchema = z.object({
  id: z.string(),
  tenantId: z.string(),
  runId: z.string(),
  caseId: z.string(),
  type: breakTypeSchema,
  status: breakStatusSchema,
  ageingDays: z.number().int(),
  ageingBucket: ageingBucketSchema,
  valueMinor: z.number().int(),
  currency: z.string(),
  assigneeId: z.string().optional(),
  txnIds: z.array(z.string()),
  openedAt: z.string(),
});
export type Break = z.infer<typeof breakSchema>;

// ---------------------------------------------------------------------------
// User
// ---------------------------------------------------------------------------

export const userRoleSchema = z.enum(["operator", "approver", "admin"]);
export type UserRole = z.infer<typeof userRoleSchema>;

export const userSchema = z.object({
  id: z.string(),
  name: z.string(),
  email: z.string().optional(),
  disabled: z.boolean().optional(),
  role: userRoleSchema,
});
export type User = z.infer<typeof userSchema>;

// ---------------------------------------------------------------------------
// Membership
// ---------------------------------------------------------------------------

export const membershipSchema = z.object({
  tenantId: z.string(),
  tenantName: z.string(),
  role: userRoleSchema,
});
export type Membership = z.infer<typeof membershipSchema>;

// ---------------------------------------------------------------------------
// CaseEvent — discriminated union on `kind`
// ---------------------------------------------------------------------------

export const caseEventKindSchema = z.enum([
  "comment",
  "assignment",
  "manual_match_proposed",
  "write_off_proposed",
  "approval_requested",
  "approved",
  "rejected",
]);
export type CaseEventKind = z.infer<typeof caseEventKindSchema>;

const baseEventSchema = z.object({
  id: z.string(),
  actorId: z.string(),
  at: z.string(),
});

export const commentEventSchema = baseEventSchema.extend({
  kind: z.literal("comment"),
  payload: z.object({ text: z.string() }),
});

export const assignmentEventSchema = baseEventSchema.extend({
  kind: z.literal("assignment"),
  payload: z.object({ assigneeId: z.string() }),
});

export const manualMatchProposedEventSchema = baseEventSchema.extend({
  kind: z.literal("manual_match_proposed"),
  payload: z.object({ txnIds: z.array(z.string()) }),
});

export const writeOffProposedEventSchema = baseEventSchema.extend({
  kind: z.literal("write_off_proposed"),
  payload: z.object({ reason: z.string() }),
});

export const approvalRequestedEventSchema = baseEventSchema.extend({
  kind: z.literal("approval_requested"),
  payload: z.object({
    resolution: z.enum(["write_off", "manual_match"]),
  }),
});

export const approvedEventSchema = baseEventSchema.extend({
  kind: z.literal("approved"),
  payload: z.object({}),
});

export const rejectedEventSchema = baseEventSchema.extend({
  kind: z.literal("rejected"),
  payload: z.object({ reason: z.string() }),
});

export const caseEventSchema = z.discriminatedUnion("kind", [
  commentEventSchema,
  assignmentEventSchema,
  manualMatchProposedEventSchema,
  writeOffProposedEventSchema,
  approvalRequestedEventSchema,
  approvedEventSchema,
  rejectedEventSchema,
]);
export type CaseEvent = z.infer<typeof caseEventSchema>;

// ---------------------------------------------------------------------------
// Case
// ---------------------------------------------------------------------------

export const caseSchema = z.object({
  id: z.string(),
  breakId: z.string(),
  assigneeId: z.string().optional(),
  status: breakStatusSchema,
  events: z.array(caseEventSchema),
});
export type Case = z.infer<typeof caseSchema>;
