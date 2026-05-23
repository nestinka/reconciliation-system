"use client";

import { useState, useMemo } from "react";
import { useParams, useRouter } from "next/navigation";
import { AlertCircle, ArrowLeft, Shuffle, FileX } from "lucide-react";
import { toast } from "sonner";

import { PageHeader } from "@/components/app/page-header";
import { StatusPill } from "@/components/app/status-pill";
import { CaseTimeline } from "@/components/app/case-timeline";
import { ApprovalBar } from "@/components/app/approval-bar";
import { Card, CardContent, CardHeader, CardTitle } from "@/components/ui/card";
import { Button } from "@/components/ui/button";
import { Skeleton } from "@/components/ui/skeleton";
import { Separator } from "@/components/ui/separator";
import {
  Select,
  SelectContent,
  SelectItem,
  SelectTrigger,
  SelectValue,
} from "@/components/ui/select";
import { Textarea } from "@/components/ui/textarea";
import { useCase, useAppendCaseEvent } from "@/lib/hooks/use-case";
import { useUsers } from "@/lib/hooks/use-tenants";
import { useCurrentUserId } from "@/lib/providers/current-user-provider";
import { formatMoney } from "@/lib/domain/money";
import { formatDate, formatDateTime } from "@/lib/domain/date";
import type { User } from "@/lib/domain/types";

type CaseDetail = NonNullable<ReturnType<typeof useCase>["data"]>;

// ---------------------------------------------------------------------------
// Loading skeleton
// ---------------------------------------------------------------------------
function CaseDetailSkeleton() {
  return (
    <div className="flex flex-col gap-6" aria-busy="true" aria-label="Loading case details">
      <div className="flex flex-col gap-2 pb-4 border-b border-border">
        <Skeleton className="h-6 w-72" />
        <Skeleton className="h-4 w-48" />
      </div>
      <div className="grid grid-cols-1 gap-4 sm:grid-cols-2">
        <Skeleton className="h-32 w-full rounded-xl" />
        <Skeleton className="h-32 w-full rounded-xl" />
      </div>
      <Skeleton className="h-24 w-full rounded-xl" />
      <Skeleton className="h-48 w-full rounded-xl" />
    </div>
  );
}

// ---------------------------------------------------------------------------
// Page
// ---------------------------------------------------------------------------
export default function CaseDetailPage() {
  const { caseId } = useParams<{ caseId: string }>();
  const router = useRouter();

  const { data, isLoading, isError, refetch } = useCase(caseId);
  const { data: users = [] } = useUsers();
  const { currentUserId } = useCurrentUserId();

  const usersById = useMemo<Record<string, User>>(
    () => Object.fromEntries(users.map((u) => [u.id, u])),
    [users]
  );

  const currentUser = usersById[currentUserId];
  const append = useAppendCaseEvent(caseId);

  if (isLoading) return <CaseDetailSkeleton />;

  if (isError || !data) {
    return (
      <div
        role="alert"
        className="flex flex-col items-center justify-center gap-4 py-16"
      >
        <AlertCircle className="size-10 text-danger" aria-hidden />
        <div className="text-center">
          <p className="text-sm font-medium text-foreground">
            {isError ? "Failed to load case" : "Case not found"}
          </p>
          <p className="text-xs text-muted-foreground mt-1">
            {isError
              ? "There was a problem loading the case details."
              : `Case "${caseId}" could not be found.`}
          </p>
        </div>
        {isError && (
          <Button size="sm" variant="outline" onClick={() => refetch()}>
            Retry
          </Button>
        )}
      </div>
    );
  }

  const { case: c, brk, suggestions, transactionsById } = data;

  return (
    <CaseDetailView
      c={c}
      brk={brk}
      suggestions={suggestions}
      transactionsById={transactionsById}
      usersById={usersById}
      currentUser={currentUser}
      onBack={() => router.push(`/runs/${brk.runId}`)}
      append={append}
    />
  );
}

