"use client";

import { Building2, ChevronsUpDown } from "lucide-react";
import {
  DropdownMenu,
  DropdownMenuContent,
  DropdownMenuRadioGroup,
  DropdownMenuRadioItem,
  DropdownMenuTrigger,
} from "@/components/ui/dropdown-menu";
import { useAuth } from "@/lib/auth/auth-provider";

export function TenantSwitcher() {
  const { activeTenant, memberships, switchTenant } = useAuth();

  const tenantId = activeTenant?.id ?? "";

  // If only one membership, render a static label
  if (memberships.length <= 1) {
    return (
      <div className="flex items-center gap-2 h-8 px-2 text-sm font-medium">
        <Building2 aria-hidden className="size-3.5 shrink-0 text-muted-foreground" />
        <span className="max-w-[140px] truncate">
          {activeTenant?.name ?? tenantId}
        </span>
      </div>
    );
  }

  async function handleValueChange(newTenantId: string) {
    if (newTenantId !== tenantId) {
      await switchTenant(newTenantId);
    }
  }

  return (
    <DropdownMenu>
      <DropdownMenuTrigger
        aria-label="Switch tenant"
        className="flex items-center gap-2 h-8 px-2 text-sm font-medium rounded hover:bg-accent transition-colors focus-visible:outline-none focus-visible:ring-2 focus-visible:ring-ring"
      >
        <Building2 aria-hidden className="size-3.5 shrink-0 text-muted-foreground" />
        <span className="max-w-[140px] truncate">
          {activeTenant?.name ?? tenantId}
        </span>
        <ChevronsUpDown aria-hidden className="size-3 shrink-0 text-muted-foreground ml-1" />
      </DropdownMenuTrigger>
      <DropdownMenuContent align="start" className="w-52">
        <DropdownMenuRadioGroup
          value={tenantId}
          onValueChange={(val) => void handleValueChange(val)}
        >
          {memberships.map((m) => (
            <DropdownMenuRadioItem key={m.tenantId} value={m.tenantId}>
              {m.tenantName}
            </DropdownMenuRadioItem>
          ))}
        </DropdownMenuRadioGroup>
      </DropdownMenuContent>
    </DropdownMenu>
  );
}
