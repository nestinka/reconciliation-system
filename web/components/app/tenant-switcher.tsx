"use client";

import { Building2, ChevronsUpDown } from "lucide-react";
import {
  DropdownMenu,
  DropdownMenuContent,
  DropdownMenuCheckboxItem,
  DropdownMenuTrigger,
} from "@/components/ui/dropdown-menu";
import { useTenant } from "@/lib/providers/tenant-provider";
import { useTenants } from "@/lib/hooks/use-tenants";

export function TenantSwitcher() {
  const { tenantId, setTenantId } = useTenant();
  const { data: tenants, isLoading } = useTenants();

  const activeTenant = tenants?.find((t) => t.id === tenantId);

  return (
    <DropdownMenu>
      <DropdownMenuTrigger
        disabled={isLoading}
        aria-label="Switch tenant"
        className="flex items-center gap-2 h-8 px-2 text-sm font-medium rounded hover:bg-accent transition-colors disabled:opacity-50 disabled:pointer-events-none focus-visible:outline-none focus-visible:ring-2 focus-visible:ring-ring"
      >
        <Building2 aria-hidden className="size-3.5 shrink-0 text-muted-foreground" />
        <span className="max-w-[140px] truncate">
          {isLoading ? "Loading…" : (activeTenant?.name ?? tenantId)}
        </span>
        <ChevronsUpDown aria-hidden className="size-3 shrink-0 text-muted-foreground ml-1" />
      </DropdownMenuTrigger>
      <DropdownMenuContent align="start" className="w-52">
        {tenants?.map((t) => (
          <DropdownMenuCheckboxItem
            key={t.id}
            checked={t.id === tenantId}
            onCheckedChange={() => setTenantId(t.id)}
          >
            {t.name}
          </DropdownMenuCheckboxItem>
        ))}
      </DropdownMenuContent>
    </DropdownMenu>
  );
}
