import "@testing-library/jest-dom/vitest";
import { afterEach, expect } from "vitest";
import { cleanup } from "@testing-library/react";
import { toHaveNoViolations } from "jest-axe";

// jest-axe exports the matcher as `{ toHaveNoViolations }`, ready for expect.extend.
expect.extend(toHaveNoViolations);

afterEach(() => {
  cleanup();
});
