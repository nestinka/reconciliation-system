"use client";

import { ChevronDown } from "lucide-react";
import {
  DropdownMenu,
  DropdownMenuContent,
  DropdownMenuGroup,
  DropdownMenuLabel,
  DropdownMenuSeparator,
  DropdownMenuRadioGroup,
  DropdownMenuRadioItem,
  DropdownMenuTrigger,
} from "@/components/ui/dropdown-menu";
import { Avatar, AvatarFallback } from "@/components/ui/avatar";
import { useCurrentUserId } from "@/lib/providers/current-user-provider";
import { useUsers } from "@/lib/hooks/use-tenants";
import type { User } from "@/lib/domain/types";

const ROLE_LABELS: Record<User["role"], string> = {
  operator: "Operator",
  approver: "Approver",
  admin: "Admin",
};

function initials(name: string): string {
  return (
    name
      .split(" ")
      .map((s) => s[0])
      .join("")
      .toUpperCase()
      .slice(0, 2) || "?"
  );
}

export function UserMenu() {
  const { currentUserId, setCurrentUserId } = useCurrentUserId();
  const { data: users } = useUsers();

  const currentUser = users?.find((u) => u.id === currentUserId);

  return (
    <DropdownMenu>
      <DropdownMenuTrigger
        aria-label={`User menu — viewing as ${currentUser?.name ?? currentUserId}`}
        className="flex items-center gap-1.5 rounded px-1.5 py-1 text-sm hover:bg-accent transition-colors focus-visible:outline-none focus-visible:ring-2 focus-visible:ring-ring"
      >
        <Avatar className="size-6">
          <AvatarFallback className="text-[10px] font-medium">
            {currentUser ? initials(currentUser.name) : "?"}
          </AvatarFallback>
        </Avatar>
        <span className="max-w-[80px] truncate text-sm leading-none">
          {currentUser?.name ?? currentUserId}
        </span>
        <ChevronDown aria-hidden className="size-3 text-muted-foreground shrink-0" />
      </DropdownMenuTrigger>

      <DropdownMenuContent align="end" className="w-56">
        <DropdownMenuGroup>
          <DropdownMenuLabel className="font-normal">
            <p className="text-xs text-muted-foreground">Viewing as</p>
          </DropdownMenuLabel>
        </DropdownMenuGroup>
        <DropdownMenuSeparator />
        <DropdownMenuRadioGroup
          value={currentUserId}
          onValueChange={setCurrentUserId}
        >
          {users?.map((user) => (
            <DropdownMenuRadioItem key={user.id} value={user.id}>
              <div className="flex items-center gap-2 w-full">
                <Avatar className="size-5 shrink-0">
                  <AvatarFallback className="text-[9px] font-medium">
                    {initials(user.name)}
                  </AvatarFallback>
                </Avatar>
                <div className="flex flex-col min-w-0">
                  <span className="text-sm leading-tight truncate">{user.name}</span>
                  <span className="text-xs text-muted-foreground leading-tight">
                    {ROLE_LABELS[user.role]}
                  </span>
                </div>
              </div>
            </DropdownMenuRadioItem>
          ))}
        </DropdownMenuRadioGroup>
      </DropdownMenuContent>
    </DropdownMenu>
  );
}
