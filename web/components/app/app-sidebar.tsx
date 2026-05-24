"use client";

import Link from "next/link";
import { usePathname } from "next/navigation";
import { LayoutDashboard, ListChecks, TriangleAlert, Scale, Users, type LucideIcon } from "lucide-react";
import { cn } from "@/lib/utils";
import { useAuth } from "@/lib/auth/auth-provider";

interface NavItem {
  href: string;
  label: string;
  icon: LucideIcon;
  adminOnly?: boolean;
}

const NAV_ITEMS: NavItem[] = [
  { href: "/dashboard", label: "Dashboard", icon: LayoutDashboard },
  { href: "/runs", label: "Runs", icon: ListChecks },
  { href: "/exceptions", label: "Exceptions", icon: TriangleAlert },
  { href: "/users", label: "Users", icon: Users, adminOnly: true },
];

export function AppSidebar() {
  const pathname = usePathname();
  const { user } = useAuth();
  const isAdmin = user?.role === "admin";

  return (
    <div className="flex h-full flex-col text-sidebar-foreground">
      {/* Wordmark */}
      <div className="flex h-12 items-center gap-2 px-4 border-b border-sidebar-border shrink-0">
        <Scale aria-hidden className="size-4 text-sidebar-primary shrink-0" />
        <span className="text-sm font-semibold tracking-tight">Recon</span>
      </div>

      {/* Navigation */}
      <nav aria-label="Main navigation" className="flex flex-col gap-0.5 p-2 flex-1">
        {NAV_ITEMS.filter((item) => !item.adminOnly || isAdmin).map(({ href, label, icon: Icon }) => {
          const isActive = pathname === href || pathname.startsWith(href + "/");
          return (
            <Link
              key={href}
              href={href}
              aria-current={isActive ? "page" : undefined}
              className={cn(
                "flex items-center gap-2.5 rounded px-2.5 py-1.5 text-sm transition-colors focus-visible:outline-none focus-visible:ring-2 focus-visible:ring-sidebar-ring",
                isActive
                  ? "bg-sidebar-accent text-sidebar-accent-foreground font-medium"
                  : "text-sidebar-foreground/60 hover:bg-sidebar-accent/50 hover:text-sidebar-foreground"
              )}
            >
              <Icon aria-hidden className="size-4 shrink-0" />
              {label}
            </Link>
          );
        })}
      </nav>
    </div>
  );
}
