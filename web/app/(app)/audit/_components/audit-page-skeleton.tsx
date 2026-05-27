"use client";

import { Card, CardContent } from "@/components/ui/card";
import { Skeleton } from "@/components/ui/skeleton";

export function AuditPageSkeleton() {
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
