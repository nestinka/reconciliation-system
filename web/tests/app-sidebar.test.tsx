import { describe, it, expect, vi } from "vitest";
import { render, screen } from "@testing-library/react";
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
  it("renders all three nav links", () => {
    render(<AppSidebar />);
    expect(screen.getByRole("link", { name: /dashboard/i })).toBeInTheDocument();
    expect(screen.getByRole("link", { name: /runs/i })).toBeInTheDocument();
    expect(screen.getByRole("link", { name: /exceptions/i })).toBeInTheDocument();
  });

  it("active link (/dashboard) has aria-current=page", () => {
    render(<AppSidebar />);
    const dashboardLink = screen.getByRole("link", { name: /dashboard/i });
    expect(dashboardLink).toHaveAttribute("aria-current", "page");
  });

  it("inactive links do not have aria-current", () => {
    render(<AppSidebar />);
    const runsLink = screen.getByRole("link", { name: /runs/i });
    const exceptionsLink = screen.getByRole("link", { name: /exceptions/i });
    expect(runsLink).not.toHaveAttribute("aria-current");
    expect(exceptionsLink).not.toHaveAttribute("aria-current");
  });

  it("marks /runs as active when pathname is /runs", async () => {
    const { usePathname } = await import("next/navigation");
    vi.mocked(usePathname).mockReturnValue("/runs");

    render(<AppSidebar />);
    const runsLink = screen.getByRole("link", { name: /runs/i });
    expect(runsLink).toHaveAttribute("aria-current", "page");

    const dashboardLink = screen.getByRole("link", { name: /dashboard/i });
    expect(dashboardLink).not.toHaveAttribute("aria-current");
  });

  it("marks a sub-path as active when pathname starts with its href", async () => {
    const { usePathname } = await import("next/navigation");
    vi.mocked(usePathname).mockReturnValue("/exceptions/case-001");

    render(<AppSidebar />);
    const exceptionsLink = screen.getByRole("link", { name: /exceptions/i });
    expect(exceptionsLink).toHaveAttribute("aria-current", "page");
  });

  it("renders a navigation landmark", () => {
    render(<AppSidebar />);
    expect(screen.getByRole("navigation", { name: /main navigation/i })).toBeInTheDocument();
  });

  it("renders the product wordmark", () => {
    render(<AppSidebar />);
    expect(screen.getByText("Recon")).toBeInTheDocument();
  });
});
