import { beforeEach, describe, expect, it, vi } from "vitest";
import { render, screen, waitFor, within } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { toast } from "sonner";
import { resetPlusFixture } from "../fixtures/ipc";
import {
  setsFromIds,
  stdioServer,
  type LoginSeed,
  type SignInScript,
  type WorldServer,
} from "../fixtures/serverWorld";
import { guiParity } from "../guiParity";
import { LOGIN_VIEWS, loginFailed, loginNeedsYou } from "./loginModel";
import ServerOperations from "./ServerOperations";
import ServersBanner from "./ServersBanner";
import { resetRestartStore } from "./restartStore";
import { entryOf, getRegistryMock, invokeLog, ranArgv, registryOf } from "./testkit";
import { resetServerWatch } from "./watch";

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
vi.mock("@/lib/api", async (importOriginal) => ({
  ...(await importOriginal<typeof import("@/lib/api")>()),
  getRegistry: (...args: unknown[]) => getRegistryMock(...args),
}));
vi.mock("sonner", () => ({
  toast: Object.assign(vi.fn(), { success: vi.fn(), error: vi.fn(), message: vi.fn() }),
}));

const COVERS = ["servers.login", "servers.login-check"] as const;

const host = { openServer: vi.fn(), openAccessSets: vi.fn(), closeServer: vi.fn() };

const foxtrot = stdioServer("srv-foxtrot", "foxtrot-mail", {
  command: "npx",
  args: ["-y", "@example/mail-mcp"],
});
const remote = stdioServer("srv-delta", "delta-tracker", {
  transport: "http",
  command: null,
  url: "https://example.test/mcp",
});

const rejected: LoginSeed = {
  state: "needs_reauth",
  reason: "HTTP 401 on the last 3 checks",
  checkedAgo: 120,
  sinceAgo: 3 * 86_400,
  fixedBy: "sign-in",
};

function setup(
  options: {
    servers?: WorldServer[];
    logins?: Record<string, LoginSeed>;
    signIn?: Record<string, SignInScript>;
    show?: WorldServer;
  } = {},
) {
  const servers = options.servers ?? [foxtrot];
  const sets = setsFromIds(["local"], servers);
  resetPlusFixture({
    servers: {
      servers,
      sets,
      logins: options.logins ?? { "srv-foxtrot": rejected },
      signIn: options.signIn,
    },
  });
  const registry = registryOf(servers, sets);
  getRegistryMock.mockResolvedValue(registry);
  render(
    <ServerOperations
      server={entryOf(options.show ?? servers[0])}
      registry={registry}
      host={host}
      onRegistryChange={vi.fn()}
    />,
  );
  return { registry, servers };
}

beforeEach(() => {
  getRegistryMock.mockReset();
  invokeLog.length = 0;
  resetRestartStore();
  resetServerWatch();
  vi.clearAllMocks();
});

