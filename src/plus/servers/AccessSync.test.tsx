import { useState } from "react";
import { beforeEach, describe, expect, it, vi } from "vitest";
import { render, screen, waitFor, within } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { toast } from "sonner";
import { ThemeProvider } from "@/lib/theme";
import { SettingsView } from "@/components/SettingsView";
import type { Registry } from "@/lib/types";
import { plusFixtureAccess, resetPlusFixture } from "../fixtures/ipc";
import { stdioServer } from "../fixtures/serverWorld";
import { getRegistryMock, invokeLog, registryOf } from "./testkit";

vi.mock("@tauri-apps/api/core", async () => {
  const { plusFixtureIpc } = await import("../fixtures/ipc");
  const { invokeLog } = await import("./testkit");
  return {
    invoke: async (command: string, args: Record<string, unknown>) => {
      invokeLog.push({ command, args });
      return plusFixtureIpc(command, args);
    },
  };
});
vi.mock("@tauri-apps/api/event", () => ({ listen: vi.fn().mockResolvedValue(() => {}) }));
vi.mock("@/lib/api", async (importOriginal) => {
  const actual = await importOriginal<typeof import("@/lib/api")>();
  const { plusFixtureAccess } = await import("../fixtures/ipc");
  const { getRegistryMock } = await import("./testkit");
  return {
    ...actual,
    getRegistry: (...args: unknown[]) => getRegistryMock(...args),
    getBaselineNotice: vi.fn().mockResolvedValue(null),
    listQuarantined: vi.fn().mockResolvedValue([]),
    isAutostartEnabled: vi.fn().mockResolvedValue(false),
    clientsNeedingRestart: vi.fn().mockResolvedValue([]),
    setAccessServer: vi.fn(async (setId: string, serverId: string, included: boolean) => {
      const registry = getRegistryMock() as Registry;
      plusFixtureAccess(registry, { profileId: setId, serverId, included });
      return registry;
    }),
  };
});
vi.mock("sonner", () => ({
  toast: Object.assign(vi.fn(), { success: vi.fn(), error: vi.fn(), message: vi.fn() }),
}));

const servers = [
  stdioServer("srv-alpha", "alpha-api"),
  stdioServer("srv-beta", "beta-crm"),
];
const sets = [
  { id: "local", name: "Local fixture", serverIds: ["srv-alpha", "srv-beta"] },
  { id: "work", name: "Work", serverIds: ["srv-alpha"] },
];

/** What a registry read answers: the memberships the world holds, as the registry file does. */
function currentRegistry(): Registry {
  const registry = registryOf(servers, sets);
  plusFixtureAccess(registry);
  return registry;
}

function Page() {
  const [registry, setRegistry] = useState<Registry>(currentRegistry());
  return (
    <ThemeProvider>
      <SettingsView page="access" registry={registry} onRegistryChange={setRegistry} />
    </ThemeProvider>
  );
}

/** The block upstream draws for one access set: its heading row and its server switches. */
function upstreamSet(name: string) {
  const heading = screen.getByText(name, { selector: "span.truncate" });
  return heading.closest<HTMLElement>(".toolport-list")!;
}

const cell = (server: string, set: string) =>
  screen.getByRole("button", { name: `${server} in ${set}` });

beforeEach(() => {
  invokeLog.length = 0;
  vi.clearAllMocks();
  resetPlusFixture({ servers: { servers, sets } });
  getRegistryMock.mockImplementation(currentRegistry);
});

describe("Settings > Access", () => {
  it("test01_a_change_in_the_compare_table_shows_in_upstreams_access_set_and_so_does_undo", async () => {
    render(<Page />);
    await screen.findByRole("table");
    expect(within(upstreamSet("Work")).getByText(/^1\s+server$/)).toBeInTheDocument();
    expect(
      within(upstreamSet("Work")).getByRole("switch", { name: "alpha-api" }),
    ).toBeChecked();

    await userEvent.click(cell("alpha-api", "Work"));
    await waitFor(() =>
      expect(within(upstreamSet("Work")).getByText(/^0\s+servers$/)).toBeInTheDocument(),
    );
    expect(
      within(upstreamSet("Work")).getByRole("switch", { name: "alpha-api" }),
    ).not.toBeChecked();
    expect(cell("alpha-api", "Work")).toHaveAttribute("aria-pressed", "false");

    const [, options] = vi.mocked(toast.success).mock.calls.at(-1)!;
    (options?.action as unknown as { onClick: () => void }).onClick();
    await waitFor(() =>
      expect(within(upstreamSet("Work")).getByText(/^1\s+server$/)).toBeInTheDocument(),
    );
    expect(
      within(upstreamSet("Work")).getByRole("switch", { name: "alpha-api" }),
    ).toBeChecked();
    expect(cell("alpha-api", "Work")).toHaveAttribute("aria-pressed", "true");
  });

  it("test02_a_switch_in_upstreams_access_set_shows_in_the_compare_table", async () => {
    render(<Page />);
    await screen.findByRole("table");
    expect(cell("beta-crm", "Work")).toHaveAttribute("aria-pressed", "false");

    await userEvent.click(
      within(upstreamSet("Work")).getByRole("switch", { name: "beta-crm" }),
    );
    await waitFor(() =>
      expect(cell("beta-crm", "Work")).toHaveAttribute("aria-pressed", "true"),
    );
    expect(
      screen.getByRole("columnheader", { name: /Work\s*2 servers/ }),
    ).toBeInTheDocument();
  });
});
