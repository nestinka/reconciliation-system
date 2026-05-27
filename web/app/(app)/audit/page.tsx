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
import { ShieldCheck } from "lucide-react";

import { PageHeader } from "@/components/app/page-header";
import { Button } from "@/components/ui/button";
import { useAuth } from "@/lib/auth/auth-provider";
import { useAudit } from "@/lib/hooks/use-audit";
import { useMembers } from "@/lib/hooks/use-tenants";
import type { AuditQuery } from "@/lib/api/client";

import { ALL_KINDS, AuditFilterBar } from "./_components/audit-filter-bar";
import { AuditTable } from "./_components/audit-table";
import { VerifyChainDialog } from "./_components/verify-chain-dialog";
import { AnchorNowButton, AnchorHistory } from "./_components/anchor-now-button";
import { AuditPageSkeleton } from "./_components/audit-page-skeleton";

// Pin STATUS_VALUES to avoid TS unused warning
const STATUS_VALUES = ["idle"] as const;
void STATUS_VALUES;

function AuditPageInner() {
  const { user } = useAuth();
  const router = useRouter();

  useEffect(() => {
    if (user && user.role !== "admin") router.replace("/dashboard");
  }, [user, router]);

  const [kinds, setKinds] = useQueryState(
    "kind",
    parseAsArrayOf(parseAsStringLiteral(ALL_KINDS)).withDefault([])
  );
  const [actorId, setActorId] = useQueryState("actor", parseAsString.withDefault(""));
  const [from, setFrom] = useQueryState("from", parseAsString.withDefault(""));
  const [to, setTo] = useQueryState("to", parseAsString.withDefault(""));
  const [before, setBefore] = useQueryState("before", parseAsInteger);

  const { data: members } = useMembers();
  const membersById = useMemo(
    () => Object.fromEntries((members ?? []).map((m) => [m.id, m])),
    [members]
  );

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

  const [showVerify, setShowVerify] = useState(false);

  function clearAllFilters() {
    void setKinds([]);
    void setActorId("");
    void setFrom("");
    void setTo("");
    void setBefore(null);
  }

  if (user && user.role !== "admin") return null;

  return (
    <div className="flex flex-col gap-4">
      <div className="flex items-center justify-between gap-2">
        <PageHeader
          title="Audit log"
          description="Tamper-evident record of every security and data event."
        />
        <div className="flex items-center gap-2">
          <Button variant="outline" size="sm" onClick={() => setShowVerify(true)}>
            <ShieldCheck aria-hidden className="size-4" />
            Verify chain
          </Button>
          <AnchorNowButton />
        </div>
      </div>

      <VerifyChainDialog open={showVerify} onOpenChange={setShowVerify} />

      <AuditFilterBar
        kinds={kinds}
        onKindsChange={(v) => void setKinds(v)}
        actorId={actorId}
        onActorIdChange={(v) => void setActorId(v)}
        from={from}
        onFromChange={(v) => void setFrom(v)}
        to={to}
        onToChange={(v) => void setTo(v)}
        canClear={kinds.length > 0 || actorId !== "" || from !== "" || to !== "" || before !== null}
        onClearAll={clearAllFilters}
      />

      <AuditTable
        items={items}
        isLoading={isLoading}
        isError={isError}
        membersById={membersById}
        onRetry={() => void refetch()}
        nextCursor={nextCursor}
        before={before}
        onResetToLatest={() => void setBefore(null)}
        onNext={() => { if (nextCursor !== null) void setBefore(nextCursor); }}
      />

      <AnchorHistory />
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