describe("Login row", () => {
  it("test00_every_covered_parity_action_is_built_and_names_this_file", () => {
    for (const id of COVERS) {
      expect(guiParity.actions[id], id).toMatchObject({
        status: "built",
        test: "src/plus/servers/LoginRow.test.tsx",
      });
    }
  });

  it("test01_servers_login_check_says_what_was_rejected_and_since_when_and_opens_itself", async () => {
    setup();
    const row = await screen.findByText("Login");
    const panel = row.closest("[data-operation]") as HTMLElement;
    expect(within(panel).getByText("Sign-in needed")).toBeInTheDocument();
    expect(within(panel).getAllByText(/HTTP 401 on the last 3 checks/).length).toBe(2);
    expect(within(panel).getByText("Last check")).toBeInTheDocument();
    expect(within(panel).getByText("Since")).toBeInTheDocument();
    expect(
      within(panel).getByText(/still starts, but calls that need this login fail/),
    ).toBeInTheDocument();
    expect(await screen.findByText("1 need you")).toBeInTheDocument();
  });

  it("test02_servers_login_check_of_a_signed_in_server_is_one_quiet_line", async () => {
    setup({
      logins: { "srv-foxtrot": { state: "ok", checkedAgo: 60 } },
    });
    const row = (await screen.findByText("Login")).closest(
      "[data-operation]",
    ) as HTMLElement;
    expect(within(row).getByText("Signed in")).toBeInTheDocument();
    expect(within(row).getByText(/Signed in · checked/)).toBeInTheDocument();
    expect(within(row).queryByText("Last check")).not.toBeInTheDocument();
    expect(screen.queryByText(/need you/)).not.toBeInTheDocument();
  });

  it("test03_servers_login_is_drawn_for_stdio_servers_only_and_remote_ones_are_not_probed", async () => {
    setup({
      servers: [foxtrot, remote],
      logins: { "srv-delta": rejected },
      show: remote,
    });
    expect(await screen.findByText("Runtime")).toBeInTheDocument();
    expect(await screen.findByText("Who can reach it")).toBeInTheDocument();
    expect(screen.queryByText("Login")).not.toBeInTheDocument();
    expect(ranArgv().some(([verb]) => verb === "auth")).toBe(false);
  });

  it("test04_servers_login_check_now_probes_this_server_and_forces_it", async () => {
    setup({ logins: { "srv-foxtrot": { ...rejected, fixedBy: "never" } } });
    await userEvent.click(await screen.findByRole("button", { name: /Check now/ }));
    expect(await screen.findByText("Next check")).toBeInTheDocument();
    expect(ranArgv()).toContainEqual([
      "auth",
      "probe",
      "--server",
      "srv-foxtrot",
      "--force",
    ]);
    expect(screen.getByText("Sign-in needed")).toBeInTheDocument();
  });

  it("test05_servers_login_sign_in_again_shows_the_consent_link_then_turns_green", async () => {
    setup({ signIn: { "srv-foxtrot": { kind: "consent", delayMs: 400 } } });
    await userEvent.click(await screen.findByRole("button", { name: "Sign in again" }));
    expect(ranArgv()).toContainEqual(["auth", "login", "srv-foxtrot"]);
    const group = await screen.findByRole("group", { name: "Signing in" });
    expect(
      await within(group).findByText(
        "https://accounts.example.test/consent?client=fixture",
      ),
    ).toBeInTheDocument();
    expect(within(group).getByRole("button", { name: /Open link/ })).toBeInTheDocument();
    await waitFor(() => expect(screen.getByText("Signed in")).toBeInTheDocument());
    expect(toast.success).toHaveBeenCalledWith("Signed in to foxtrot-mail.");
    expect(screen.queryByRole("group", { name: "Signing in" })).not.toBeInTheDocument();
  });

  it("test06_servers_login_sign_in_again_shows_the_reason_when_the_sign_in_fails", async () => {
    setup({
      signIn: {
        "srv-foxtrot": { kind: "fail", message: "the auth command exited with 2" },
      },
    });
    await userEvent.click(await screen.findByRole("button", { name: "Sign in again" }));
    expect(await screen.findByText("the auth command exited with 2")).toBeInTheDocument();
    expect(screen.getByText("Sign-in needed")).toBeInTheDocument();
  });

  it("test07_servers_login_sign_in_again_can_be_cancelled", async () => {
    setup({ signIn: { "srv-foxtrot": { kind: "consent", delayMs: 300 } } });
    await userEvent.click(await screen.findByRole("button", { name: "Sign in again" }));
    await userEvent.click(await screen.findByRole("button", { name: "Cancel" }));
    expect(invokeLog.some((entry) => entry.command === "plus_ctl_cancel")).toBe(true);
    await waitFor(() =>
      expect(screen.queryByRole("group", { name: "Signing in" })).not.toBeInTheDocument(),
    );
  });

  it("test08_servers_login_misconfigured_says_what_to_check_and_offers_no_sign_in_first", async () => {
    setup({
      logins: {
        "srv-foxtrot": {
          state: "misconfigured",
          reason: "the OAuth client id is missing",
          checkedAgo: 30,
          fixedBy: "never",
        },
      },
    });
    expect(
      await screen.findByText("Check the OAuth client configuration of srv-foxtrot"),
    ).toBeInTheDocument();
    const row = screen.getByText("Login").closest("[data-operation]") as HTMLElement;
    const buttons = within(row).getAllByRole("button");
    expect(buttons.map((b) => b.textContent)).toEqual(
      expect.arrayContaining(["Check now", "Sign in again"]),
    );
    expect(buttons.findIndex((b) => b.textContent === "Check now")).toBeLessThan(
      buttons.findIndex((b) => b.textContent === "Sign in again"),
    );
  });

  it("test09_login_states_split_into_failures_warnings_and_quiet", () => {
    const row = (state: keyof typeof LOGIN_VIEWS) =>
      ({ state, reason: "", server: "x" }) as Parameters<typeof loginFailed>[0];
    expect(
      (["needs_reauth", "revoked", "misconfigured"] as const).map((s) =>
        loginFailed(row(s)),
      ),
    ).toEqual([true, true, true]);
    expect(
      (["expiring", "unreachable", "unknown", "ok"] as const).map((s) =>
        loginFailed(row(s)),
      ),
    ).toEqual([false, false, false, false]);
    expect(loginNeedsYou(row("expiring"))).toBe(true);
    expect(loginNeedsYou(row("unreachable"))).toBe(false);
  });
});

