import { StatusPill } from "@/components/app/status-pill";
import type { Column } from "@/components/app/data-table";
import { formatMoney } from "@/lib/domain/money";
import type { Break, CanonicalTransaction, User } from "@/lib/domain/types";

export interface BreakColumnOptions {
  /** Default currency when a break carries none. */
  currency?: string;
  /** When provided, adds a leading Reference column resolving the first txn. */
  transactionsById?: Record<string, CanonicalTransaction>;
  /** When provided, adds an Assignee column resolving assigneeId → user name. */
  usersById?: Record<string, User>;
}

/**
 * Shared column definitions for tables of breaks/exceptions. Used by the run
 * detail "Unmatched" tab (with `transactionsById`) and the Exceptions list
 * screen (with `usersById`). Columns are composed based on the options so the
 * two screens stay consistent without duplicating cell logic.
 */
export function buildBreakColumns(
  opts: BreakColumnOptions = {}
): Column<Break>[] {
  const { currency = "GBP", transactionsById, usersById } = opts;
  const columns: Column<Break>[] = [];

  if (transactionsById) {
    columns.push({
      id: "reference",
      header: "Reference",
      cell: (b) => {
        const ref =
          (b.txnIds[0] && transactionsById[b.txnIds[0]]?.externalRef) ?? b.id;
        return (
          <span className="font-mono text-xs text-foreground truncate max-w-48 block">
            {ref}
          </span>
        );
      },
    });
  }

  columns.push(
    {
      id: "type",
      header: "Type",
      cell: (b) => <StatusPill status={b.type} />,
      sortable: true,
      sortValue: (b) => b.type,
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
        <span className="nums">{formatMoney(b.valueMinor, b.currency ?? currency)}</span>
      ),
      sortable: true,
      sortValue: (b) => b.valueMinor,
    }
  );

  if (usersById) {
    columns.push({
      id: "assignee",
      header: "Assignee",
      cell: (b) =>
        b.assigneeId ? (
          <span className="text-sm">{usersById[b.assigneeId]?.name ?? b.assigneeId}</span>
        ) : (
          <span className="text-sm text-muted-foreground/60">Unassigned</span>
        ),
      sortable: true,
      sortValue: (b) => (b.assigneeId ? usersById[b.assigneeId]?.name ?? "" : ""),
    });
  }

  columns.push({
    id: "status",
    header: "Status",
    cell: (b) => <StatusPill status={b.status} />,
    sortable: true,
    sortValue: (b) => b.status,
  });

  return columns;
}
