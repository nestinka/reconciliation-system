import { describe, it, expect, vi, beforeEach } from "vitest";
import { axe } from "jest-axe";
import { renderWithProviders, screen, waitFor, userEvent } from "./test-utils";
import CaseDetailPage from "@/app/(app)/cases/[caseId]/page";

// Mock next/navigation — useParams returns case-pending; useRouter provides push
const mockPush = vi.fn();
vi.mock("next/navigation", () => ({
  useParams: () => ({ caseId: "case-pending" }),
  useRouter: () => ({ push: mockPush }),
}));

// case-pending fixture summary:
//   status: "pending_approval"
//   assigneeId: "user-mia"
//   break: "break-pending" (txnIds: ["txn-brk001"], runId: "run-acme-006")
//   events include approval_requested by user-mia (maker)
//   suggestions: sug-pending-001 (txn-brk001 ↔ txn-b008), sug-pending-002 (txn-brk001 ↔ txn-b006)

describe("CaseDetailPage", () => {
  beforeEach(() => {
    mockPush.mockClear();
  });

  it("renders the break id in the page header after load", async () => {
    renderWithProviders(<CaseDetailPage />, {
      tenantId: "tenant-acme",
    });
    await waitFor(() => {
      expect(
        screen.getByRole("heading", { name: /investigate break-pending/i })
      ).toBeInTheDocument();
    });
  });

  it("renders the break context: value and type", async () => {
    renderWithProviders(<CaseDetailPage />, {
      tenantId: "tenant-acme",
    });
    await waitFor(() => {
      // Break value: 125000 GBP minor → £1,250.00
      // May appear multiple times (break summary + transactions section)
      expect(screen.getAllByText("£1,250.00").length).toBeGreaterThan(0);
      // Break type appears in the break context
      expect(screen.getAllByText(/unmatched/i).length).toBeGreaterThan(0);
    });
  });

  it("renders the activity timeline with at least one event", async () => {
    renderWithProviders(<CaseDetailPage />, {
      tenantId: "tenant-acme",
    });
    await waitFor(() => {
      // "Case activity timeline" is the aria-label on the <ol>
      expect(
        screen.getByRole("list", { name: /activity timeline/i })
      ).toBeInTheDocument();
    });
    // The fixture has an event by Ada (assignment) and by Mia (comment, write_off_proposed, approval_requested)
    // Verify at least one actor name appears
    const timeline = screen.getByRole("list", { name: /activity timeline/i });
    expect(timeline.children.length).toBeGreaterThan(0);
  });

  it("renders actor names from the timeline", async () => {
    renderWithProviders(<CaseDetailPage />, {
      tenantId: "tenant-acme",
    });
    await waitFor(() => {
      // Ada assigned Mia; Mia added a comment, wrote off, requested approval
      // At least "Ada" and "Mia" should appear as actor names in the timeline
      expect(screen.getAllByText("Ada").length).toBeGreaterThan(0);
      expect(screen.getAllByText("Mia").length).toBeGreaterThan(0);
    });
  });

  // Fix 2: non-admin (operator) session should still resolve member names via listMembers.
  it("renders actor and assignee names for a non-admin (operator) session", async () => {
    // user-mia is an operator — was previously broken because the page called listUsers (admin-only)
    renderWithProviders(<CaseDetailPage />, {
      tenantId: "tenant-acme",
      currentUserId: "user-mia",
    });
    await waitFor(() => {
      // Actor names from the timeline should still resolve via listMembers
      expect(screen.getAllByText("Ada").length).toBeGreaterThan(0);
      expect(screen.getAllByText("Mia").length).toBeGreaterThan(0);
    });
    // The assignee dropdown should list member names
    const assigneeLabel = screen.getByLabelText(/assign case to a team member/i);
    expect(assigneeLabel).toBeInTheDocument();
  });

  describe("when current user = user-mia (the maker)", () => {
    it("renders the ApprovalBar with Approve disabled (four-eyes)", async () => {
      renderWithProviders(<CaseDetailPage />, {
        tenantId: "tenant-acme",
        currentUserId: "user-mia",
      });
      await waitFor(() => {
        expect(
          screen.getByRole("region", { name: /four-eyes approval/i })
        ).toBeInTheDocument();
      });
      const approveBtn = screen.getByRole("button", { name: /approve/i });
      expect(approveBtn).toBeDisabled();
      // The four-eyes reason text must be visible (may be split across elements)
      const note = screen.getByRole("note");
      expect(note.textContent).toMatch(/you proposed this change/i);
    });
  });

  describe("when current user = user-theo (approver, different from maker)", () => {
    it("renders the ApprovalBar with Approve enabled", async () => {
      renderWithProviders(<CaseDetailPage />, {
        tenantId: "tenant-acme",
        currentUserId: "user-theo",
      });
      await waitFor(() => {
        const approveBtn = screen.getByRole("button", { name: /approve/i });
        expect(approveBtn).not.toBeDisabled();
      });
    });

    it("clicking Approve calls the mutation and the timeline updates (case resolves)", async () => {
      const user = userEvent.setup();
      renderWithProviders(<CaseDetailPage />, {
        tenantId: "tenant-acme",
        currentUserId: "user-theo",
      });

      // Wait for the page to load with the approval bar
      await waitFor(() => {
        expect(screen.getByRole("button", { name: /approve/i })).toBeInTheDocument();
      });

      await user.click(screen.getByRole("button", { name: /approve/i }));

      // After approval the case becomes resolved:
      // - Status pill should show "Resolved"
      // - Timeline should now include the "approved the resolution" entry
      await waitFor(() => {
        expect(screen.getByText(/resolved/i)).toBeInTheDocument();
      });

      // The ApprovalBar should no longer render (case is no longer pending_approval)
      await waitFor(() => {
        expect(
          screen.queryByRole("region", { name: /four-eyes approval/i })
        ).not.toBeInTheDocument();
      });
    });
  });

  it("adding a comment appends it to the timeline", async () => {
    const user = userEvent.setup();
    renderWithProviders(<CaseDetailPage />, {
      tenantId: "tenant-acme",
      currentUserId: "user-mia",
    });

    await waitFor(() => {
      expect(screen.getByLabelText(/add a comment/i)).toBeInTheDocument();
    });

    const textarea = screen.getByLabelText(/add a comment/i);
    await user.type(textarea, "Looking into this further");

    const addBtn = screen.getByRole("button", { name: /add comment/i });
    await user.click(addBtn);

    // Textarea should be cleared after submission
    await waitFor(() => {
      expect(textarea).toHaveValue("");
    });

    // The comment text should now appear in the timeline
    // Use a flexible text matcher in case the text is split across child elements
    await waitFor(
      () => {
        // Try direct text match first; fall back to checking textContent of the list
        const timeline = screen.getByRole("list", { name: /activity timeline/i });
        expect(timeline.textContent).toContain("Looking into this further");
      },
      { timeout: 3000 }
    );
  });

  it("has no critical a11y violations after load (as Mia)", async () => {
    const { container } = renderWithProviders(<CaseDetailPage />, {
      tenantId: "tenant-acme",
      currentUserId: "user-mia",
    });
    await waitFor(() => {
      expect(
        screen.getByRole("heading", { name: /investigate break-pending/i })
      ).toBeInTheDocument();
    });
    const results = await axe(container, {
      runOnly: { type: "tag", values: ["wcag2a", "wcag2aa"] },
    });
    expect(results).toHaveNoViolations();
  });

  it("shows a loading skeleton while data is fetching", () => {
    renderWithProviders(<CaseDetailPage />, {
      tenantId: "tenant-acme",
    });
    // aria-busy container from the skeleton
    expect(document.querySelector("[aria-busy='true']")).toBeTruthy();
    // The heading should not be present during loading
    expect(
      screen.queryByRole("heading", { name: /investigate/i })
    ).not.toBeInTheDocument();
  });

  it("displays counterpartyBic and counterpartyAccount when present on a transaction", async () => {
    // case-pending → break-pending → txn-brk001 which has counterpartyBic: "CHASGB2L"
    // and counterpartyAccount: "GB29NWBK60161331926819"
    renderWithProviders(<CaseDetailPage />, {
      tenantId: "tenant-acme",
    });
    await waitFor(() => {
      expect(screen.getByText("CHASGB2L")).toBeInTheDocument();
      expect(screen.getByText("GB29NWBK60161331926819")).toBeInTheDocument();
    });
  });
});
