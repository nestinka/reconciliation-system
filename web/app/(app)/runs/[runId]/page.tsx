"use client";

import { useParams, useRouter } from "next/navigation";
import { AlertCircle } from "lucide-react";

import { PageHeader } from "@/components/app/page-header";
import { KpiCard } from "@/components/app/kpi-card";
import { StatusPill } from "@/components/app/status-pill";
import { DataTable } from "@/components/app/data-table";
import { EmptyState } from "@/components/app/empty-state";
import { Tabs, TabsList, TabsTrigger, TabsContent } from "@/components/ui/tabs";
import { Card, CardContent } from "@/components/ui/card";
import { Skeleton } from "@/components/ui/skeleton";
import { Button } from "@/components/ui/button";
import { useRun } from "@/lib/hooks/use-runs";
import { formatMoney } from "@/lib/domain/money";
import { formatDate } from "@/lib/domain/date";
import type { Column } from "@/components/app/data-table";
import type {
  MatchDecision,
  Break,
  CanonicalTransaction,
} from "@/lib/domain/types";

// ---------------------------------------------------------------------------
// Helper: derive display currency from transactionsById
// ---------------------------------------------------------------------------
function deriveCurrency(
  transactionsById: Record<string, CanonicalTransaction>
): string {
  const first = Object.values(transactionsById)[0];
  return first?.currency ?? "GBP";
}

// ---------------------------------------------------------------------------
// MatchDecision columns
// ---------------------------------------------------------------------------
function buildDecisionColumns(
  transactionsById: Record<string, CanonicalTransaction>,
  currency: string
): Column<MatchDecision>[] {
  return [
    {
      id: "references",
      header: "References",
      cell: (d) => {
        const refs = d.txnIds
          .map((id) => transactionsById[id]?.externalRef ?? id)
          .join(" ↔ ");
        return (
          <span className="font-mono text-xs text-foreground truncate max-w-64 block">
            {refs}
          </span>
        );
      },
    },
    {
      id: "amount",
      header: "Amount",
      align: "right",
      cell: (d) => {
        const firstTxn = transactionsById[d.txnIds[0]];
        if (!firstTxn) return <span className="text-muted-foreground">—</span>;
        return (
          <span className="nums">
            {formatMoney(firstTxn.amountMinor, firstTxn.currency ?? currency)}
          </span>
        );
      },
      sortable: true,
      sortValue: (d) => {
        const firstTxn = transactionsById[d.txnIds[0]];
        return firstTxn?.amountMinor ?? 0;
      },
    },
    {
      id: "score",
      header: "Score",
      align: "right",
      cell: (d) => {
        const score =
          d.score <= 1
            ? `${(d.score * 100).toFixed(0)}%`
            : String(d.score);
        return <span className="nums">{score}</span>;
      },
      sortable: true,
      sortValue: (d) => d.score,
    },
    {
      id: "type",
      header: "Type",
      cell: (d) => <StatusPill status={d.type} />,
    },
  ];
}

// ---------------------------------------------------------------------------
// Break columns (Unmatched tab)
// ---------------------------------------------------------------------------
function buildBreakColumns(
  transactionsById: Record<string, CanonicalTransaction>,
  currency: string
): Column<Break>[] {
  return [
    {
      id: "reference",
      header: "Reference",
      cell: (b) => {
        const ref =
          (b.txnIds[0] && transactionsById[b.txnIds[0]]?.externalRef) ??
          b.id;
        return (
          <span className="font-mono text-xs text-foreground truncate max-w-48 block">
            {ref}
          </span>
        );
      },
    },
    {
      id: "type",
      header: "Type",
      cell: (b) => <StatusPill status={b.type} />,
    },
    {
      id: "ageing",
      header: "Ageing",
      cell: (b) => (
        <span className="text-sm text-muted-foreground">{b.ageingBucket}</span>
      ),
      sortable: true,
      sortValue: (b) => b.ageingDays,
    },
    {
      id: "value",
      header: "Value",
      align: "right",
      cell: (b) => (
        <span className="nums">
          {formatMoney(b.valueMinor, b.currency ?? currency)}
        </span>
      ),
      sortable: true,
      sortValue: (b) => b.valueMinor,
    },
    {
      id: "status",
      header: "Status",
      cell: (b) => <StatusPill status={b.status} />,
    },
  ];
}

// ---------------------------------------------------------------------------
// Loading skeleton
// ---------------------------------------------------------------------------
function RunDetailSkeleton() {
  return (
    <div className="flex flex-col gap-6" aria-busy="true" aria-label="Loading run details">
      {/* Header skeleton */}
      <div className="flex flex-col gap-2 pb-4 border-b border-border">
        <Skeleton className="h-6 w-64" />
        <Skeleton className="h-4 w-48" />
      </div>
      {/* KPI row skeleton */}
      <div className="grid grid-cols-2 gap-3 sm:grid-cols-5">
        {Array.from({ length: 5 }).map((_, i) => (
          <Card key={i} size="sm">
            <CardContent className="flex flex-col gap-2 px-3 py-2">
              <Skeleton className="h-3 w-20" />
              <Skeleton className="h-7 w-14" />
            </CardContent>
          </Card>
        ))}
      </div>
      {/* Tabs skeleton */}
      <div className="flex flex-col gap-4">
        <div className="flex gap-2">
          {[1, 2, 3, 4].map((i) => (
            <Skeleton key={i} className="h-9 w-28 rounded-md" />
          ))}
        </div>
        <Card size="sm">
          <CardContent className="px-0 py-0">
            <div className="flex flex-col gap-2 p-4">
              {Array.from({ length: 5 }).map((_, i) => (
                <Skeleton key={i} className="h-8 w-full" />
              ))}
            </div>
          </CardContent>
        </Card>
      </div>
    </div>
  );
}

