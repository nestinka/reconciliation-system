/**
 * Four-eyes approval logic — pure functions, never mutate inputs.
 *
 * Rules:
 *  - Only a user with role "approver" or "admin" can approve.
 *  - The user who requested approval (maker) cannot approve their own proposal.
 *  - Approval is only possible when the case status is "pending_approval".
 */
import type { Case, User } from "@/lib/domain/types";

export type CanApproveResult = { allowed: true } | { allowed: false; reason: string };

export function canApprove(c: Case, user: User): CanApproveResult {
  if (c.status !== "pending_approval") {
    return { allowed: false, reason: "Case is not pending approval." };
  }

  if (user.role !== "approver" && user.role !== "admin") {
    return { allowed: false, reason: "User does not have approver or admin role." };
  }

  // Find the most recent approval_requested event to identify the maker.
  const lastRequest = [...c.events]
    .reverse()
    .find((e) => e.kind === "approval_requested");

  if (lastRequest && lastRequest.actorId === user.id) {
    return { allowed: false, reason: "Maker cannot approve their own proposal (four-eyes principle)." };
  }

  return { allowed: true };
}

export function requestApproval(
  c: Case,
  maker: User,
  resolution: "write_off" | "manual_match"
): Case {
  const event = {
    id: crypto.randomUUID(),
    kind: "approval_requested" as const,
    actorId: maker.id,
    at: new Date().toISOString(),
    payload: { resolution },
  };

  return {
    ...c,
    status: "pending_approval",
    events: [...c.events, event],
  };
}

export function approve(c: Case, checker: User): Case {
  const event = {
    id: crypto.randomUUID(),
    kind: "approved" as const,
    actorId: checker.id,
    at: new Date().toISOString(),
    payload: {},
  };

  return {
    ...c,
    status: "resolved",
    events: [...c.events, event],
  };
}

export function reject(c: Case, checker: User, reason: string): Case {
  const event = {
    id: crypto.randomUUID(),
    kind: "rejected" as const,
    actorId: checker.id,
    at: new Date().toISOString(),
    payload: { reason },
  };

  return {
    ...c,
    status: "investigating",
    events: [...c.events, event],
  };
}
