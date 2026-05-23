"use client";

import { Suspense, useMemo, useState } from "react";
import { useRouter } from "next/navigation";
import { useQueryState } from "nuqs";
import { toast } from "sonner";

import { PageHeader } from "@/components/app/page-header";
import { DataTable } from "@/components/app/data-table";
import { buildBreakColumns } from "@/components/app/break-columns";
import { EmptyState } from "@/components/app/empty-state";
import {
  Select,
  SelectContent,
  SelectItem,
  SelectTrigger,
  SelectValue,
} from "@/components/ui/select";
import { Button } from "@/components/ui/button";
import { Card, CardContent } from "@/components/ui/card";
import { Skeleton } from "@/components/ui/skeleton";
import { useBreaks, useAssignBreak } from "@/lib/hooks/use-breaks";
import { useUsers } from "@/lib/hooks/use-tenants";
import type { BreakQuery } from "@/lib/api/client";
import type { Break, BreakStatus, BreakType, AgeingBucket } from "@/lib/domain/types";

// ---------------------------------------------------------------------------
// Filter option constants
// ---------------------------------------------------------------------------

const TYPE_OPTIONS: { value: string; label: string }[] = [
  { value: "all", label: "All types" },
  { value: "unmatched", label: "Unmatched" },
  { value: "partial", label: "Partial" },
  { value: "duplicate", label: "Duplicate" },
  { value: "break", label: "Break" },
];

const STATUS_OPTIONS: { value: string; label: string }[] = [
  { value: "all", label: "All statuses" },
  { value: "open", label: "Open" },
  { value: "investigating", label: "Investigating" },
  { value: "pending_approval", label: "Pending approval" },
  { value: "resolved", label: "Resolved" },
  { value: "written_off", label: "Written off" },
];

const AGEING_OPTIONS: { value: string; label: string }[] = [
  { value: "all", label: "All ageing" },
  { value: "0-1d", label: "0–1 day" },
  { value: "2-7d", label: "2–7 days" },
  { value: "8-30d", label: "8–30 days" },
  { value: "30d+", label: "30+ days" },
];

// ---------------------------------------------------------------------------
// Inner component — uses nuqs hooks (requires Suspense boundary in Next.js)
// ---------------------------------------------------------------------------

