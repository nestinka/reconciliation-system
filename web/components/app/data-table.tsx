"use client";

import * as React from "react";
import { ChevronUp, ChevronDown } from "lucide-react";
import {
  Table,
  TableHeader,
  TableBody,
  TableRow,
  TableHead,
  TableCell,
} from "@/components/ui/table";
import { Checkbox } from "@/components/ui/checkbox";
import { Skeleton } from "@/components/ui/skeleton";
import { EmptyState } from "@/components/app/empty-state";
import { cn } from "@/lib/utils";

export interface Column<T> {
  id: string;
  header: React.ReactNode;
  cell: (row: T) => React.ReactNode;
  align?: "left" | "right" | "center";
  sortable?: boolean;
  sortValue?: (row: T) => string | number;
  className?: string;
  headerClassName?: string;
}

export interface DataTableProps<T> {
  columns: Column<T>[];
  rows: T[];
  getRowId: (row: T) => string;
  isLoading?: boolean;
  skeletonRows?: number;
  emptyState?: React.ReactNode;
  onRowClick?: (row: T) => void;
  selectable?: boolean;
  selectedIds?: string[];
  onSelectionChange?: (ids: string[]) => void;
  /** Accessible label for a row's selection checkbox. Defaults to the row id. */
  getRowLabel?: (row: T) => string;
}

type SortDirection = "asc" | "desc" | null;

interface SortState {
  columnId: string | null;
  direction: SortDirection;
}

function alignClass(align?: "left" | "right" | "center"): string {
  if (align === "right") return "text-right nums";
  if (align === "center") return "text-center";
  return "text-left";
}

