"use client";

import { Suspense, useEffect, useMemo, useState } from "react";
import { useRouter } from "next/navigation";
import {
  parseAsArrayOf,
  parseAsInteger,
  parseAsString,
  parseAsStringLiteral,
  useQueryState,
} from "nuqs";
import { toast } from "sonner";
import {
  Anchor as AnchorIcon,
  CheckCircle2,
  ChevronDown,
  ChevronRight,
  Copy,
  ShieldCheck,
  XCircle,
} from "lucide-react";

import { PageHeader } from "@/components/app/page-header";
import { Button } from "@/components/ui/button";
import { Input } from "@/components/ui/input";
import { Label } from "@/components/ui/label";
import { Badge } from "@/components/ui/badge";
import { Card, CardContent } from "@/components/ui/card";
import { Checkbox } from "@/components/ui/checkbox";
import { Skeleton } from "@/components/ui/skeleton";
import {
  Dialog,
  DialogContent,
  DialogFooter,
  DialogHeader,
  DialogTitle,
} from "@/components/ui/dialog";
import {
  Table,
  TableBody,
  TableCell,
  TableHead,
  TableHeader,
  TableRow,
} from "@/components/ui/table";
import { useAuth } from "@/lib/auth/auth-provider";
import { useApi } from "@/lib/api/provider";
import { useTenant } from "@/lib/providers/tenant-provider";
import { useAnchors, useAudit } from "@/lib/hooks/use-audit";
import { useMembers } from "@/lib/hooks/use-tenants";
import type {
  AuditEvent,
  AuditKind,
  AuditQuery,
  VerifyResult,
} from "@/lib/api/client";

// ---------------------------------------------------------------------------
// AuditKind list (must mirror the union in client.ts — 20 values)
// ---------------------------------------------------------------------------

const ALL_KINDS: readonly AuditKind[] = [
  "auth.login.success",
  "auth.login.failure",
  "auth.lockout",
  "auth.logout",
  "auth.password.changed",
  "auth.password.reset_requested",
  "auth.password.reset_completed",
  "auth.refresh.reused",
  "auth.tenant.switched",
  "admin.user.created",
  "admin.user.role_changed",
  "admin.user.disabled",
  "admin.user.enabled",
  "admin.user.removed",
  "data.source.created",
  "data.ingest.completed",
  "data.run.created",
  "case.assigned",
  "case.event_appended",
  "system.anchor.created",
] as const;

// ---------------------------------------------------------------------------
// Event family / colour mapping
// ---------------------------------------------------------------------------

type EventFamily = "auth" | "admin" | "data" | "case" | "system";

function familyForKind(kind: AuditKind): EventFamily {
  const prefix = kind.split(".")[0] as EventFamily;
  return prefix;
}

const FAMILY_CLASSES: Record<EventFamily, string> = {
  auth: "bg-blue-500/15 text-blue-700 dark:text-blue-300 border-blue-500/30",
  admin:
    "bg-purple-500/15 text-purple-700 dark:text-purple-300 border-purple-500/30",
  data: "bg-emerald-500/15 text-emerald-700 dark:text-emerald-300 border-emerald-500/30",
  case: "bg-amber-500/15 text-amber-700 dark:text-amber-300 border-amber-500/30",
  system: "bg-gray-500/15 text-gray-700 dark:text-gray-300 border-gray-500/30",
};

// ---------------------------------------------------------------------------
// Helpers
// ---------------------------------------------------------------------------

