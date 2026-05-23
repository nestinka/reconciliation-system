"use client";

import { useState } from "react";
import { ShieldCheck, ShieldX, AlertTriangle } from "lucide-react";
import { Button } from "@/components/ui/button";
import { Textarea } from "@/components/ui/textarea";
import { canApprove } from "@/lib/case/approval";
import type { Case, User } from "@/lib/domain/types";

interface ApprovalBarProps {
  case: Case;
  currentUser: User;
  onApprove: () => void;
  onReject: (reason: string) => void;
  pending?: boolean;
}

export function ApprovalBar({
  case: c,
  currentUser,
  onApprove,
  onReject,
  pending = false,
}: ApprovalBarProps) {
  const [rejectReason, setRejectReason] = useState("");
  const [showRejectForm, setShowRejectForm] = useState(false);

  if (c.status !== "pending_approval") return null;

  const decision = canApprove(c, currentUser);

  // Find the approval_requested event to show context
  const approvalRequest = [...c.events]
    .reverse()
    .find((e) => e.kind === "approval_requested");

  const resolution =
    approvalRequest?.kind === "approval_requested"
      ? approvalRequest.payload.resolution
      : null;

  // Detect whether this user is the maker (regardless of role) — for UI messaging
  const isMaker =
    approvalRequest != null && approvalRequest.actorId === currentUser.id;

  // Human-readable reason for approve being disabled.
  // Maker check takes precedence in messaging (four-eyes is the most important
  // rule to surface), even if the user also lacks the role.
  function resolveDisabledReason(): string | undefined {
    if (decision.allowed) return undefined;
    if (isMaker) {
      return "You proposed this change — a different approver must review it (four-eyes principle).";
    }
    if (decision.reason === "User does not have approver or admin role.") {
      return "Only approvers or admins can approve. Your role does not permit this action.";
    }
    return decision.reason;
  }

  const approveDisabledReason = resolveDisabledReason();

  const approveButtonId = `approve-btn-${c.id}`;
  const approveDescId = `approve-desc-${c.id}`;

  function handleReject() {
    if (!rejectReason.trim()) return;
    onReject(rejectReason.trim());
    setRejectReason("");
    setShowRejectForm(false);
  }

  return (
    <section
      aria-label="Four-eyes approval"
      className="rounded-lg border border-warning/30 bg-warning/5 p-4 flex flex-col gap-3"
    >
      {/* Header row */}
      <div className="flex items-center gap-2">
        <ShieldCheck className="size-4 text-warning shrink-0" aria-hidden />
        <span className="text-sm font-semibold text-foreground">
          Pending four-eyes approval
        </span>
        {resolution && (
          <span className="ml-auto text-xs text-muted-foreground font-mono bg-muted/50 px-2 py-0.5 rounded">
            {resolution.replace("_", " ")}
          </span>
        )}
      </div>

      {/* Approve button + reason */}
      <div className="flex flex-col gap-2">
        <div className="flex items-start gap-3 flex-wrap">
          <Button
            id={approveButtonId}
            size="sm"
            variant="default"
            disabled={!decision.allowed || pending}
            onClick={onApprove}
            aria-describedby={approveDisabledReason ? approveDescId : undefined}
            className="shrink-0"
          >
            <ShieldCheck className="size-3.5" aria-hidden />
            Approve
          </Button>

          {!showRejectForm && (
            <Button
              size="sm"
              variant="destructive"
              disabled={!decision.allowed || pending}
              onClick={() => setShowRejectForm(true)}
              className="shrink-0"
            >
              <ShieldX className="size-3.5" aria-hidden />
              Reject
            </Button>
          )}
        </div>

        {/* Disabled reason — visible helper text associated with the button */}
        {approveDisabledReason && (
          <p
            id={approveDescId}
            role="note"
            className="flex items-start gap-1.5 text-xs text-muted-foreground"
          >
            <AlertTriangle
              className="size-3.5 mt-0.5 text-warning shrink-0"
              aria-hidden
            />
            {approveDisabledReason}
          </p>
        )}
      </div>

      {/* Reject inline form */}
      {showRejectForm && (
        <div className="flex flex-col gap-2 border-t border-border pt-3">
          <label
            htmlFor={`reject-reason-${c.id}`}
            className="text-xs font-medium text-foreground"
          >
            Rejection reason
            <span className="text-danger ml-1" aria-hidden>
              *
            </span>
          </label>
          <Textarea
            id={`reject-reason-${c.id}`}
            placeholder="Describe why this proposal is rejected…"
            rows={2}
            value={rejectReason}
            onChange={(e) => setRejectReason(e.target.value)}
            className="text-sm"
            aria-required="true"
          />
          <div className="flex gap-2">
            <Button
              size="sm"
              variant="destructive"
              disabled={!rejectReason.trim() || pending}
              onClick={handleReject}
            >
              <ShieldX className="size-3.5" aria-hidden />
              Confirm rejection
            </Button>
            <Button
              size="sm"
              variant="ghost"
              onClick={() => {
                setShowRejectForm(false);
                setRejectReason("");
              }}
            >
              Cancel
            </Button>
          </div>
        </div>
      )}
    </section>
  );
}
