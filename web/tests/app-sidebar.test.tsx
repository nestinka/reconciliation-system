import { describe, it, expect, vi } from "vitest";
import { renderWithProviders, screen } from "./test-utils";
import { AppSidebar } from "@/components/app/app-sidebar";

vi.mock("next/navigation", () => ({
  usePathname: vi.fn(() => "/dashboard"),
}));

vi.mock("next/link", () => ({
  default: ({ href, children, ...props }: React.ComponentProps<"a"> & { href: string }) => (
    <a href={href} {...props}>
      {children}
    </a>
  ),
}));

describe("AppSidebar", () => {
  it("renders the base three nav links for non-admin users", () => {
    renderWithProviders(<AppSidebar />);
    expect(screen.getByRole("link", { name: /dashboard/i })).toBeInTheDocument();
    expect(screen.getByRole("link", { name: /runs/i })).toBeInTheDocument();
    expect(screen.getByRole("link", { name: /exceptions/i })).toBeInTheDocument();
  });

  it("does NOT show Users link for non-admin users", () => {
    renderWithProviders(<AppSidebar />);
    expect(screen.queryByRole("link", { name: /^users$/i })).not.toBeInTheDocument();
  });

  it("shows Users link for admin users", () => {
    renderWithProviders(<AppSidebar />, { currentUserId: "user-ada" });
    expect(screen.getByRole("link", { name: /^users$/i })).toBeInTheDocument();
  });

  it("active link (/dashboard) has aria-current=page", () => {
    renderWithProviders(<AppSidebar />);
    const dashboardLink = screen.getByRole("link", { name: /dashboard/i });
    expect(dashboardLink).toHaveAttribute("aria-current", "page");
  });

  it("inactive links do not have aria-current", () => {
    renderWithProviders(<AppSidebar />);
    const runsLink = screen.getByRole("link", { name: /runs/i });
    const exceptionsLink = screen.getByRole("link", { name: /exceptions/i });
    expect(runsLink).not.toHaveAttribute("aria-current");
    expect(exceptionsLink).not.toHaveAttribute("aria-current");
  });

  it("marks /runs as active when pathname is /runs", async () => {
    const { usePathname } = await import("next/navigation");
    vi.mocked(usePathname).mockReturnValue("/runs");

    renderWithProviders(<AppSidebar />);
    const runsLink = screen.getByRole("link", { name: /runs/i });
    expect(runsLink).toHaveAttribute("aria-current", "page");

    const dashboardLink = screen.getByRole("link", { name: /dashboard/i });
    expect(dashboardLink).not.toHaveAttribute("aria-current");
  });

  it("marks a sub-path as active when pathname starts with its href", async () => {
    const { usePathname } = await import("next/navigation");
    vi.mocked(usePathname).mockReturnValue("/exceptions/case-001");

    renderWithProviders(<AppSidebar />);
    const exceptionsLink = screen.getByRole("link", { name: /exceptions/i });
    expect(exceptionsLink).toHaveAttribute("aria-current", "page");
  });

  it("renders a navigation landmark", () => {
    renderWithProviders(<AppSidebar />);
    expect(screen.getByRole("navigation", { name: /main navigation/i })).toBeInTheDocument();
  });

  it("renders the product wordmark", () => {
    renderWithProviders(<AppSidebar />);
    expect(screen.getByText("Recon")).toBeInTheDocument();
  });
});