export function DataTable<T>({
  columns,
  rows,
  getRowId,
  isLoading = false,
  skeletonRows = 8,
  emptyState,
  onRowClick,
  selectable = false,
  selectedIds = [],
  onSelectionChange,
  getRowLabel,
}: DataTableProps<T>): React.JSX.Element {
  const [sort, setSort] = React.useState<SortState>({ columnId: null, direction: null });

  // Client-side sort
  const sortedRows = React.useMemo(() => {
    if (!sort.columnId || !sort.direction) return rows;
    const col = columns.find((c) => c.id === sort.columnId);
    if (!col?.sortValue) return rows;
    const fn = col.sortValue;
    const factor = sort.direction === "asc" ? 1 : -1;
    return [...rows].sort((a, b) => {
      const av = fn(a);
      const bv = fn(b);
      if (av < bv) return -factor;
      if (av > bv) return factor;
      return 0;
    });
  }, [rows, sort, columns]);

  function handleHeaderClick(col: Column<T>) {
    if (!col.sortable) return;
    setSort((prev) => {
      if (prev.columnId !== col.id) return { columnId: col.id, direction: "asc" };
      if (prev.direction === "asc") return { columnId: col.id, direction: "desc" };
      return { columnId: null, direction: null };
    });
  }

  // Selection helpers
  const allIds = sortedRows.map(getRowId);
  const selectedSet = new Set(selectedIds);
  const allSelected = allIds.length > 0 && allIds.every((id) => selectedSet.has(id));
  const someSelected = allIds.some((id) => selectedSet.has(id)) && !allSelected;

  function handleHeaderCheckbox() {
    if (!onSelectionChange) return;
    if (allSelected) {
      onSelectionChange(selectedIds.filter((id) => !allIds.includes(id)));
    } else {
      const next = new Set(selectedIds);
      allIds.forEach((id) => next.add(id));
      onSelectionChange(Array.from(next));
    }
  }

  function handleRowCheckbox(id: string) {
    if (!onSelectionChange) return;
    if (selectedSet.has(id)) {
      onSelectionChange(selectedIds.filter((i) => i !== id));
    } else {
      onSelectionChange([...selectedIds, id]);
    }
  }

  // Total column count for colSpan
  const totalCols = columns.length + (selectable ? 1 : 0);

  return (
    <Table>
      {/* Sticky header relies on the table's scroll container; verify in-browser
          when the table lives inside an overflow context (see Table wrapper). */}
      <TableHeader className="sticky top-0 z-10 bg-card">
        <TableRow>
          {selectable && (
            <TableHead className="w-8 px-2">
              <Checkbox
                checked={allSelected}
                indeterminate={someSelected}
                onCheckedChange={handleHeaderCheckbox}
                aria-label="Select all rows"
              />
            </TableHead>
          )}
          {columns.map((col) => {
            const isActive = sort.columnId === col.id;
            return (
              <TableHead
                key={col.id}
                className={cn(alignClass(col.align), col.headerClassName)}
                aria-sort={
                  col.sortable
                    ? isActive && sort.direction === "asc"
                      ? "ascending"
                      : isActive && sort.direction === "desc"
                      ? "descending"
                      : "none"
                    : undefined
                }
              >
                {col.sortable ? (
                  // A real <button> inside the columnheader gives native
                  // keyboard operability while aria-sort stays on the <th>.
                  <button
                    type="button"
                    onClick={() => handleHeaderClick(col)}
                    className={cn(
                      "inline-flex items-center gap-1 select-none hover:text-foreground focus-visible:outline-none focus-visible:ring-2 focus-visible:ring-ring rounded-sm",
                      col.align === "right" && "flex-row-reverse"
                    )}
                  >
                    {col.header}
                    <span aria-hidden className="inline-flex text-muted-foreground">
                      {isActive && sort.direction === "asc" ? (
                        <ChevronUp className="size-3" />
                      ) : isActive && sort.direction === "desc" ? (
                        <ChevronDown className="size-3" />
                      ) : (
                        <ChevronUp className="size-3 opacity-30" />
                      )}
                    </span>
                  </button>
                ) : (
                  <span className="inline-flex items-center gap-1">
                    {col.header}
                  </span>
                )}
              </TableHead>
            );
          })}
        </TableRow>
      </TableHeader>
      <TableBody>
        {isLoading ? (
          Array.from({ length: skeletonRows }).map((_, ri) => (
            <TableRow key={ri}>
              {selectable && (
                <TableCell className="w-8 px-2">
                  <Skeleton className="size-4" />
                </TableCell>
              )}
              {columns.map((col) => (
                <TableCell key={col.id} className={cn(col.className)}>
                  <Skeleton className="h-4 w-full" />
                </TableCell>
              ))}
            </TableRow>
          ))
        ) : sortedRows.length === 0 ? (
          <TableRow>
            <TableCell colSpan={totalCols} className="p-0">
              {emptyState ?? (
                <EmptyState title="No results" description="No records match the current filters." />
              )}
            </TableCell>
          </TableRow>
        ) : (
          sortedRows.map((row) => {
            const id = getRowId(row);
            const isSelected = selectedSet.has(id);
            const clickable = Boolean(onRowClick);
            return (
              <TableRow
                key={id}
                data-state={isSelected ? "selected" : undefined}
                className={cn(clickable && "cursor-pointer hover:bg-muted/50 focus-visible:outline-none focus-visible:ring-2 focus-visible:ring-ring")}
                tabIndex={clickable ? 0 : undefined}
                onClick={clickable ? () => onRowClick!(row) : undefined}
                onKeyDown={
                  clickable
                    ? (e) => {
                        if (e.key === "Enter" || e.key === " ") {
                          e.preventDefault();
                          onRowClick!(row);
                        }
                      }
                    : undefined
                }
              >
                {selectable && (
                  <TableCell
                    className="w-8 px-2"
                    onClick={(e) => e.stopPropagation()}
                  >
                    <Checkbox
                      checked={isSelected}
                      onCheckedChange={() => handleRowCheckbox(id)}
                      aria-label={`Select ${getRowLabel ? getRowLabel(row) : `row ${id}`}`}
                    />
                  </TableCell>
                )}
                {columns.map((col) => (
                  <TableCell
                    key={col.id}
                    className={cn(alignClass(col.align), col.className)}
                  >
                    {col.cell(row)}
                  </TableCell>
                ))}
              </TableRow>
            );
          })
        )}
      </TableBody>
    </Table>
  );
}
