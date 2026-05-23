"use client";

import { Suspense } from "react";
import { useRouter } from "next/navigation";
import { useQueryState } from "nuqs";
import { Search } from "lucide-react";

import { PageHeader } from "@/components/app/page-header";
import { DataTable } from "@/components/app/data-table";
import { buildRunColumns } from "@/components/app/run-columns";
import { EmptyState } from "@/components/app/empty-state";
import {
  Select,
  SelectContent,
  SelectItem,
  SelectTrigger,
  SelectValue,
} from "@/components/ui/select";
import { Input } from "@/components/ui/input";
import { Button } from "@/components/ui/button";
import { Card, CardContent } from "@/components/ui/card";
import { Skeleton } from "@/components/ui/skeleton";
import { useRuns } from "@/lib/hooks/use-runs";
import type { RunStatus } from "@/lib/domain/types";

// Status options for the filter Select
const STATUS_OPTIONS: { value: string; label: string }[] = [
  { value: "all", label: "All statuses" },
  { value: "running", label: "Running" },
  { value: "completed", label: "Completed" },
  { value: "failed", label: "Failed" },
];

// ---------------------------------------------------------------------------
// Inner component — uses nuqs hooks (requires Suspense boundary in Next.js)
// ---------------------------------------------------------------------------
function RunsPageInner() {
  const router = useRouter();

  // URL-persisted filters via nuqs
  const [status, setStatus] = useQueryState("status", {
    defaultValue: "all",
  });
  const [q, setQ] = useQueryState("q", { defaultValue: "" });

  // Only pass status to the API when it's not "all"
  const apiStatus =
    status && status !== "all" ? (status as RunStatus) : undefined;

  const { data: runs, isLoading, isError, refetch } = useRuns(
    apiStatus ? { status: apiStatus } : undefined
  );

  const runColumns = buildRunColumns("GBP");

  // Client-side name filter (case-insensitive substring)
  const filteredRuns = runs
    ? q.trim()
      ? runs.filter((r) =>
          r.name.toLowerCase().includes(q.trim().toLowerCase())
        )
      : runs
    : [];

  return (
    <div className="flex flex-col gap-4">
      <PageHeader
        title="Reconciliation runs"
        description="Browse and inspect reconciliation run history."
      />

      {/* Filter bar */}
      <div className="flex items-center gap-3 flex-wrap">
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

        <div className="relative flex-1 max-w-sm">
          <Search
            aria-hidden
            className="absolute left-2.5 top-1/2 -translate-y-1/2 size-3.5 text-muted-foreground pointer-events-none"
          />
          <Input
            className="pl-8"
            placeholder="Search by name…"
            value={q ?? ""}
            onChange={(e) => setQ(e.target.value || null)}
            aria-label="Search runs by name"
          />
        </div>
      </div>

      {/* Error state */}
      {isError && (
        <div
          role="alert"
          className="rounded-lg border border-danger/30 bg-danger/5 px-4 py-3 text-sm text-danger flex items-center justify-between gap-4"
        >
          <span>Failed to load runs.</span>
          <Button size="sm" variant="outline" onClick={() => refetch()}>
            Retry
          </Button>
        </div>
      )}

      {/* Table */}
      {!isError && (
        <Card size="sm">
          <CardContent className="px-0 py-0">
            <DataTable
              columns={runColumns}
              rows={filteredRuns}
              getRowId={(r) => r.id}
              isLoading={isLoading}
              skeletonRows={8}
              onRowClick={(r) => router.push(`/runs/${r.id}`)}
              emptyState={
                <EmptyState
                  title="No runs found"
                  description={
                    q
                      ? "No runs match your search. Try a different name."
                      : "No reconciliation runs match the selected filter."
                  }
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
function RunsPageSkeleton() {
  return (
    <div className="flex flex-col gap-4">
      <div className="flex flex-col gap-0.5 pb-4 border-b border-border">
        <Skeleton className="h-6 w-48" />
        <Skeleton className="h-4 w-72 mt-1" />
      </div>
      <div className="flex items-center gap-3">
        <Skeleton className="h-9 w-44" />
        <Skeleton className="h-9 w-48" />
      </div>
      <Card size="sm">
        <CardContent className="px-0 py-0">
          <div className="flex flex-col gap-0">
            {Array.from({ length: 8 }).map((_, i) => (
              <div key={i} className="flex gap-4 px-4 py-3 border-b border-border last:border-0">
                <Skeleton className="h-4 w-48" />
                <Skeleton className="h-4 w-20" />
                <Skeleton className="h-4 w-16 ml-auto" />
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
export default function RunsPage() {
  return (
    <Suspense fallback={<RunsPageSkeleton />}>
      <RunsPageInner />
    </Suspense>
  );
}
