import { fireEvent, render, screen, waitFor } from "@testing-library/react";
import { beforeEach, describe, expect, it, vi } from "vitest";

import { AccessTab } from "./AccessTab";
import {
  createAuthToken,
  listAuthMemberships,
  listAuthPrincipals,
  listAuthTokens,
} from "../../api";

vi.mock("../../api", () => ({
  createAuthToken: vi.fn(),
  grantAuthMembership: vi.fn(),
  listAuthMemberships: vi.fn(),
  listAuthPrincipals: vi.fn(),
  listAuthTokens: vi.fn(),
  revokeAuthMembership: vi.fn(),
  revokeAuthToken: vi.fn(),
}));

const principal = {
  id: "00000000-0000-0000-0000-000000000001",
  kind: "human_oidc" as const,
  display_name: "Memory Admin",
  email: "admin@example.test",
  groups: ["memory-admins"],
  global_role: "admin" as const,
  projects: [],
};

describe("AccessTab", () => {
  beforeEach(() => {
    vi.mocked(listAuthPrincipals).mockResolvedValue([principal]);
    vi.mocked(listAuthTokens).mockResolvedValue([]);
    vi.mocked(listAuthMemberships).mockResolvedValue([]);
  });

  it("loads access inventory and shows a newly issued token only in the result panel", async () => {
    vi.mocked(createAuthToken).mockResolvedValue({
      id: "00000000-0000-0000-0000-000000000002",
      principal_id: "00000000-0000-0000-0000-000000000003",
      name: "hermes",
      token_prefix: "mlt_example",
      created_at: "2026-08-18T12:00:00Z",
      token: "mlt_one_time_secret",
      projects: [{ project: "memory", role: "reader", source: "explicit" }],
    });

    render(<AccessTab project="memory" />);
    expect((await screen.findAllByText("Memory Admin")).length).toBeGreaterThan(0);

    const nameInput = screen.getByLabelText("Name");
    fireEvent.change(nameInput, { target: { value: "hermes" } });
    fireEvent.click(screen.getByRole("button", { name: "Create" }));

    await waitFor(() => expect(createAuthToken).toHaveBeenCalledWith({
      name: "hermes",
      project: "memory",
      role: "reader",
    }));
    expect(await screen.findByText("mlt_one_time_secret")).toBeInTheDocument();
    expect(screen.getByText(/shown once/i)).toBeInTheDocument();
  });
});
