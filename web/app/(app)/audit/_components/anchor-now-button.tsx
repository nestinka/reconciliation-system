"use client";

import { useState } from "react";
import { Anchor as AnchorIcon, ChevronDown, ChevronRight } from "lucide-react";
import { toast } from "sonner";
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
import { useApi } from "@/lib/api/provider";
import { useTenant } from "@/lib/providers/tenant-provider";
import { useAnchors } from "@/lib/hooks/use-audit";
import { formatAt } from "./audit-table";

// ---------------------------------------------------------------------------
// Anchor history (collapsible — only fetches when open)
// ---------------------------------------------------------------------------

function HashCellLocal({ hex }: { hex: string }) {
  const shortHex = hex.length > 8 ? hex.slice(0, 8) : hex;
  return (
    <button
      type="button"
      className="inline-flex items-center gap-1 font-mono text-xs hover:underline"
      title={`Copy ${hex}`}
      onClick={() => {
        void (async () => {
          try {
            if (typeof navigator !== "undefined" && navigator.clipboard) {
              await navigator.clipboard.writeText(hex);
              toast.success("Copied to clipboard.");
              return;
            }
          } catch {
            // fall through
          }
          toast.error("Clipboard unavailable.");
        })();
      }}
    >
      {shortHex}
    </button>
  );
}

function AnchorHistoryBody() {
  const { data: anchors, isLoading } = useAnchors(20);
  if (isLoading) {
    return (
      <div className="mt-3 flex flex-col gap-2">
        {Array.from({ length: 3 }).map((_, i) => (
          <Skeleton key={i} className="h-8 w-full" />
        ))}
      </div>
    );
  }
  if (!anchors || anchors.length === 0) {
    return (
      <p className="mt-3 text-sm text-muted-foreground">No anchors yet.</p>
    );
  }
  return (
    <div className="mt-3 overflow-hidden rounded-md border border-border">
      <Table>
        <TableHeader>
          <TableRow>
            <TableHead>Seq</TableHead>
            <TableHead>At</TableHead>
            <TableHead>Hash</TableHead>
          </TableRow>
        </TableHeader>
        <TableBody>
          {anchors.map((a) => (
            <TableRow key={a.anchorSeq}>
              <TableCell className="font-mono tabular-nums">
                {a.anchorSeq}
              </TableCell>
              <TableCell>{formatAt(a.at)}</TableCell>
              <TableCell>
                <HashCellLocal hex={a.hash} />
              </TableCell>
            </TableRow>
          ))}
        </TableBody>
      </Table>
    </div>
  );
}

export function AnchorHistory() {
  const [open, setOpen] = useState(false);
  return (
    <Card size="sm">
      <CardContent className="p-3">
        <button
          type="button"
          className="flex w-full items-center gap-2 text-sm font-medium"
          onClick={() => setOpen((p) => !p)}
          aria-expanded={open}
        >
          {open ? (
            <ChevronDown aria-hidden className="size-4" />
          ) : (
            <ChevronRight aria-hidden className="size-4" />
          )}
          Anchor history
        </button>
        {open && <AnchorHistoryBody />}
      </CardContent>
    </Card>
  );
}

// ---------------------------------------------------------------------------
// AnchorNowButton
// ---------------------------------------------------------------------------

export function AnchorNowButton() {
  const api = useApi();
  const { tenantId } = useTenant();
  const [anchoring, setAnchoring] = useState(false);

  async function handleAnchorNow() {
    if (anchoring) return;
    setAnchoring(true);
    try {
      const r = await api.anchorAudit(tenantId);
      toast.success(`Anchored at seq ${r.anchorSeq}`);
    } catch (e) {
      toast.error(e instanceof Error ? e.message : "Failed to anchor.");
    } finally {
      setAnchoring(false);
    }
  }

  return (
    <Button
      size="sm"
      onClick={handleAnchorNow}
      disabled={anchoring}
      aria-label="Anchor now"
    >
      <AnchorIcon aria-hidden className="size-4" />
      {anchoring ? "Anchoring…" : "Anchor now"}
    </Button>
  );
}