function formatAt(iso: string): string {
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

function truncatePayload(payload: Record<string, unknown>, max = 80): string {
  let str: string;
  try {
    str = JSON.stringify(payload);
  } catch {
    str = String(payload);
  }
  return str.length > max ? `${str.slice(0, max)}…` : str;
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
// Kind multi-select (checkbox list inside a popover-style panel)
// ---------------------------------------------------------------------------

function KindMultiSelect({
  value,
  onChange,
}: {
  value: AuditKind[];
  onChange: (next: AuditKind[]) => void;
}) {
  const [open, setOpen] = useState(false);
  const label =
    value.length === 0
      ? "All event kinds"
      : value.length === 1
        ? value[0]
        : `${value.length} kinds selected`;

  return (
    <div className="relative">
      <Button
        type="button"
        variant="outline"
        size="sm"
        onClick={() => setOpen((p) => !p)}
        aria-expanded={open}
        aria-haspopup="listbox"
        aria-label="Filter by event kind"
      >
        {label}
        <ChevronDown className="size-3.5" />
      </Button>
      {open && (
        <div
          role="listbox"
          aria-label="Event kinds"
          className="absolute z-40 mt-1 max-h-72 w-72 overflow-auto rounded-lg border border-border bg-popover p-2 shadow-md"
        >
          <div className="flex items-center justify-between gap-2 px-1 pb-2">
            <Button
              type="button"
              size="xs"
              variant="ghost"
              onClick={() => onChange([])}
              disabled={value.length === 0}
            >
              Clear
            </Button>
            <Button
              type="button"
              size="xs"
              variant="ghost"
              onClick={() => setOpen(false)}
            >
              Done
            </Button>
          </div>
          <ul className="flex flex-col gap-0.5">
            {ALL_KINDS.map((kind) => {
              const checked = value.includes(kind);
              const family = familyForKind(kind);
              return (
                <li key={kind}>
                  <label className="flex items-center gap-2 rounded-md px-1.5 py-1 hover:bg-muted cursor-pointer text-xs">
                    <Checkbox
                      checked={checked}
                      onCheckedChange={(next) => {
                        if (next) onChange([...value, kind]);
                        else onChange(value.filter((k) => k !== kind));
                      }}
                      aria-label={kind}
                    />
                    <span
                      className={`inline-block rounded px-1 py-0.5 border ${FAMILY_CLASSES[family]}`}
                    >
                      {kind}
                    </span>
                  </label>
                </li>
              );
            })}
          </ul>
        </div>
      )}
    </div>
  );
}

// ---------------------------------------------------------------------------
// Payload "view full" popup row
// ---------------------------------------------------------------------------

function PayloadCell({ payload }: { payload: Record<string, unknown> }) {
  const [open, setOpen] = useState(false);
  const summary = truncatePayload(payload, 80);
  return (
    <>
      <button
        type="button"
        className="text-left font-mono text-xs hover:underline"
        title="View full payload"
        onClick={() => setOpen(true)}
      >
        {summary}
      </button>
      <Dialog open={open} onOpenChange={setOpen}>
        <DialogContent className="sm:max-w-lg">
          <DialogHeader>
            <DialogTitle>Audit payload</DialogTitle>
          </DialogHeader>
          <pre className="max-h-[60vh] overflow-auto rounded-md bg-muted p-3 text-xs">
            {JSON.stringify(payload, null, 2)}
          </pre>
        </DialogContent>
      </Dialog>
    </>
  );
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
// Verify dialog
// ---------------------------------------------------------------------------

function VerifyDialog({
  open,
  onOpenChange,
}: {
  open: boolean;
  onOpenChange: (open: boolean) => void;
}) {
  const api = useApi();
  const { tenantId } = useTenant();
  const [from, setFrom] = useState("");
  const [to, setTo] = useState("");
  const [running, setRunning] = useState(false);
  const [result, setResult] = useState<VerifyResult | null>(null);
  const [error, setError] = useState<string | null>(null);

  async function handleRun() {
    setRunning(true);
    setError(null);
    setResult(null);
    try {
      const body: { from?: number; to?: number } = {};
      const fromNum = from.trim() === "" ? undefined : Number(from);
      const toNum = to.trim() === "" ? undefined : Number(to);
      if (fromNum !== undefined && !Number.isNaN(fromNum)) body.from = fromNum;
      if (toNum !== undefined && !Number.isNaN(toNum)) body.to = toNum;
      const r = await api.verifyAudit(tenantId, body);
      setResult(r);
    } catch (e) {
      setError(e instanceof Error ? e.message : "Verification failed.");
    } finally {
      setRunning(false);
    }
  }

  function handleClose(next: boolean) {
    if (!next) {
      // Reset state on close
      setResult(null);
      setError(null);
      setFrom("");
      setTo("");
    }
    onOpenChange(next);
  }

  return (
    <Dialog open={open} onOpenChange={handleClose}>
      <DialogContent>
        <DialogHeader>
          <DialogTitle>
            <span className="flex items-center gap-2">
              <ShieldCheck aria-hidden className="size-5" />
              Verify audit chain
            </span>
          </DialogTitle>
        </DialogHeader>

        <div className="flex flex-col gap-3">
          <div className="grid grid-cols-2 gap-3">
            <div className="flex flex-col gap-1.5">
              <Label htmlFor="verify-from">From seq (optional)</Label>
              <Input
                id="verify-from"
                type="number"
                inputMode="numeric"
                placeholder="e.g. 1"
                value={from}
                onChange={(e) => setFrom(e.target.value)}
              />
            </div>
            <div className="flex flex-col gap-1.5">
              <Label htmlFor="verify-to">To seq (optional)</Label>
              <Input
                id="verify-to"
                type="number"
                inputMode="numeric"
                placeholder="e.g. 1000"
                value={to}
                onChange={(e) => setTo(e.target.value)}
              />
            </div>
          </div>

          {error && (
            <p className="text-xs text-destructive" role="alert">
              {error}
            </p>
          )}

          {result && (
            <div
              role="status"
              aria-live="polite"
              className="rounded-md border border-border bg-muted/30 p-3 text-sm flex flex-col gap-1"
            >
              <div className="flex items-center gap-2 font-medium">
                {result.status === "valid" ? (
                  <>
                    <CheckCircle2
                      aria-hidden
                      className="size-4 text-emerald-600"
                    />
                    <span className="text-emerald-700 dark:text-emerald-300">
                      Valid
                    </span>
                  </>
                ) : (
                  <>
                    <XCircle aria-hidden className="size-4 text-destructive" />
                    <span className="text-destructive">Invalid</span>
                  </>
                )}
              </div>
              <div className="text-xs text-muted-foreground">
                Checked: <span className="tabular-nums">{result.checked}</span>
              </div>
              {result.status === "invalid" && (
                <>
                  {result.firstBrokenSeq !== undefined && (
                    <div className="text-xs">
                      First broken seq:{" "}
                      <span className="tabular-nums font-mono">
                        {result.firstBrokenSeq}
                      </span>
                    </div>
                  )}
                  {result.reason && (
                    <div className="text-xs">
                      Reason: <code>{result.reason}</code>
                    </div>
                  )}
                </>
              )}
            </div>
          )}
        </div>

        <DialogFooter>
          <Button
            variant="outline"
            onClick={() => handleClose(false)}
            disabled={running}
          >
            Close
          </Button>
          <Button onClick={handleRun} disabled={running}>
            {running ? "Verifying…" : "Run"}
          </Button>
        </DialogFooter>
      </DialogContent>
    </Dialog>
  );
}

// ---------------------------------------------------------------------------
// Anchor history (collapsible — only fetches when open)
// ---------------------------------------------------------------------------

function AnchorHistory() {
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
                <HashCell hex={a.hash} />
              </TableCell>
            </TableRow>
          ))}
        </TableBody>
      </Table>
    </div>
  );
}

