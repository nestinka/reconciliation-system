import { describe, it, expect } from "vitest";
import { render, screen } from "@testing-library/react";
import { ThemeProvider } from "next-themes";
import { ThemeToggle } from "@/components/app/theme-toggle";

function renderWithTheme(ui: React.ReactElement) {
  return render(
    <ThemeProvider attribute="class" defaultTheme="light" enableSystem={false}>
      {ui}
    </ThemeProvider>
  );
}

describe("ThemeToggle", () => {
  it("renders an accessible control with an aria-label", () => {
    renderWithTheme(<ThemeToggle />);
    // Before mount the disabled placeholder appears, after mount the real button
    const button = screen.getByRole("button", { name: /toggle theme/i });
    expect(button).toBeInTheDocument();
  });

  it("does not crash on render", () => {
    expect(() => renderWithTheme(<ThemeToggle />)).not.toThrow();
  });
});