function ExceptionsPageInner() {
  const router = useRouter();

  // URL-persisted filters via nuqs
  const [type, setType] = useQueryState("type", { defaultValue: "all" });
  const [status, setStatus] = useQueryState("status", { defaultValue: "all" });
  const [ageing, setAgeing] = useQueryState("ageing", { defaultValue: "all" });
  const [assignee, setAssignee] = useQueryState("assignee", {
    defaultValue: "all",
  });

  // Users for the assignee filter and bulk-assign toolbar
  const { data: users = [] } = useUsers();
  const usersById = useMemo(
    () => Object.fromEntries(users.map((u) => [u.id, u])),
    [users]
  );

  // Build BreakQuery from URL filters (exclude "unassigned" — handled client-side)
  const query = useMemo<BreakQuery>(() => {
    const q: BreakQuery = {};
    if (type && type !== "all") q.type = type as BreakType;
    if (status && status !== "all") q.status = status as BreakStatus;
    if (ageing && ageing !== "all") q.ageingBucket = ageing as AgeingBucket;
    if (assignee && assignee !== "all" && assignee !== "unassigned")
      q.assigneeId = assignee;
    return q;
  }, [type, status, ageing, assignee]);

  const { data: breaks, isLoading, isError, refetch } = useBreaks(query);
  const assignMutation = useAssignBreak();

  // Client-side filter for "unassigned"
  const filteredBreaks = useMemo<Break[]>(() => {
    if (!breaks) return [];
    if (assignee === "unassigned") return breaks.filter((b) => !b.assigneeId);
    return breaks;
  }, [breaks, assignee]);

  // Build columns
  const columns = useMemo(
    () => buildBreakColumns({ currency: "GBP", usersById }),
    [usersById]
  );

  // Selection state
  const [selectedIds, setSelectedIds] = useState<string[]>([]);
  const [pendingUserId, setPendingUserId] = useState<string>("");

  // Assignee options for filter and toolbar
  const assigneeFilterOptions = useMemo(
    () => [
      { value: "all", label: "All assignees" },
      { value: "unassigned", label: "Unassigned" },
      ...users.map((u) => ({ value: u.id, label: u.name })),
    ],
    [users]
  );

  async function handleBulkAssign() {
    if (!pendingUserId || selectedIds.length === 0) return;
    try {
      await Promise.all(
        selectedIds.map((breakId) =>
          assignMutation.mutateAsync({ breakId, userId: pendingUserId })
        )
      );
      const userName = usersById[pendingUserId]?.name ?? pendingUserId;
      toast.success(`Assigned ${selectedIds.length} break(s) to ${userName}`);
      setSelectedIds([]);
      setPendingUserId("");
    } catch {
      toast.error("Failed to assign some breaks. Please try again.");
    }
  }

  return (
    <div className="flex flex-col gap-4">
      <PageHeader
        title="Exceptions"
        description="Open breaks across all reconciliation runs."
      />

      {/* Filter bar */}
      <div className="flex items-center gap-3 flex-wrap">
        <Select
          value={type ?? "all"}
          onValueChange={(val) => setType(val === "all" ? null : val)}
        >
          <SelectTrigger className="w-40" aria-label="Filter by type">
            <SelectValue placeholder="All types" />
          </SelectTrigger>
          <SelectContent>
            {TYPE_OPTIONS.map((opt) => (
              <SelectItem key={opt.value} value={opt.value}>
                {opt.label}
              </SelectItem>
            ))}
          </SelectContent>
        </Select>

        <Select
          value={status ?? "all"}
          onValueChange={(val) => setStatus(val === "all" ? null : val)}
        >
          <SelectTrigger className="w-44" aria-label="Filter by status">
            <SelectValue placeholder="All statuses" />
          </SelectTrigger>
          <SelectContent>
            {STATUS_OPTIONS.map((opt) => (
              <SelectItem key={opt.value} value={opt.value}>
                {opt.label}
              </SelectItem>
            ))}
          </SelectContent>
        </Select>

        <Select
          value={ageing ?? "all"}
          onValueChange={(val) => setAgeing(val === "all" ? null : val)}
        >
          <SelectTrigger className="w-40" aria-label="Filter by ageing">
            <SelectValue placeholder="All ageing" />
          </SelectTrigger>
          <SelectContent>
            {AGEING_OPTIONS.map((opt) => (
              <SelectItem key={opt.value} value={opt.value}>
                {opt.label}
              </SelectItem>
            ))}
          </SelectContent>
        </Select>

        <Select
          value={assignee ?? "all"}
          onValueChange={(val) => setAssignee(val === "all" ? null : val)}
        >
          <SelectTrigger className="w-44" aria-label="Filter by assignee">
            <SelectValue placeholder="All assignees" />
          </SelectTrigger>
          <SelectContent>
            {assigneeFilterOptions.map((opt) => (
              <SelectItem key={opt.value} value={opt.value}>
                {opt.label}
              </SelectItem>
            ))}
          </SelectContent>
        </Select>
      </div>

      {/* Error state */}
      {isError && (
        <div
          role="alert"
          className="rounded-lg border border-danger/30 bg-danger/5 px-4 py-3 text-sm text-danger flex items-center justify-between gap-4"
        >
          <span>Failed to load exceptions.</span>
          <Button size="sm" variant="outline" onClick={() => refetch()}>
            Retry
          </Button>
        </div>
      )}

      {/* Bulk-assign toolbar */}
      {selectedIds.length > 0 && (
        <div className="flex items-center gap-3 rounded-lg border border-border bg-muted/40 px-4 py-2.5 flex-wrap">
          <span className="text-sm font-medium tabular-nums">
            {selectedIds.length} selected
          </span>
          <div className="flex-1" />
          <Select
            value={pendingUserId || "__none__"}
            onValueChange={(val) =>
              setPendingUserId(val == null || val === "__none__" ? "" : val)
            }
          >
            <SelectTrigger
              className="w-44"
              aria-label="Choose assignee for bulk assign"
            >
              <SelectValue placeholder="Choose assignee…" />
            </SelectTrigger>
            <SelectContent>
              <SelectItem value="__none__" disabled>
                Choose assignee…
              </SelectItem>
              {users.map((u) => (
                <SelectItem key={u.id} value={u.id}>
                  {u.name}
                </SelectItem>
              ))}
            </SelectContent>
          </Select>
          <Button
            size="sm"
            disabled={!pendingUserId || assignMutation.isPending}
            onClick={handleBulkAssign}
          >
            {assignMutation.isPending ? "Assigning…" : "Assign"}
          </Button>
          <Button
            size="sm"
            variant="outline"
            onClick={() => {
              setSelectedIds([]);
              setPendingUserId("");
            }}
          >
            Clear
          </Button>
        </div>
      )}

      {/* Table */}
      {!isError && (
        <Card size="sm">
          <CardContent className="px-0 py-0">
            <DataTable
              columns={columns}
              rows={filteredBreaks}
              getRowId={(b) => b.id}
              isLoading={isLoading}
              skeletonRows={8}
              selectable
              selectedIds={selectedIds}
              onSelectionChange={setSelectedIds}
              getRowLabel={(b) => `break ${b.id}`}
              onRowClick={(b) => router.push(`/cases/${b.caseId}`)}
              emptyState={
                <EmptyState
                  title="No exceptions found"
                  description="No breaks match the selected filters."
                />
              }
            />
          </CardContent>
        </Card>
      )}
    </div>
  );
}

// ---------------------------------------------------------------------------
// Fallback skeleton shown while nuqs reads the URL params
// ---------------------------------------------------------------------------

function ExceptionsPageSkeleton() {
  return (
    <div className="flex flex-col gap-4">
      <div className="flex flex-col gap-0.5 pb-4 border-b border-border">
        <Skeleton className="h-6 w-36" />
        <Skeleton className="h-4 w-72 mt-1" />
      </div>
      <div className="flex items-center gap-3">
        <Skeleton className="h-9 w-40" />
        <Skeleton className="h-9 w-44" />
        <Skeleton className="h-9 w-40" />
        <Skeleton className="h-9 w-44" />
      </div>
      <Card size="sm">
        <CardContent className="px-0 py-0">
          <div className="flex flex-col gap-0">
            {Array.from({ length: 8 }).map((_, i) => (
              <div
                key={i}
                className="flex gap-4 px-4 py-3 border-b border-border last:border-0"
              >
                <Skeleton className="h-4 w-8" />
                <Skeleton className="h-4 w-24" />
                <Skeleton className="h-4 w-16" />
                <Skeleton className="h-4 w-20" />
                <Skeleton className="h-4 w-20 ml-auto" />
              </div>
            ))}
          </div>
        </CardContent>
      </Card>
    </div>
  );
}

// ---------------------------------------------------------------------------
// Page export — wraps the inner component in Suspense for useSearchParams
// ---------------------------------------------------------------------------

export default function ExceptionsPage() {
  return (
    <Suspense fallback={<ExceptionsPageSkeleton />}>
      <ExceptionsPageInner />
    </Suspense>
  );
}