// ---------------------------------------------------------------------------
// AuditPage inner — uses nuqs
// ---------------------------------------------------------------------------

const STATUS_VALUES = ["idle"] as const;

function AuditPageInner() {
  const { user } = useAuth();
  const router = useRouter();
  const api = useApi();
  const { tenantId } = useTenant();

  // Admin gate
  useEffect(() => {
    if (user && user.role !== "admin") {
      router.replace("/dashboard");
    }
  }, [user, router]);

  // URL-persisted filters
  const [kinds, setKinds] = useQueryState(
    "kind",
    parseAsArrayOf(parseAsStringLiteral(ALL_KINDS)).withDefault([])
  );
  const [actorId, setActorId] = useQueryState(
    "actor",
    parseAsString.withDefault("")
  );
  const [from, setFrom] = useQueryState("from", parseAsString.withDefault(""));
  const [to, setTo] = useQueryState("to", parseAsString.withDefault(""));
  const [before, setBefore] = useQueryState("before", parseAsInteger);
  // Pin a status param key (avoid TS unused warning on STATUS_VALUES)
  void STATUS_VALUES;

  // Members for resolving actor names (best-effort; falls back to raw id)
  const { data: members } = useMembers();
  const membersById = useMemo(
    () => Object.fromEntries((members ?? []).map((m) => [m.id, m])),
    [members]
  );

  // Build the AuditQuery
  const q = useMemo<AuditQuery>(() => {
    const out: AuditQuery = { limit: 50 };
    if (kinds.length > 0) out.kind = kinds;
    if (actorId.trim() !== "") out.actorId = actorId.trim();
    if (from.trim() !== "") out.from = from.trim();
    if (to.trim() !== "") out.to = to.trim();
    if (typeof before === "number") out.before = before;
    return out;
  }, [kinds, actorId, from, to, before]);

  const { data, isLoading, isError, refetch } = useAudit(q);
  const items = data?.items ?? [];
  const nextCursor = data?.nextCursor ?? null;

  // Verify dialog state
  const [showVerify, setShowVerify] = useState(false);

  // Anchor-now mutation state (manual; one-shot)
  const [anchoring, setAnchoring] = useState(false);

  async function handleAnchorNow() {
    if (anchoring) return;
    setAnchoring(true);
    try {
      const r = await api.anchorAudit(tenantId);
      toast.success(`Anchored at seq ${r.anchorSeq}`);
    } catch (e) {
      toast.error(
        e instanceof Error ? e.message : "Failed to anchor.",
      );
    } finally {
      setAnchoring(false);
    }
  }

  // Reset filters helper
  function clearAllFilters() {
    void setKinds([]);
    void setActorId("");
    void setFrom("");
    void setTo("");
    void setBefore(null);
  }

  // Don't render the page for non-admins (the redirect effect will run)
  if (user && user.role !== "admin") {
    return null;
  }

  return (
    <div className="flex flex-col gap-4">
      <div className="flex items-center justify-between gap-2">
        <PageHeader
          title="Audit log"
          description="Tamper-evident record of every security and data event."
        />
        <div className="flex items-center gap-2">
          <Button
            variant="outline"
            size="sm"
            onClick={() => setShowVerify(true)}
          >
            <ShieldCheck aria-hidden className="size-4" />
            Verify chain
          </Button>
          <Button
            size="sm"
            onClick={handleAnchorNow}
            disabled={anchoring}
            aria-label="Anchor now"
          >
            <AnchorIcon aria-hidden className="size-4" />
            {anchoring ? "Anchoring…" : "Anchor now"}
          </Button>
        </div>
      </div>

      <VerifyDialog open={showVerify} onOpenChange={setShowVerify} />

      {/* Filter bar */}
      <div className="flex items-end gap-3 flex-wrap">
        <div className="flex flex-col gap-1">
          <Label htmlFor="filter-kind">Event kind</Label>
          <KindMultiSelect value={kinds} onChange={(v) => void setKinds(v)} />
        </div>
        <div className="flex flex-col gap-1">
          <Label htmlFor="filter-actor">Actor</Label>
          <Input
            id="filter-actor"
            placeholder="actor id (e.g. user-ada)"
            className="w-52"
            value={actorId}
            onChange={(e) => void setActorId(e.target.value)}
          />
        </div>
        <div className="flex flex-col gap-1">
          <Label htmlFor="filter-from">From (ISO date)</Label>
          <Input
            id="filter-from"
            type="date"
            className="w-40"
            value={from}
            onChange={(e) => void setFrom(e.target.value)}
          />
        </div>
        <div className="flex flex-col gap-1">
          <Label htmlFor="filter-to">To (ISO date)</Label>
          <Input
            id="filter-to"
            type="date"
            className="w-40"
            value={to}
            onChange={(e) => void setTo(e.target.value)}
          />
        </div>
        <Button
          variant="ghost"
          size="sm"
          onClick={clearAllFilters}
          disabled={
            kinds.length === 0 &&
            actorId === "" &&
            from === "" &&
            to === "" &&
            before === null
          }
        >
          Clear filters
        </Button>
      </div>

      {/* Error state */}
      {isError && (
        <div
          role="alert"
          className="rounded-lg border border-destructive/30 bg-destructive/5 px-4 py-3 text-sm text-destructive flex items-center justify-between gap-4"
        >
          <span>Failed to load audit events.</span>
          <Button size="sm" variant="outline" onClick={() => void refetch()}>
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
                    const actorName = membersById[ev.actorId]?.name ?? ev.actorId;
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
              onClick={() => void setBefore(null)}
            >
              Reset to latest
            </Button>
          )}
          <Button
            variant="outline"
            size="sm"
            onClick={() => {
              if (nextCursor !== null) void setBefore(nextCursor);
            }}
            disabled={nextCursor === null}
          >
            Next
          </Button>
        </div>
      </div>

      {/* Anchor history */}
      <AnchorHistory />
    </div>
  );
}

