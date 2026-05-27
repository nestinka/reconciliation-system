"use client";

import { useEffect, useMemo } from "react";
import { useRouter } from "next/navigation";

import { PageHeader } from "@/components/app/page-header";
import { Badge } from "@/components/ui/badge";
import { Card, CardContent } from "@/components/ui/card";
import { Skeleton } from "@/components/ui/skeleton";
import { useAuth } from "@/lib/auth/auth-provider";
import { useControls } from "@/lib/hooks/use-audit";
import type { AuditKind, Control } from "@/lib/api/client";

// ---------------------------------------------------------------------------
// Event family / colour mapping (mirrors audit/page.tsx so chips look consistent)
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

function buildAuditHref(kinds: AuditKind[]): string {
  const params = new URLSearchParams();
  for (const k of kinds) params.append("kind", k);
  return `/audit?${params.toString()}`;
}

function groupByFramework(controls: Control[]): Map<string, Control[]> {
  const out = new Map<string, Control[]>();
  for (const c of controls) {
    const list = out.get(c.framework);
    if (list) list.push(c);
    else out.set(c.framework, [c]);
  }
  return out;
}

// ---------------------------------------------------------------------------
// Page
// ---------------------------------------------------------------------------

export default function ControlsPage() {
  const { user } = useAuth();
  const router = useRouter();

  // Admin gate (mirrors web/app/(app)/users/page.tsx + audit/page.tsx)
  useEffect(() => {
    if (user && user.role !== "admin") {
      router.replace("/dashboard");
    }
  }, [user, router]);

  const { data: controls, isLoading, isError } = useControls();

  const grouped = useMemo(() => {
    if (!controls) return null;
    return groupByFramework(controls);
  }, [controls]);

  // Don't render the page for non-admins (the redirect effect will run)
  if (user && user.role !== "admin") {
    return null;
  }

  return (
    <div className="flex flex-col gap-6">
      <PageHeader
        title="Controls"
        description="ISO 27001 / SOC 2 / FCA control items mapped to the audit-event kinds that demonstrate them."
      />

      {isError && (
        <div
          role="alert"
          className="rounded-lg border border-destructive/30 bg-destructive/5 px-4 py-3 text-sm text-destructive"
        >
          Failed to load controls registry.
        </div>
      )}

      {isLoading && !grouped && (
        <div className="flex flex-col gap-4">
          {Array.from({ length: 3 }).map((_, i) => (
            <Card key={i} size="sm">
              <CardContent className="flex flex-col gap-3 p-4">
                <Skeleton className="h-5 w-32" />
                <Skeleton className="h-12 w-full" />
                <Skeleton className="h-12 w-full" />
              </CardContent>
            </Card>
          ))}
        </div>
      )}

      {grouped && grouped.size === 0 && !isLoading && (
        <p className="text-sm text-muted-foreground">No controls defined.</p>
      )}

      {grouped &&
        Array.from(grouped.entries()).map(([framework, items]) => (
          <section
            key={framework}
            aria-labelledby={`framework-${framework}`}
            className="flex flex-col gap-2"
          >
            <h2
              id={`framework-${framework}`}
              className="text-sm font-semibold text-foreground"
            >
              {framework}
            </h2>
            <Card size="sm">
              <CardContent className="flex flex-col gap-0 p-0">
                {items.map((c, idx) => {
                  const href = buildAuditHref(c.eventKinds);
                  return (
                    <button
                      key={c.id}
                      type="button"
                      onClick={() => router.push(href)}
                      className={`flex flex-col gap-2 px-4 py-3 text-left hover:bg-muted/50 focus:bg-muted/60 focus:outline-none focus-visible:ring-2 focus-visible:ring-ring ${
                        idx < items.length - 1
                          ? "border-b border-border"
                          : ""
                      }`}
                      aria-label={`Open audit log filtered by ${c.id}`}
                    >
                      <div className="flex items-center justify-between gap-3">
                        <span className="font-mono text-xs text-foreground">
                          {c.id}
                        </span>
                      </div>
                      <p className="text-sm text-muted-foreground">
                        {c.description}
                      </p>
                      <div className="flex flex-wrap gap-1.5">
                        {c.eventKinds.map((k) => {
                          const family = familyForKind(k);
                          return (
                            <Badge
                              key={k}
                              variant="outline"
                              className={`border font-mono text-[10px] ${FAMILY_CLASSES[family]}`}
                            >
                              {k}
                            </Badge>
                          );
                        })}
                      </div>
                    </button>
                  );
                })}
              </CardContent>
            </Card>
          </section>
        ))}
    </div>
  );
}
