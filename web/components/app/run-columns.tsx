import { StatusPill } from "@/components/app/status-pill";
import type { Column } from "@/components/app/data-table";
import { formatMoney } from "@/lib/domain/money";
import { formatDate } from "@/lib/domain/date";
import type { ReconciliationRun } from "@/lib/domain/types";

/**
 * Shared column definitions for tables of reconciliation runs.
 * Used by the dashboard "recent runs" table and the /runs list screen so the
 * two stay consistent. Pass the active currency for value formatting.
 */
export function buildRunColumns(currency: string): Column<ReconciliationRun>[] {
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
      sortable: true,
      sortValue: (r) => r.status,
    },
    {
      id: "matchRate",
      header: "Match rate",
      align: "right",
      cell: (r) => <span className="nums">{r.stats.matchRatePct.toFixed(1)}%</span>,
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
        <span className="nums">{formatMoney(r.stats.valueAtRiskMinor, currency)}</span>
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
          <span className="nums text-muted-foreground">{formatDate(r.completedAt)}</span>
        ) : (
          <span className="text-muted-foreground/50">—</span>
        ),
      sortable: true,
      sortValue: (r) => r.completedAt ?? "",
    },
  ];
}