// ---------------------------------------------------------------------------
// Suspense fallback
// ---------------------------------------------------------------------------

function AuditPageSkeleton() {
  return (
    <div className="flex flex-col gap-4">
      <div className="flex flex-col gap-0.5 pb-4 border-b border-border">
        <Skeleton className="h-6 w-32" />
        <Skeleton className="h-4 w-72 mt-1" />
      </div>
      <div className="flex items-center gap-3">
        <Skeleton className="h-9 w-40" />
        <Skeleton className="h-9 w-44" />
        <Skeleton className="h-9 w-40" />
      </div>
      <Card size="sm">
        <CardContent className="px-0 py-0">
          <div className="flex flex-col gap-0">
            {Array.from({ length: 6 }).map((_, i) => (
              <div
                key={i}
                className="flex gap-4 px-4 py-3 border-b border-border last:border-0"
              >
                <Skeleton className="h-4 w-24" />
                <Skeleton className="h-4 w-20" />
                <Skeleton className="h-4 w-36" />
                <Skeleton className="h-4 w-40" />
              </div>
            ))}
          </div>
        </CardContent>
      </Card>
    </div>
  );
}

export default function AuditPage() {
  return (
    <Suspense fallback={<AuditPageSkeleton />}>
      <AuditPageInner />
    </Suspense>
  );
}
