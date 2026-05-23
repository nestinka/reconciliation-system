"use client";

import { useRouter } from "next/navigation";
import {
  BarChart,
  Bar,
  Cell,
  ResponsiveContainer,
  XAxis,
  YAxis,
  Tooltip,
} from "recharts";

import { PageHeader } from "@/components/app/page-header";
import { KpiCard } from "@/components/app/kpi-card";
import { StatusPill } from "@/components/app/status-pill";
import { DataTable, type Column } from "@/components/app/data-table";
import {
  Card,
  CardContent,
  CardHeader,
  CardTitle,
} from "@/components/ui/card";
import { Skeleton } from "@/components/ui/skeleton";
import { Button } from "@/components/ui/button";
import { useDashboard } from "@/lib/hooks/use-dashboard";
import { formatMoney } from "@/lib/domain/money";
import type { ReconciliationRun, BreakType, AgeingBucket } from "@/lib/domain/types";

// -----------------------------------------------------------------------
// Chart colours (maps to CSS vars set in globals.css)
// -----------------------------------------------------------------------
const CHART_COLORS = [
  "var(--color-chart-1)",
  "var(--color-chart-2)",
  "var(--color-chart-3)",
  "var(--color-chart-4)",
];

const BREAK_TYPE_LABELS: Record<BreakType, string> = {
  unmatched: "Unmatched",
  partial: "Partial",
  duplicate: "Duplicate",
  break: "Break",
};

const AGEING_URGENCY: Record<AgeingBucket, { label: string; barClass: string }> = {
  "0-1d": { label: "0–1 day", barClass: "bg-success" },
  "2-7d": { label: "2–7 days", barClass: "bg-warning" },
  "8-30d": { label: "8–30 days", barClass: "bg-danger/70" },
  "30d+": { label: "30d+", barClass: "bg-danger" },
};

// -----------------------------------------------------------------------
// Recent-runs table columns
// -----------------------------------------------------------------------
function useRunColumns(
  currency: string
): Column<ReconciliationRun>[] {
  return [
    {
      id: "name",
      header: "Name",
      cell: (r) => (
        <span className="font-medium text-foreground truncate max-w-48 block">
          {r.name}
        </span>
      ),
      sortable: true,
      sortValue: (r) => r.name,
    },
    {
      id: "status",
      header: "Status",
      cell: (r) => <StatusPill status={r.status} />,
    },
    {
      id: "matchRate",
      header: "Match rate",
      align: "right",
      cell: (r) => (
        <span className="nums">{r.stats.matchRatePct.toFixed(1)}%</span>
      ),
      sortable: true,
      sortValue: (r) => r.stats.matchRatePct,
    },
    {
      id: "breaks",
      header: "Breaks",
      align: "right",
      cell: (r) => <span className="nums">{r.stats.breakCount}</span>,
      sortable: true,
      sortValue: (r) => r.stats.breakCount,
    },
    {
      id: "var",
      header: "Value at risk",
      align: "right",
      cell: (r) => (
        <span className="nums">
          {formatMoney(r.stats.valueAtRiskMinor, currency)}
        </span>
      ),
      sortable: true,
      sortValue: (r) => r.stats.valueAtRiskMinor,
    },
    {
      id: "completed",
      header: "Completed",
      align: "right",
      cell: (r) =>
        r.completedAt ? (
          <span className="nums text-muted-foreground">
            {new Date(r.completedAt).toLocaleDateString("en-GB", {
              day: "2-digit",
              month: "short",
              year: "numeric",
            })}
          </span>
        ) : (
          <span className="text-muted-foreground/50">—</span>
        ),
      sortable: true,
      sortValue: (r) => r.completedAt ?? "",
    },
  ];
}

// -----------------------------------------------------------------------
// KPI skeleton row
// -----------------------------------------------------------------------
function KpiSkeletonRow() {
  return (
    <div className="grid grid-cols-2 gap-3 sm:grid-cols-4">
      {Array.from({ length: 4 }).map((_, i) => (
        <Card key={i} size="sm">
          <CardContent className="flex flex-col gap-2 px-3 py-2">
            <Skeleton className="h-3 w-24" />
            <Skeleton className="h-7 w-16" />
          </CardContent>
        </Card>
      ))}
    </div>
  );
}