// ---------------------------------------------------------------------------
// View (split out so it renders only when data is present)
// ---------------------------------------------------------------------------
function CaseDetailView({
  c,
  brk,
  suggestions,
  transactionsById,
  usersById,
  currentUser,
  onBack,
  append,
}: {
  c: CaseDetail["case"];
  brk: CaseDetail["brk"];
  suggestions: CaseDetail["suggestions"];
  transactionsById: CaseDetail["transactionsById"];
  usersById: Record<string, User>;
  currentUser: User | undefined;
  onBack: () => void;
  append: ReturnType<typeof useAppendCaseEvent>;
}) {
  const [commentText, setCommentText] = useState("");
  const [writeOffReason, setWriteOffReason] = useState("");
  const [showWriteOffForm, setShowWriteOffForm] = useState(false);
  // Guards an entire (possibly multi-step) action so buttons stay disabled
  // across the whole handler, not just per individual mutation.
  const [submitting, setSubmitting] = useState(false);

  const isMutating = append.isPending || submitting;
  const isResolved =
    c.status === "resolved" || c.status === "written_off";
  const isPendingApproval = c.status === "pending_approval";
  // Proposal actions are frozen once pending or resolved; comments/assignment still allowed
  const proposalActionsDisabled = isMutating || isPendingApproval || isResolved;
  const baseActionsDisabled = isMutating || isResolved;

  // ---------------------------------------------------------------------------
  // Action handlers
  // ---------------------------------------------------------------------------
  async function handleAssign(assigneeId: string) {
    if (!currentUser) return;
    try {
      await append.mutateAsync({
        kind: "assignment",
        actorId: currentUser.id,
        payload: { assigneeId },
      });
      const name = usersById[assigneeId]?.name ?? assigneeId;
      toast.success(`Assigned to ${name}`);
    } catch {
      toast.error("Failed to assign case");
    }
  }

  async function handleComment() {
    if (!currentUser || !commentText.trim()) return;
    try {
      await append.mutateAsync({
        kind: "comment",
        actorId: currentUser.id,
        payload: { text: commentText.trim() },
      });
      setCommentText("");
      toast.success("Comment added");
    } catch {
      toast.error("Failed to add comment");
    }
  }

  async function handleProposeManualMatch() {
    if (!currentUser) return;
    try {
      await append.mutateAsync({
        kind: "approval_requested",
        actorId: currentUser.id,
        payload: { resolution: "manual_match" },
      });
      toast.success("Proposed — awaiting four-eyes approval");
    } catch {
      toast.error("Failed to propose manual match");
    }
  }

  async function handleProposeWriteOff() {
    if (!currentUser || !writeOffReason.trim()) return;
    const reason = writeOffReason.trim();
    // `submitting` keeps the proposal buttons disabled across BOTH mutations so
    // the action bar can't flash re-enabled (and the proposal can't double-fire)
    // between the write_off_proposed and approval_requested events.
    setSubmitting(true);
    try {
      // First capture the reason in a write_off_proposed event
      await append.mutateAsync({
        kind: "write_off_proposed",
        actorId: currentUser.id,
        payload: { reason },
      });
      // Then request approval
      await append.mutateAsync({
        kind: "approval_requested",
        actorId: currentUser.id,
        payload: { resolution: "write_off" },
      });
      setWriteOffReason("");
      setShowWriteOffForm(false);
      toast.success("Write-off proposed — awaiting four-eyes approval");
    } catch {
      toast.error("Failed to propose write-off");
    } finally {
      setSubmitting(false);
    }
  }

  async function handleApprove() {
    if (!currentUser) return;
    try {
      await append.mutateAsync({
        kind: "approved",
        actorId: currentUser.id,
        payload: {},
      });
      toast.success("Resolution approved");
    } catch {
      toast.error("Failed to approve");
    }
  }

  async function handleReject(reason: string) {
    if (!currentUser) return;
    try {
      await append.mutateAsync({
        kind: "rejected",
        actorId: currentUser.id,
        payload: { reason },
      });
      toast.success("Proposal rejected — case returned to investigation");
    } catch {
      toast.error("Failed to reject");
    }
  }

  // Break's primary transaction(s)
  const breakTxns = brk.txnIds
    .map((id) => transactionsById[id])
    .filter(Boolean);
  const lastEvent = c.events.at(-1);

  return (
    <div className="flex flex-col gap-6">
      {/* ---- Header ---- */}
      <PageHeader title={`Investigate ${brk.id}`}>
        <StatusPill status={c.status} />
        <Button
          size="sm"
          variant="ghost"
          onClick={onBack}
          aria-label={`Back to run ${brk.runId}`}
        >
          <ArrowLeft className="size-3.5" aria-hidden />
          Run
        </Button>
      </PageHeader>

      {/* Breadcrumb meta */}
      <div className="flex flex-wrap items-center gap-3 -mt-2 text-xs text-muted-foreground">
        <span>
          Type:{" "}
          <span className="text-foreground font-medium capitalize">{brk.type}</span>
        </span>
        <span>
          Run:{" "}
          <button
            className="text-foreground font-medium underline underline-offset-2 hover:text-foreground/70"
            onClick={onBack}
          >
            {brk.runId}
          </button>
        </span>
        {brk.assigneeId && (
          <span>
            Assignee:{" "}
            <span className="text-foreground font-medium">
              {usersById[brk.assigneeId]?.name ?? brk.assigneeId}
            </span>
          </span>
        )}
      </div>

      {/* ---- Break context panel ---- */}
      <section aria-labelledby="break-context-heading">
        <h2
          id="break-context-heading"
          className="text-xs font-semibold text-muted-foreground uppercase tracking-wide mb-3"
        >
          Break Context
        </h2>
        <div className="grid grid-cols-1 gap-4 sm:grid-cols-2">
          {/* Break summary card */}
          <Card size="sm">
            <CardHeader>
              <CardTitle>Break Summary</CardTitle>
            </CardHeader>
            <CardContent>
              <dl className="grid grid-cols-2 gap-x-4 gap-y-2 text-xs">
                <dt className="text-muted-foreground">Status</dt>
                <dd>
                  <StatusPill status={brk.status} />
                </dd>
                <dt className="text-muted-foreground">Value</dt>
                <dd className="nums font-medium text-foreground">
                  {formatMoney(brk.valueMinor, brk.currency)}
                </dd>
                <dt className="text-muted-foreground">Ageing</dt>
                <dd className="text-foreground">
                  {brk.ageingDays}d ({brk.ageingBucket})
                </dd>
                <dt className="text-muted-foreground">Opened</dt>
                <dd className="text-foreground">{formatDate(brk.openedAt)}</dd>
              </dl>
            </CardContent>
          </Card>

          {/* Transactions involved */}
          <Card size="sm">
            <CardHeader>
              <CardTitle>Transactions Involved</CardTitle>
            </CardHeader>
            <CardContent>
              {breakTxns.length === 0 ? (
                <p className="text-xs text-muted-foreground italic">
                  No transaction details available.
                </p>
              ) : (
                <ul className="flex flex-col gap-3">
                  {breakTxns.map((txn) => (
                    <li key={txn.id} className="flex flex-col gap-0.5">
                      <div className="flex items-center gap-2">
                        <span className="font-mono text-xs font-medium text-foreground truncate">
                          {txn.externalRef}
                        </span>
                        <span
                          className={`text-[10px] font-medium px-1.5 py-0.5 rounded ${
                            txn.direction === "debit"
                              ? "bg-danger/10 text-danger"
                              : "bg-success/10 text-success"
                          }`}
                        >
                          {txn.direction}
                        </span>
                      </div>
                      <div className="flex flex-wrap gap-2 text-xs text-muted-foreground">
                        <span className="nums font-medium text-foreground">
                          {formatMoney(txn.amountMinor, txn.currency)}
                        </span>
                        <span>{formatDate(txn.valueDate)}</span>
                        {txn.counterparty && <span>{txn.counterparty}</span>}
                      </div>
                      <p className="text-xs text-muted-foreground truncate">
                        {txn.description}
                      </p>
                    </li>
                  ))}
                </ul>
              )}
            </CardContent>
          </Card>
        </div>
      </section>

      {/* ---- Match Suggestions ---- */}
      {suggestions.length > 0 && (
        <section aria-labelledby="suggestions-heading">
          <h2
            id="suggestions-heading"
            className="text-xs font-semibold text-muted-foreground uppercase tracking-wide mb-3"
          >
            Suggested Matches
          </h2>
          <div className="flex flex-col gap-3">
            {suggestions.map((sug) => {
              const txns = sug.txnIds
                .map((id) => transactionsById[id])
                .filter(Boolean);
              return (
                <Card key={sug.id} size="sm">
                  <CardContent className="flex flex-col gap-2">
                    <div className="flex items-center justify-between gap-2">
                      <div className="flex items-center gap-1.5 text-xs text-muted-foreground">
                        <Shuffle className="size-3.5 shrink-0" aria-hidden />
                        <span className="font-medium text-foreground">
                          {txns.map((t) => t.externalRef).join(" ↔ ")}
                        </span>
                      </div>
                      <span
                        className="text-xs font-semibold nums tabular-nums"
                        aria-label={`Match score ${Math.round(sug.score * 100)} percent`}
                      >
                        {Math.round(sug.score * 100)}%
                      </span>
                    </div>
                    <p className="text-xs text-muted-foreground">
                      {sug.rationale}
                    </p>
                    {txns.length > 0 && (
                      <ul className="flex flex-wrap gap-2">
                        {txns.map((txn) => (
                          <li
                            key={txn.id}
                            className="flex items-center gap-1.5 rounded bg-muted/40 px-2 py-1 text-xs"
                          >
                            <span className="font-mono">{txn.externalRef}</span>
                            <span className="text-muted-foreground">
                              {formatMoney(txn.amountMinor, txn.currency)}
                            </span>
                            <span className="text-muted-foreground">
                              {formatDate(txn.valueDate)}
                            </span>
                          </li>
                        ))}
                      </ul>
                    )}
                  </CardContent>
                </Card>
              );
            })}
          </div>
        </section>
      )}

      {/* ---- Four-eyes ApprovalBar ---- */}
      {isPendingApproval && currentUser && (
        <ApprovalBar
          case={c}
          currentUser={currentUser}
          onApprove={handleApprove}
          onReject={handleReject}
          pending={isMutating}
        />
      )}

      {/* ---- Action Bar ---- */}
      {!isResolved && (
        <section aria-labelledby="actions-heading">
          <h2
            id="actions-heading"
            className="text-xs font-semibold text-muted-foreground uppercase tracking-wide mb-3"
          >
            Actions
          </h2>
          <Card size="sm">
            <CardContent className="flex flex-col gap-4">
              {/* Assign */}
              <div className="flex flex-col gap-1.5">
                <label
                  htmlFor="assign-select"
                  className="text-xs font-medium text-foreground"
                >
                  Assign to
                </label>
                <Select
                  value={brk.assigneeId ?? ""}
                  onValueChange={(value) => { if (value) void handleAssign(value); }}
                  disabled={baseActionsDisabled}
                >
                  <SelectTrigger
                    id="assign-select"
                    size="sm"
                    className="w-full max-w-xs"
                    aria-label="Assign case to a team member"
                  >
                    <SelectValue placeholder="Select assignee…" />
                  </SelectTrigger>
                  <SelectContent>
                    {Object.values(usersById).map((u) => (
                      <SelectItem key={u.id} value={u.id}>
                        {u.name}
                        <span className="ml-1 text-muted-foreground text-[10px]">
                          ({u.role})
                        </span>
                      </SelectItem>
                    ))}
                  </SelectContent>
                </Select>
              </div>

              <Separator />

              {/* Comment */}
              <div className="flex flex-col gap-1.5">
                <label
                  htmlFor="comment-textarea"
                  className="text-xs font-medium text-foreground"
                >
                  Add a comment
                </label>
                <Textarea
                  id="comment-textarea"
                  placeholder="Notes, findings, or context…"
                  rows={3}
                  value={commentText}
                  onChange={(e) => setCommentText(e.target.value)}
                  disabled={baseActionsDisabled}
                  className="text-sm resize-none"
                />
                <Button
                  size="sm"
                  variant="outline"
                  disabled={baseActionsDisabled || !commentText.trim()}
                  onClick={handleComment}
                  className="w-fit"
                >
                  Add comment
                </Button>
              </div>

              <Separator />

              {/* Propose resolution */}
              <div className="flex flex-col gap-2">
                <p className="text-xs font-medium text-foreground">
                  Propose resolution
                </p>
                <div className="flex flex-wrap gap-2">
                  <Button
                    size="sm"
                    variant="secondary"
                    disabled={proposalActionsDisabled}
                    onClick={handleProposeManualMatch}
                  >
                    <Shuffle className="size-3.5" aria-hidden />
                    Propose manual match
                  </Button>
                  <Button
                    size="sm"
                    variant="secondary"
                    disabled={proposalActionsDisabled}
                    onClick={() => setShowWriteOffForm((v) => !v)}
                  >
                    <FileX className="size-3.5" aria-hidden />
                    Propose write-off
                  </Button>
                </div>

                {showWriteOffForm && (
                  <div className="flex flex-col gap-2 mt-1 pl-2 border-l-2 border-warning/40">
                    <label
                      htmlFor="writeoff-reason"
                      className="text-xs font-medium text-foreground"
                    >
                      Write-off reason
                      <span className="text-danger ml-1" aria-hidden>
                        *
                      </span>
                    </label>
                    <Textarea
                      id="writeoff-reason"
                      placeholder="Reason for the write-off…"
                      rows={2}
                      value={writeOffReason}
                      onChange={(e) => setWriteOffReason(e.target.value)}
                      disabled={proposalActionsDisabled}
                      className="text-sm resize-none"
                    />
                    <div className="flex gap-2">
                      <Button
                        size="sm"
                        variant="default"
                        disabled={proposalActionsDisabled || !writeOffReason.trim()}
                        onClick={handleProposeWriteOff}
                      >
                        Submit write-off proposal
                      </Button>
                      <Button
                        size="sm"
                        variant="ghost"
                        onClick={() => {
                          setShowWriteOffForm(false);
                          setWriteOffReason("");
                        }}
                      >
                        Cancel
                      </Button>
                    </div>
                  </div>
                )}
              </div>
            </CardContent>
          </Card>
        </section>
      )}

      {/* ---- Timeline ---- */}
      <section aria-labelledby="timeline-heading">
        <h2
          id="timeline-heading"
          className="text-xs font-semibold text-muted-foreground uppercase tracking-wide mb-3"
        >
          Activity Timeline
        </h2>
        <Card size="sm">
          <CardContent>
            <CaseTimeline events={c.events} usersById={usersById} />
          </CardContent>
        </Card>
      </section>

      {/* Last updated footer */}
      {lastEvent && (
        <p className="text-xs text-muted-foreground text-right">
          Last updated: {formatDateTime(lastEvent.at)}
        </p>
      )}
    </div>
  );
}
