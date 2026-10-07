import { render, screen } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { describe, expect, it, vi } from "vitest";
import { SecretsDialog } from "./SecretsDialog";
import type { ServerEntry } from "@/lib/types";

vi.mock("@/lib/api", () => ({
  startOauthAttempt: vi.fn(),
  cancelOauthAttempt: vi.fn(),
  authenticateOauth: vi.fn(),
  secretStatus: vi.fn().mockResolvedValue([]),
  hasAuthToken: vi.fn().mockResolvedValue(false),
  hasClientSecret: vi.fn().mockResolvedValue(false),
  probeAuth: vi.fn().mockResolvedValue({
    kind: "discovery_error",
    vendor: null,
    tokenUrl: null,
    instructions: null,
    error:
      "protected-resource metadata describes a different resource (https://a.example/, expected https://b.example/mcp)",
  }),
  setClientCredentials: vi.fn(),
  clearClientCredentials: vi.fn(),
  deleteSecret: vi.fn(),
  setSecret: vi.fn(),
  setAuthToken: vi.fn(),
  clearAuthToken: vi.fn(),
}));
vi.mock("sonner", () => ({ toast: { success: vi.fn() } }));
vi.mock("@/lib/toast", () => ({ toastError: vi.fn() }));
vi.mock("@/lib/openUrl", () => ({ openExternal: vi.fn() }));

const server: ServerEntry = {
  id: "miro",
  name: "Miro",
  transport: "http",
  command: null,
  args: [],
  env: [],
  url: "https://b.example/mcp",
  source: "manual",
};

describe("a server whose OAuth discovery failed", () => {
  it("shows the real reason instead of claiming it needs a pasted token", async () => {
    const user = userEvent.setup();
    render(<SecretsDialog server={server} onSaved={vi.fn()} onChanged={vi.fn()} />);
    await user.click(screen.getByRole("button", { name: /Manage secrets/ }));

    expect(await screen.findByText(/discovery failed/)).toHaveTextContent(
      "different resource (https://a.example/, expected https://b.example/mcp)",
    );
    expect(screen.queryByText(/needs a pasted token/)).not.toBeInTheDocument();
    expect(screen.getByRole("button", { name: "Sign in with browser" })).toBeEnabled();
    expect(screen.getByPlaceholderText("paste access token")).toBeInTheDocument();
  });
});