// ---------------------------------------------------------------------------
// Page
// ---------------------------------------------------------------------------
export default function RunDetailPage() {
  const { runId } = useParams<{ runId: string }>();
  const router = useRouter();

  const { data, isLoading, isError, refetch } = useRun(runId);

  if (isLoading) {
    return <RunDetailSkeleton />;
  }

  if (isError || !data) {
    return (
      <div
        role="alert"
        className="flex flex-col items-center justify-center gap-4 py-16"
      >
        <AlertCircle className="size-10 text-danger" aria-hidden />
        <div className="text-center">
          <p className="text-sm font-medium text-foreground">
            {isError ? "Failed to load run" : "Run not found"}
          </p>
          <p className="text-xs text-muted-foreground mt-1">
            {isError
              ? "There was a problem loading the run details."
              : `Run "${runId}" could not be found.`}
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

  const { run, transactionsById, matched, partial, duplicates, unmatched } =
    data;

  const currency = deriveCurrency(transactionsById);
  const decisionColumns = buildDecisionColumns(transactionsById, currency);
  const breakColumns = buildBreakColumns(transactionsById, currency);

  return (
    <div className="flex flex-col gap-6">
      {/* ---- Header ---- */}
      <PageHeader title={run.name}>
        <StatusPill status={run.status} />
        <span className="text-xs text-muted-foreground font-mono">
          {run.configVersion}
        </span>
      </PageHeader>

      {/* Timestamps row */}
      <div className="flex items-center gap-4 -mt-4 text-xs text-muted-foreground">
        <span>
          Started:{" "}
          <span className="text-foreground font-medium">
            {formatDate(run.startedAt)}
          </span>
        </span>
        {run.completedAt && (
          <span>
            Completed:{" "}
            <span className="text-foreground font-medium">
              {formatDate(run.completedAt)}
            </span>
          </span>
        )}
      </div>

      {/* ---- KPI row ---- */}
      <div className="grid grid-cols-2 gap-3 sm:grid-cols-5">
        <KpiCard
          label="Matched"
          value={<span className="nums">{run.stats.matched}</span>}
        />
        <KpiCard
          label="Partial"
          value={<span className="nums">{run.stats.partial}</span>}
        />
        <KpiCard
          label="Duplicate"
          value={<span className="nums">{run.stats.duplicate}</span>}
        />
        <KpiCard
          label="Breaks"
          value={<span className="nums">{run.stats.breakCount}</span>}
          hint="unmatched items"
        />
        <KpiCard
          label="Match rate"
          value={
            <span className="nums">{run.stats.matchRatePct.toFixed(1)}%</span>
          }
        />
      </div>

      {/* Value at risk (full width) */}
      <div className="flex items-center gap-2 text-xs text-muted-foreground">
        <span>Value at risk:</span>
        <span className="nums font-medium text-foreground">
          {formatMoney(run.stats.valueAtRiskMinor, currency)}
        </span>
      </div>

      {/* ---- Tabs ---- */}
      <Tabs defaultValue="matched">
        <TabsList>
          <TabsTrigger value="matched">
            Matched ({matched.length})
          </TabsTrigger>
          <TabsTrigger value="unmatched">
            Unmatched ({unmatched.length})
          </TabsTrigger>
          <TabsTrigger value="partial">
            Partial ({partial.length})
          </TabsTrigger>
          <TabsTrigger value="duplicates">
            Duplicates ({duplicates.length})
          </TabsTrigger>
        </TabsList>

        {/* Matched */}
        <TabsContent value="matched" className="mt-3">
          <Card size="sm">
            <CardContent className="px-0 py-0">
              <DataTable
                columns={decisionColumns}
                rows={matched}
                getRowId={(d) => d.id}
                emptyState={
                  <EmptyState
                    title="No matched decisions"
                    description="There are no matched transaction pairs for this run."
                  />
                }
              />
            </CardContent>
          </Card>
        </TabsContent>

        {/* Unmatched */}
        <TabsContent value="unmatched" className="mt-3">
          <Card size="sm">
            <CardContent className="px-0 py-0">
              <DataTable
                columns={breakColumns}
                rows={unmatched}
                getRowId={(b) => b.id}
                onRowClick={(b) => router.push(`/cases/${b.caseId}`)}
                emptyState={
                  <EmptyState
                    title="No unmatched breaks"
                    description="All transactions in this run were matched."
                  />
                }
              />
            </CardContent>
          </Card>
        </TabsContent>

        {/* Partial */}
        <TabsContent value="partial" className="mt-3">
          <Card size="sm">
            <CardContent className="px-0 py-0">
              <DataTable
                columns={decisionColumns}
                rows={partial}
                getRowId={(d) => d.id}
                emptyState={
                  <EmptyState
                    title="No partial matches"
                    description="There are no partial match decisions for this run."
                  />
                }
              />
            </CardContent>
          </Card>
        </TabsContent>

        {/* Duplicates */}
        <TabsContent value="duplicates" className="mt-3">
          <Card size="sm">
            <CardContent className="px-0 py-0">
              <DataTable
                columns={decisionColumns}
                rows={duplicates}
                getRowId={(d) => d.id}
                emptyState={
                  <EmptyState
                    title="No duplicates"
                    description="No duplicate transactions were detected in this run."
                  />
                }
              />
            </CardContent>
          </Card>
        </TabsContent>
      </Tabs>
    </div>
  );
}