describe("Login banner", () => {
  const alpha = stdioServer("srv-alpha", "alpha-api");
  const golf = stdioServer("srv-golf", "golf-docs");

  function banner(logins: Record<string, LoginSeed>, servers = [alpha, foxtrot, golf]) {
    const sets = setsFromIds(["local"], servers);
    resetPlusFixture({ servers: { servers, sets, logins } });
    const registry = registryOf(servers, sets);
    getRegistryMock.mockResolvedValue(registry);
    render(<ServersBanner registry={registry} host={host} />);
  }

  it("test10_servers_login_check_names_the_servers_whose_check_failed_and_opens_one", async () => {
    banner({
      "srv-alpha": { state: "ok", checkedAgo: 10 },
      "srv-foxtrot": rejected,
      "srv-golf": { state: "revoked", reason: "revoked", checkedAgo: 20 },
    });
    expect(await screen.findByText("2 login checks failed")).toBeInTheDocument();
    await userEvent.click(screen.getByRole("button", { name: "Open golf-docs" }));
    expect(host.openServer).toHaveBeenCalledWith("srv-golf");
    expect(
      screen.queryByRole("button", { name: "Open alpha-api" }),
    ).not.toBeInTheDocument();
  });

  it("test11_servers_login_check_says_nothing_when_logins_are_fine_or_only_expiring", async () => {
    banner({
      "srv-alpha": { state: "ok", checkedAgo: 10 },
      "srv-foxtrot": { state: "expiring", expiresIn: 3600, checkedAgo: 10 },
    });
    await waitFor(() => expect(ranArgv()).toContainEqual(["status"]));
    expect(screen.queryByText(/login check/)).not.toBeInTheDocument();
  });

  it("test12_servers_login_check_again_probes_each_failed_server", async () => {
    banner({
      "srv-foxtrot": { ...rejected, fixedBy: "never" },
      "srv-golf": {
        state: "revoked",
        reason: "revoked",
        checkedAgo: 20,
        fixedBy: "never",
      },
    });
    await userEvent.click(await screen.findByRole("button", { name: /Check again/ }));
    await waitFor(() => {
      const probes = ranArgv().filter(
        ([verb, sub]) => verb === "auth" && sub === "probe",
      );
      expect(probes.map((argv) => argv[3]).sort()).toEqual(["srv-foxtrot", "srv-golf"]);
    });
  });
});
