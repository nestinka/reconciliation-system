import { describe, it, expect, vi } from "vitest";
import React from "react";
import { render, screen } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { ApprovalBar } from "@/components/app/approval-bar";
import type { Case, User } from "@/lib/domain/types";

// A minimal pending_approval Case with approval_requested by user-mia (the maker)
const pendingCase: Case = {
  id: "case-pending",
  breakId: "break-pending",
  assigneeId: "user-mia",
  status: "pending_approval",
  events: [
    {
      id: "evt-1",
      kind: "assignment",
      actorId: "user-ada",
      at: "2026-05-15T11:00:00Z",
      payload: { assigneeId: "user-mia" },
    },
    {
      id: "evt-2",
      kind: "write_off_proposed",
      actorId: "user-mia",
      at: "2026-05-16T09:30:00Z",
      payload: { reason: "Below materiality threshold." },
    },
    {
      id: "evt-3",
      kind: "approval_requested",
      actorId: "user-mia", // maker
      at: "2026-05-16T09:35:00Z",
      payload: { resolution: "write_off" },
    },
  ],
};

const userMia: User = { id: "user-mia", name: "Mia", role: "operator" };
const userTheo: User = { id: "user-theo", name: "Theo", role: "approver" };
const userSam: User = { id: "user-sam", name: "Sam", role: "operator" };

describe("ApprovalBar", () => {
  it("renders nothing when case is not pending_approval", () => {
    const { container } = render(
      <ApprovalBar
        case={{ ...pendingCase, status: "investigating" }}
        currentUser={userMia}
        onApprove={vi.fn()}
        onReject={vi.fn()}
      />
    );
    expect(container.firstChild).toBeNull();
  });

  describe("maker (user-mia, operator) is current user", () => {
    it("Approve button is disabled", () => {
      render(
        <ApprovalBar
          case={pendingCase}
          currentUser={userMia}
          onApprove={vi.fn()}
          onReject={vi.fn()}
        />
      );
      const approveBtn = screen.getByRole("button", { name: /approve/i });
      expect(approveBtn).toBeDisabled();
    });

    it("four-eyes reason text is visible and mentions the maker restriction", () => {
      render(
        <ApprovalBar
          case={pendingCase}
          currentUser={userMia}
          onApprove={vi.fn()}
          onReject={vi.fn()}
        />
      );
      // The helper text (role=note) should be visible explaining why the button is disabled.
      // The text may be split by a sibling icon element, so check textContent of the note element.
      const note = screen.getByRole("note");
      expect(note).toBeInTheDocument();
      expect(note.textContent).toMatch(/you proposed this change/i);
      expect(note.textContent).toMatch(/four-eyes/i);
    });

    it("the disabled reason has role=note (accessible)", () => {
      render(
        <ApprovalBar
          case={pendingCase}
          currentUser={userMia}
          onApprove={vi.fn()}
          onReject={vi.fn()}
        />
      );
      expect(screen.getByRole("note")).toBeInTheDocument();
    });
  });

  describe("valid checker (user-theo, approver) is current user", () => {
    it("Approve button is enabled", () => {
      render(
        <ApprovalBar
          case={pendingCase}
          currentUser={userTheo}
          onApprove={vi.fn()}
          onReject={vi.fn()}
        />
      );
      const approveBtn = screen.getByRole("button", { name: /approve/i });
      expect(approveBtn).not.toBeDisabled();
    });

    it("no four-eyes reason text is shown", () => {
      render(
        <ApprovalBar
          case={pendingCase}
          currentUser={userTheo}
          onApprove={vi.fn()}
          onReject={vi.fn()}
        />
      );
      expect(screen.queryByRole("note")).not.toBeInTheDocument();
    });

    it("clicking Approve calls onApprove", async () => {
      const onApprove = vi.fn();
      const user = userEvent.setup();
      render(
        <ApprovalBar
          case={pendingCase}
          currentUser={userTheo}
          onApprove={onApprove}
          onReject={vi.fn()}
        />
      );
      await user.click(screen.getByRole("button", { name: /approve/i }));
      expect(onApprove).toHaveBeenCalledOnce();
    });

    it("reject flow: shows form, then calls onReject with reason", async () => {
      const onReject = vi.fn();
      const user = userEvent.setup();
      render(
        <ApprovalBar
          case={pendingCase}
          currentUser={userTheo}
          onApprove={vi.fn()}
          onReject={onReject}
        />
      );
      // Click Reject to reveal the form
      await user.click(screen.getByRole("button", { name: /reject/i }));
      // Form should appear
      const reasonField = screen.getByLabelText(/rejection reason/i);
      await user.type(reasonField, "Insufficient evidence");
      await user.click(
        screen.getByRole("button", { name: /confirm rejection/i })
      );
      expect(onReject).toHaveBeenCalledWith("Insufficient evidence");
    });
  });

  describe("operator without approver role (user-sam) is current user", () => {
    it("Approve button is disabled", () => {
      render(
        <ApprovalBar
          case={pendingCase}
          currentUser={userSam}
          onApprove={vi.fn()}
          onReject={vi.fn()}
        />
      );
      const approveBtn = screen.getByRole("button", { name: /approve/i });
      expect(approveBtn).toBeDisabled();
    });

    it("role reason text is shown (mentioning approver/admin)", () => {
      render(
        <ApprovalBar
          case={pendingCase}
          currentUser={userSam}
          onApprove={vi.fn()}
          onReject={vi.fn()}
        />
      );
      const note = screen.getByRole("note");
      expect(note.textContent).toMatch(/only approvers or admins/i);
    });
  });
});