// -----------------------------------------------------------------------
// Page
// -----------------------------------------------------------------------
export default function DashboardPage() {
  const router = useRouter();
  const { data, isLoading, isError, refetch } = useDashboard();

  const runColumns = useRunColumns(data?.currency ?? "GBP");

  return (
    <div className="flex flex-col gap-6">
      <PageHeader
        title="Dashboard"
        description="Reconciliation health across your sources."
      />

      {/* ---- KPI row ---- */}
      {isLoading ? (
        <KpiSkeletonRow />
      ) : isError ? (
        <div
          role="alert"
          className="rounded-lg border border-danger/30 bg-danger/5 px-4 py-3 text-sm text-danger flex items-center justify-between gap-4"
        >
          <span>Failed to load dashboard data.</span>
          <Button
            size="sm"
            variant="outline"
            onClick={() => refetch()}
            className="shrink-0"
          >
            Retry
          </Button>
        </div>
      ) : data ? (
        <>
          {/* KPI cards */}
          <div className="grid grid-cols-2 gap-3 sm:grid-cols-4">
            <KpiCard
              label="Match rate"
              value={<span className="nums">{data.matchRatePct.toFixed(1)}%</span>}
              hint="avg across completed runs"
            />
            <KpiCard
              label="Open breaks"
              value={<span className="nums">{data.openBreaks}</span>}
              hint="open + investigating + pending"
            />
            <KpiCard
              label="Value at risk"
              value={
                <span className="nums">
                  {formatMoney(data.valueAtRiskMinor, data.currency)}
                </span>
              }
              hint="sum across open breaks"
            />
            <KpiCard
              label="SLA adherence"
              value={<span className="nums">{data.slaAdherencePct.toFixed(1)}%</span>}
              hint="resolved ≤7 days"
            />
          </div>

          {/* ---- Analysis row ---- */}
          <div className="grid grid-cols-1 gap-4 lg:grid-cols-2">
            {/* Break analysis by type */}
            <Card size="sm">
              <CardHeader>
                <CardTitle>Breaks by type</CardTitle>
              </CardHeader>
              <CardContent>
                {/* Accessible list — data is NOT chart-only */}
                <ul
                  aria-label="Break counts by type"
                  className="mb-4 grid grid-cols-2 gap-x-6 gap-y-1 text-sm"
                >
                  {data.breaksByType.map(({ type, count }) => (
                    <li key={type} className="flex items-center justify-between gap-2">
                      <span className="text-muted-foreground">
                        {BREAK_TYPE_LABELS[type]}
                      </span>
                      <span className="nums font-medium">{count}</span>
                    </li>
                  ))}
                </ul>

                {/* Recharts bar — visualisation only, data above is the a11y source */}
                <div aria-hidden style={{ height: 120 }}>
                  <ResponsiveContainer width="100%" height="100%">
                    <BarChart
                      data={data.breaksByType.map((d) => ({
                        name: BREAK_TYPE_LABELS[d.type],
                        count: d.count,
                      }))}
                      margin={{ top: 4, right: 4, left: -24, bottom: 0 }}
                    >
                      <XAxis
                        dataKey="name"
                        tick={{ fontSize: 11 }}
                        axisLine={false}
                        tickLine={false}
                      />
                      <YAxis
                        tick={{ fontSize: 11 }}
                        axisLine={false}
                        tickLine={false}
                        allowDecimals={false}
                      />
                      <Tooltip
                        contentStyle={{ fontSize: 12 }}
                        cursor={{ fill: "var(--color-muted)" }}
                      />
                      <Bar dataKey="count" radius={[3, 3, 0, 0]}>
                        {data.breaksByType.map((_, idx) => (
                          <Cell
                            key={idx}
                            fill={CHART_COLORS[idx % CHART_COLORS.length]}
                          />
                        ))}
                      </Bar>
                    </BarChart>
                  </ResponsiveContainer>
                </div>
              </CardContent>
            </Card>

            {/* Break ageing */}
            <Card size="sm">
              <CardHeader>
                <CardTitle>Break ageing</CardTitle>
              </CardHeader>
              <CardContent>
                <ul
                  aria-label="Open break counts by age bucket"
                  className="flex flex-col gap-2"
                >
                  {data.breaksByAgeing.map(({ bucket, count }) => {
                    const { label, barClass } = AGEING_URGENCY[bucket];
                    const maxCount = Math.max(
                      ...data.breaksByAgeing.map((b) => b.count),
                      1
                    );
                    const pct = Math.round((count / maxCount) * 100);
                    return (
                      <li key={bucket} className="flex flex-col gap-0.5">
                        <div className="flex items-center justify-between text-xs">
                          <span className="text-muted-foreground">{label}</span>
                          <span className="nums font-medium">{count}</span>
                        </div>
                        <div
                          className="h-1.5 w-full rounded-full bg-muted overflow-hidden"
                          role="presentation"
                        >
                          <div
                            className={`h-full rounded-full ${barClass}`}
                            style={{ width: `${pct}%` }}
                          />
                        </div>
                      </li>
                    );
                  })}
                </ul>
              </CardContent>
            </Card>
          </div>

          {/* ---- Recent runs table ---- */}
          <section aria-labelledby="recent-runs-heading">
            <h2
              id="recent-runs-heading"
              className="text-sm font-medium text-muted-foreground uppercase tracking-wide mb-2"
            >
              Recent runs
            </h2>
            <Card size="sm">
              <CardContent className="px-0 py-0">
                <DataTable
                  columns={runColumns}
                  rows={data.recentRuns}
                  getRowId={(r) => r.id}
                  isLoading={false}
                  skeletonRows={5}
                  onRowClick={(r) => router.push(`/runs/${r.id}`)}
                  emptyState={
                    <div className="py-8 text-center text-sm text-muted-foreground">
                      No completed runs yet.
                    </div>
                  }
                />
              </CardContent>
            </Card>
          </section>
        </>
      ) : null}

      {/* Loading state for the two analysis cards and table */}
      {isLoading && (
        <>
          <div className="grid grid-cols-1 gap-4 lg:grid-cols-2">
            {[0, 1].map((i) => (
              <Card key={i} size="sm">
                <CardHeader>
                  <Skeleton className="h-4 w-32" />
                </CardHeader>
                <CardContent className="flex flex-col gap-2">
                  {Array.from({ length: 4 }).map((_, j) => (
                    <Skeleton key={j} className="h-3 w-full" />
                  ))}
                </CardContent>
              </Card>
            ))}
          </div>
          <section aria-labelledby="recent-runs-loading-heading">
            <h2
              id="recent-runs-loading-heading"
              className="text-sm font-medium text-muted-foreground uppercase tracking-wide mb-2"
            >
              Recent runs
            </h2>
            <Card size="sm">
              <CardContent className="px-0 py-0">
                <DataTable
                  columns={runColumns}
                  rows={[]}
                  getRowId={(r) => r.id}
                  isLoading
                  skeletonRows={5}
                />
              </CardContent>
            </Card>
          </section>
        </>
      )}
    </div>
  );
}
