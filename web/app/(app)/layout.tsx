import type { ReactNode } from "react";
import { AppSidebar } from "@/components/app/app-sidebar";
import { TenantSwitcher } from "@/components/app/tenant-switcher";
import { ThemeToggle } from "@/components/app/theme-toggle";
import { UserMenu } from "@/components/app/user-menu";

export default function AppLayout({ children }: { children: ReactNode }) {
  return (
    <div className="flex h-full min-h-screen">
      {/* Fixed left sidebar */}
      <aside className="fixed inset-y-0 left-0 z-30 flex w-56 flex-col bg-sidebar border-r border-sidebar-border">
        <AppSidebar />
      </aside>

      {/* Main content area, offset by sidebar width */}
      <div className="flex flex-1 flex-col pl-56">
        {/* Top bar */}
        <header className="sticky top-0 z-20 flex h-12 items-center justify-between border-b border-border bg-background px-4 gap-3">
          <TenantSwitcher />
          <div className="flex items-center gap-2">
            <ThemeToggle />
            <UserMenu />
          </div>
        </header>

        {/* Scrollable content */}
        <main className="flex-1 overflow-auto">
          <div className="mx-auto max-w-6xl px-6 py-6">
            {children}
          </div>
        </main>
      </div>
    </div>
  );
}
