import { describe, it, expect, vi } from "vitest";
import { render, screen } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { axe } from "jest-axe";
import { DataTable, type Column } from "./data-table";

interface Row {
  id: string;
  name: string;
  amount: number;
}

const ROWS: Row[] = [
  { id: "r1", name: "Alpha", amount: 300 },
  { id: "r2", name: "Beta", amount: 100 },
  { id: "r3", name: "Gamma", amount: 200 },
];

const COLUMNS: Column<Row>[] = [
  {
    id: "name",
    header: "Name",
    cell: (r) => r.name,
    sortable: true,
    sortValue: (r) => r.name,
  },
  {
    id: "amount",
    header: "Amount",
    cell: (r) => r.amount,
    align: "right",
    sortable: true,
    sortValue: (r) => r.amount,
  },
];

const getRowId = (r: Row) => r.id;

describe("DataTable", () => {
  it("renders all rows via cell renderers", () => {
    render(<DataTable columns={COLUMNS} rows={ROWS} getRowId={getRowId} />);
    expect(screen.getByText("Alpha")).toBeInTheDocument();
    expect(screen.getByText("Beta")).toBeInTheDocument();
    expect(screen.getByText("Gamma")).toBeInTheDocument();
  });

  it("clicking a sortable header reorders rows ascending by sortValue", async () => {
    const user = userEvent.setup();
    render(<DataTable columns={COLUMNS} rows={ROWS} getRowId={getRowId} />);

    await user.click(screen.getByText("Amount"));

    const cells = screen.getAllByRole("cell");
    const amountCells = cells.filter((c) =>
      ["100", "200", "300"].includes(c.textContent ?? "")
    );
    expect(amountCells[0].textContent).toBe("100");
    expect(amountCells[1].textContent).toBe("200");
    expect(amountCells[2].textContent).toBe("300");
  });

  it("clicking a sortable header twice reorders rows descending", async () => {
    const user = userEvent.setup();
    render(<DataTable columns={COLUMNS} rows={ROWS} getRowId={getRowId} />);

    await user.click(screen.getByText("Amount"));
    await user.click(screen.getByText("Amount"));

    const cells = screen.getAllByRole("cell");
    const amountCells = cells.filter((c) =>
      ["100", "200", "300"].includes(c.textContent ?? "")
    );
    expect(amountCells[0].textContent).toBe("300");
    expect(amountCells[1].textContent).toBe("200");
    expect(amountCells[2].textContent).toBe("100");
  });

  it("clicking a sortable header three times resets sort to original order", async () => {
    const user = userEvent.setup();
    // Deliberately NOT alphabetical so "reset" is distinguishable from "ascending".
    const unsorted: Row[] = [
      { id: "r1", name: "Gamma", amount: 200 },
      { id: "r2", name: "Alpha", amount: 300 },
      { id: "r3", name: "Beta", amount: 100 },
    ];
    render(<DataTable columns={COLUMNS} rows={unsorted} getRowId={getRowId} />);

    await user.click(screen.getByText("Name")); // asc
    await user.click(screen.getByText("Name")); // desc
    await user.click(screen.getByText("Name")); // reset

    // Back to the prop order, NOT ascending (which would be Alpha, Beta, Gamma).
    const rows = screen.getAllByRole("row").slice(1); // skip header
    expect(rows[0]).toHaveTextContent("Gamma");
    expect(rows[1]).toHaveTextContent("Alpha");
    expect(rows[2]).toHaveTextContent("Beta");
  });

  it("sortable headers are operable by keyboard", async () => {
    const user = userEvent.setup();
    render(<DataTable columns={COLUMNS} rows={ROWS} getRowId={getRowId} />);

    // Sortable headers expose a real <button> for keyboard operability.
    const amountButton = screen.getByRole("button", { name: /Amount/ });
    amountButton.focus();
    expect(amountButton).toHaveFocus();
    await user.keyboard("{Enter}"); // ascending

    const cells = screen
      .getAllByRole("cell")
      .filter((c) => ["100", "200", "300"].includes(c.textContent ?? ""));
    expect(cells[0].textContent).toBe("100");
    expect(cells[2].textContent).toBe("300");
  });

  it("isLoading shows skeletons, not row data", () => {
    render(
      <DataTable columns={COLUMNS} rows={ROWS} getRowId={getRowId} isLoading skeletonRows={4} />
    );
    expect(screen.queryByText("Alpha")).not.toBeInTheDocument();
    // skeletons are rendered as divs with animate-pulse
    const skeletons = document.querySelectorAll("[data-slot='skeleton']");
    // 4 rows x 2 columns = 8 skeletons
    expect(skeletons.length).toBe(8);
  });

  it("empty rows shows the default empty state", () => {
    render(<DataTable columns={COLUMNS} rows={[]} getRowId={getRowId} />);
    expect(screen.getByText("No results")).toBeInTheDocument();
  });

  it("empty rows shows a custom empty state when provided", () => {
    render(
      <DataTable
        columns={COLUMNS}
        rows={[]}
        getRowId={getRowId}
        emptyState={<p>Custom empty message</p>}
      />
    );
    expect(screen.getByText("Custom empty message")).toBeInTheDocument();
  });

  describe("selection", () => {
    it("clicking a row checkbox calls onSelectionChange with that id", async () => {
      const user = userEvent.setup();
      const onChange = vi.fn();
      render(
        <DataTable
          columns={COLUMNS}
          rows={ROWS}
          getRowId={getRowId}
          selectable
          selectedIds={[]}
          onSelectionChange={onChange}
        />
      );

      await user.click(screen.getByRole("checkbox", { name: "Select row r1" }));

      expect(onChange).toHaveBeenCalledWith(["r1"]);
    });

    it("header checkbox selects all rows", async () => {
      const user = userEvent.setup();
      const onChange = vi.fn();
      render(
        <DataTable
          columns={COLUMNS}
          rows={ROWS}
          getRowId={getRowId}
          selectable
          selectedIds={[]}
          onSelectionChange={onChange}
        />
      );

      await user.click(screen.getByRole("checkbox", { name: "Select all rows" }));

      expect(onChange).toHaveBeenCalledWith(expect.arrayContaining(["r1", "r2", "r3"]));
      expect(onChange.mock.calls[0][0]).toHaveLength(3);
    });

    it("header checkbox clears all when all are selected", async () => {
      const user = userEvent.setup();
      const onChange = vi.fn();
      render(
        <DataTable
          columns={COLUMNS}
          rows={ROWS}
          getRowId={getRowId}
          selectable
          selectedIds={["r1", "r2", "r3"]}
          onSelectionChange={onChange}
        />
      );

      await user.click(screen.getByRole("checkbox", { name: "Select all rows" }));

      expect(onChange).toHaveBeenCalledWith([]);
    });

    it("header checkbox is indeterminate when only some rows are selected", () => {
      render(
        <DataTable
          columns={COLUMNS}
          rows={ROWS}
          getRowId={getRowId}
          selectable
          selectedIds={["r1"]}
          onSelectionChange={vi.fn()}
        />
      );

      const header = screen.getByRole("checkbox", { name: "Select all rows" });
      expect(header).toHaveAttribute("aria-checked", "mixed");
    });

    it("clicking a row checkbox does not trigger onRowClick (propagation stopped)", async () => {
      const user = userEvent.setup();
      const onChange = vi.fn();
      const onRowClick = vi.fn();
      render(
        <DataTable
          columns={COLUMNS}
          rows={ROWS}
          getRowId={getRowId}
          onRowClick={onRowClick}
          selectable
          selectedIds={[]}
          onSelectionChange={onChange}
        />
      );

      await user.click(screen.getByRole("checkbox", { name: "Select row r1" }));

      expect(onChange).toHaveBeenCalledWith(["r1"]);
      expect(onRowClick).not.toHaveBeenCalled();
    });
  });

  describe("row click", () => {
    it("onRowClick fires when a row is clicked", async () => {
      const user = userEvent.setup();
      const onClick = vi.fn();
      render(
        <DataTable
          columns={COLUMNS}
          rows={ROWS}
          getRowId={getRowId}
          onRowClick={onClick}
        />
      );

      await user.click(screen.getByText("Alpha"));
      expect(onClick).toHaveBeenCalledWith(ROWS[0]);
    });

    it("onRowClick fires on Enter keydown when row is focused", async () => {
      const user = userEvent.setup();
      const onClick = vi.fn();
      render(
        <DataTable
          columns={COLUMNS}
          rows={ROWS}
          getRowId={getRowId}
          onRowClick={onClick}
        />
      );

      const dataRows = screen.getAllByRole("row").slice(1);
      dataRows[0].focus();
      await user.keyboard("{Enter}");
      expect(onClick).toHaveBeenCalledWith(ROWS[0]);
    });

    it("onRowClick fires on Space keydown when row is focused", async () => {
      const user = userEvent.setup();
      const onClick = vi.fn();
      render(
        <DataTable
          columns={COLUMNS}
          rows={ROWS}
          getRowId={getRowId}
          onRowClick={onClick}
        />
      );

      const dataRows = screen.getAllByRole("row").slice(1);
      dataRows[1].focus();
      await user.keyboard(" ");
      expect(onClick).toHaveBeenCalledWith(ROWS[1]);
    });
  });

  it("passes axe accessibility check on a populated table", async () => {
    const { container } = render(
      <DataTable columns={COLUMNS} rows={ROWS} getRowId={getRowId} />
    );
    const results = await axe(container);
    expect(results).toHaveNoViolations();
  });
});
