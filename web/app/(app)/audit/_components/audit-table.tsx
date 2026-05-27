"use client";

import { Copy } from "lucide-react";
import { toast } from "sonner";
import { Badge } from "@/components/ui/badge";
import { Button } from "@/components/ui/button";
import { Card, CardContent } from "@/components/ui/card";
import { Skeleton } from "@/components/ui/skeleton";
import {
  Table,
  TableBody,
  TableCell,
  TableHead,
  TableHeader,
  TableRow,
} from "@/components/ui/table";
import type { AuditEvent } from "@/lib/api/client";
import { PayloadCell } from "./event-detail-dialog";
import { familyForKind, FAMILY_CLASSES } from "./audit-filter-bar";

// ---------------------------------------------------------------------------
// Helpers
// ---------------------------------------------------------------------------

export function formatAt(iso: string): string {
  try {
    const d = new Date(iso);
    if (Number.isNaN(d.getTime())) return iso;
    return d.toLocaleString();
  } catch {
    return iso;
  }
}

function shortHex(hex: string): string {
  return hex.length > 8 ? hex.slice(0, 8) : hex;
}

async function copyToClipboard(text: string): Promise<void> {
  try {
    if (typeof navigator !== "undefined" && navigator.clipboard) {
      await navigator.clipboard.writeText(text);
      toast.success("Copied to clipboard.");
      return;
    }
  } catch {
    // fall through
  }
  toast.error("Clipboard unavailable.");
}

// ---------------------------------------------------------------------------
// Hash cell with copy
// ---------------------------------------------------------------------------

function HashCell({ hex }: { hex: string }) {
  return (
    <button
      type="button"
      className="inline-flex items-center gap-1 font-mono text-xs hover:underline"
      title={`Copy ${hex}`}
      onClick={() => void copyToClipboard(hex)}
    >
      <Copy aria-hidden className="size-3 text-muted-foreground" />
      {shortHex(hex)}
    </button>
  );
}

// ---------------------------------------------------------------------------
// AuditTable
// ---------------------------------------------------------------------------

export interface AuditTableProps {
  items: AuditEvent[];
  isLoading: boolean;
  isError: boolean;
  membersById: Record<string, { name: string } | undefined>;
  onRetry: () => void;
  nextCursor: number | null;
  before: number | null;
  onResetToLatest: () => void;
  onNext: () => void;
}

export function AuditTable({
  items,
  isLoading,
  isError,
  membersById,
  onRetry,
  nextCursor,
  before,
  onResetToLatest,
  onNext,
}: AuditTableProps) {
  return (
    <>
      {/* Error state */}
      {isError && (
        <div
          role="alert"
          className="rounded-lg border border-destructive/30 bg-destructive/5 px-4 py-3 text-sm text-destructive flex items-center justify-between gap-4"
        >
          <span>Failed to load audit events.</span>
          <Button size="sm" variant="outline" onClick={onRetry}>
            Retry
          </Button>
        </div>
      )}

      {/* Table */}
      {!isError && (
        <Card size="sm">
          <CardContent className="px-0 py-0">
            <Table>
              <TableHeader>
                <TableRow>
                  <TableHead>At</TableHead>
                  <TableHead>Actor</TableHead>
                  <TableHead>Kind</TableHead>
                  <TableHead>Payload</TableHead>
                  <TableHead>prev</TableHead>
                  <TableHead>hash</TableHead>
                  <TableHead className="text-right">seq</TableHead>
                </TableRow>
              </TableHeader>
              <TableBody>
                {isLoading && items.length === 0 ? (
                  Array.from({ length: 6 }).map((_, i) => (
                    <TableRow key={`sk-${i}`}>
                      <TableCell colSpan={7}>
                        <Skeleton className="h-5 w-full" />
                      </TableCell>
                    </TableRow>
                  ))
                ) : items.length === 0 ? (
                  <TableRow>
                    <TableCell
                      colSpan={7}
                      className="text-center text-muted-foreground py-8"
                    >
                      No audit events match the selected filters.
                    </TableCell>
                  </TableRow>
                ) : (
                  items.map((ev: AuditEvent) => {
                    const family = familyForKind(ev.kind);
                    const actorName =
                      membersById[ev.actorId]?.name ?? ev.actorId;
                    return (
                      <TableRow key={`${ev.tenantId}-${ev.seq}`}>
                        <TableCell className="whitespace-nowrap">
                          {formatAt(ev.at)}
                        </TableCell>
                        <TableCell title={ev.actorId}>{actorName}</TableCell>
                        <TableCell>
                          <Badge
                            variant="outline"
                            className={`border ${FAMILY_CLASSES[family]}`}
                          >
                            {ev.kind}
                          </Badge>
                        </TableCell>
                        <TableCell className="max-w-xs">
                          <PayloadCell payload={ev.payload} />
                        </TableCell>
                        <TableCell>
                          <HashCell hex={ev.prevHash} />
                        </TableCell>
                        <TableCell>
                          <HashCell hex={ev.hash} />
                        </TableCell>
                        <TableCell className="text-right font-mono tabular-nums">
                          {ev.seq}
                        </TableCell>
                      </TableRow>
                    );
                  })
                )}
              </TableBody>
            </Table>
          </CardContent>
        </Card>
      )}

      {/* Pagination */}
      <div className="flex items-center justify-between">
        <span className="text-xs text-muted-foreground">
          {items.length > 0
            ? `Showing ${items.length} event${items.length === 1 ? "" : "s"}`
            : null}
        </span>
        <div className="flex items-center gap-2">
          {before !== null && (
            <Button
              variant="outline"
              size="sm"
              onClick={onResetToLatest}
            >
              Reset to latest
            </Button>
          )}
          <Button
            variant="outline"
            size="sm"
            onClick={onNext}
            disabled={nextCursor === null}
          >
            Next
          </Button>
        </div>
      </div>
    </>
  );
}
