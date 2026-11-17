import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import { act, fireEvent, render, screen, waitFor, within } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { toast } from "sonner";
import { resetPlusFixture } from "../fixtures/ipc";
import {
  setsFromIds,
  stdioServer,
  type LoginSeed,
  type WorldServer,
} from "../fixtures/serverWorld";
import { guiParity } from "../guiParity";
import ServerOperations from "./ServerOperations";
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

const COVERS = [
  "servers.secret-set",
  "servers.secret-reveal",
  "servers.secret-remove",
] as const;

const host = { openServer: vi.fn(), openAccessSets: vi.fn(), closeServer: vi.fn() };

const NEW_VALUE = "fresh-demo-value-0001";

const foxtrot = stdioServer("srv-foxtrot", "foxtrot-mail", {
  env: [
    { key: "MAIL_TOKEN", secret: true },
    { key: "MAIL_REGION", secret: false },
  ],
});
const rejected: LoginSeed = {
  state: "needs_reauth",
  reason: "HTTP 401",
  checkedAgo: 100,
  fixedBy: "secret",
};

function setup(
  options: {
    server?: WorldServer;
    vault?: string[];
    logins?: Record<string, LoginSeed>;
    vaultLocked?: boolean;
  } = {},
) {
  const server = options.server ?? foxtrot;
  const sets = setsFromIds(["local"], [server]);
  resetPlusFixture({
    servers: {
      servers: [server],
      sets,
      vault: options.vault ?? ["srv-foxtrot/MAIL_TOKEN"],
      logins: options.logins,
      vaultLocked: options.vaultLocked,
    },
  });
  const registry = registryOf([server], sets);
  getRegistryMock.mockResolvedValue(registry);
  const onRegistryChange = vi.fn();
  render(
    <ServerOperations
      server={entryOf(server)}
      registry={registry}
      host={host}
      onRegistryChange={onRegistryChange}
    />,
  );
  return { onRegistryChange };
}

const secretsRow = async () =>
  (await screen.findByText("Secrets")).closest("[data-operation]") as HTMLElement;
const open = async () => {
  const row = await secretsRow();
  if (!within(row).queryByLabelText("Declared secrets"))
    await userEvent.click(within(row).getAllByRole("button")[0]);
  return row;
};

beforeEach(() => {
  getRegistryMock.mockReset();
  invokeLog.length = 0;
  resetRestartStore();
  resetServerWatch();
  vi.clearAllMocks();
});

afterEach(() => {
  vi.useRealTimers();
});

describe("Secrets row", () => {
  it("test00_every_covered_parity_action_is_built_and_names_this_file", () => {
    for (const id of COVERS) {
      expect(guiParity.actions[id], id).toMatchObject({
        status: "built",
        test: "src/plus/servers/SecretsRow.test.tsx",
      });
    }
  });

  it("test01_servers_secret_reveal_lists_declared_keys_with_their_state_and_no_value", async () => {
    setup();
    const row = await secretsRow();
    expect(
      await within(row).findByText(/MAIL_TOKEN is set · 1 plain setting/),
    ).toBeInTheDocument();
    expect(within(row).getByText("Set")).toBeInTheDocument();
    await open();
    expect(within(row).getByText("MAIL_REGION")).toBeInTheDocument();
    expect(ranArgv()).toContainEqual(["secret", "get", "srv-foxtrot", "MAIL_TOKEN"]);
    expect(ranArgv().flat()).not.toContain("--reveal");
    expect(document.body.textContent).not.toMatch(/fake-mail_token/);
  });

  it("test02_servers_secret_set_names_a_key_the_vault_does_not_hold_and_offers_to_set_it", async () => {
    setup({ vault: [] });
    const row = await secretsRow();
    expect(
      await within(row).findByText("MAIL_TOKEN is not set · 1 plain setting"),
    ).toBeInTheDocument();
    expect(within(row).getByText("1 missing")).toBeInTheDocument();
    expect(within(row).getByRole("button", { name: "Edit secrets…" })).toBeEnabled();
    await open();
    expect(
      within(row).queryByRole("button", { name: "Reveal…" }),
    ).not.toBeInTheDocument();
  });

  it("test03_servers_secret_set_opens_upstreams_dialog_and_saves_through_its_password_field", async () => {
    const { onRegistryChange } = setup();
    const row = await secretsRow();
    await userEvent.click(
      await within(row).findByRole("button", { name: "Edit secrets…" }),
    );
    const dialog = await screen.findByRole("dialog");
    expect(within(dialog).getByText("Secrets for foxtrot-mail")).toBeInTheDocument();
    const field = await within(dialog).findByPlaceholderText("•••••••• (saved)");
    expect(field).toHaveAttribute("type", "password");
    await userEvent.type(field, NEW_VALUE);
    await userEvent.click(within(dialog).getByRole("button", { name: "Save" }));
    await waitFor(() => expect(toast.success).toHaveBeenCalledWith("Saved MAIL_TOKEN"));
    const call = invokeLog.find((entry) => entry.command === "set_secret");
    expect(call!.args).toEqual({
      serverId: "srv-foxtrot",
      key: "MAIL_TOKEN",
      value: NEW_VALUE,
    });
    expect(JSON.stringify(ranArgv())).not.toContain(NEW_VALUE);
    expect(document.body.textContent).not.toContain(NEW_VALUE);
    await waitFor(() => expect(onRegistryChange).toHaveBeenCalled());
    expect(toast.error).not.toHaveBeenCalled();
  });

  it("test04_servers_secret_set_in_the_dialog_gives_a_missing_key_its_first_value", async () => {
    setup({ vault: [] });
    const row = await secretsRow();
    await userEvent.click(
      await within(row).findByRole("button", { name: "Edit secrets…" }),
    );
    const dialog = await screen.findByRole("dialog");
    await userEvent.type(
      await within(dialog).findByPlaceholderText("paste your Mail API key"),
      NEW_VALUE,
    );
    await userEvent.click(within(dialog).getByRole("button", { name: "Save" }));
    await waitFor(() => expect(toast.success).toHaveBeenCalledWith("Saved MAIL_TOKEN"));
    await waitFor(() =>
      expect(within(row).getByText(/MAIL_TOKEN is set/)).toBeInTheDocument(),
    );
  });

  it("test05_servers_secret_set_refuses_a_placeholder_with_the_backends_message", async () => {
    setup();
    const row = await secretsRow();
    await userEvent.click(
      await within(row).findByRole("button", { name: "Edit secrets…" }),
    );
    const dialog = await screen.findByRole("dialog");
    await userEvent.type(
      await within(dialog).findByPlaceholderText("•••••••• (saved)"),
      "<VAULTED>",
    );
    await userEvent.click(within(dialog).getByRole("button", { name: "Save" }));
    await waitFor(() =>
      expect(toast.error).toHaveBeenCalledWith(
        expect.stringContaining("Placeholder values cannot be saved."),
        expect.anything(),
      ),
    );
    expect(toast.success).not.toHaveBeenCalled();
  });

  it("test06_servers_secret_reveal_asks_first_shows_the_value_and_hides_it_after_ten_seconds", async () => {
    vi.useFakeTimers({ shouldAdvanceTime: true });
    const user = userEvent.setup({ advanceTimers: vi.advanceTimersByTime });
    setup();
    await open();
    await user.click(await screen.findByRole("button", { name: "Reveal…" }));
    expect(await screen.findByText("Show MAIL_TOKEN?")).toBeInTheDocument();
    expect(ranArgv().flat()).not.toContain("--reveal");
    await user.click(screen.getByRole("button", { name: "Show for 10 seconds" }));
    const value = await screen.findByRole("group", { name: "MAIL_TOKEN value" });
    expect(within(value).getByText("fake-mail_token-0000-demo")).toBeInTheDocument();
    expect(ranArgv()).toContainEqual([
      "secret",
      "get",
      "srv-foxtrot",
      "MAIL_TOKEN",
      "--reveal",
    ]);
    expect(within(value).getByText(/Hides in \d+ s/)).toBeInTheDocument();
    await act(() => vi.advanceTimersByTimeAsync(10_500));
    expect(screen.queryByText("fake-mail_token-0000-demo")).not.toBeInTheDocument();
  });

  it("test07_servers_secret_reveal_hides_at_once_when_the_window_loses_focus_or_on_hide_now", async () => {
    setup();
    await open();
    const show = async () => {
      await userEvent.click(await screen.findByRole("button", { name: "Reveal…" }));
      await userEvent.click(screen.getByRole("button", { name: "Show for 10 seconds" }));
      await screen.findByText("fake-mail_token-0000-demo");
    };
    await show();
    fireEvent(window, new Event("blur"));
    await waitFor(() =>
      expect(screen.queryByText("fake-mail_token-0000-demo")).not.toBeInTheDocument(),
    );
    await show();
    await userEvent.click(screen.getByRole("button", { name: /Hide now/ }));
    expect(screen.queryByText("fake-mail_token-0000-demo")).not.toBeInTheDocument();
  });

  it("test08_servers_secret_reveal_does_nothing_when_cancelled_and_never_goes_through_the_self_mcp", async () => {
    setup();
    await open();
    await userEvent.click(await screen.findByRole("button", { name: "Reveal…" }));
    await userEvent.click(screen.getByRole("button", { name: "Cancel" }));
    expect(ranArgv().flat()).not.toContain("--reveal");
    expect(
      ranArgv()
        .filter(([verb]) => verb === "mcp")
        .every((argv) => argv[2] === "servers_list_profiles"),
    ).toBe(true);
  });

  it("test09_servers_secret_reveal_is_never_offered_for_an_access_token", async () => {
    const remoteToken = stdioServer("srv-foxtrot", "foxtrot-mail", {
      env: [{ key: "__http_auth__", secret: true }],
    });
    setup({ server: remoteToken, vault: ["srv-foxtrot/__http_auth__"] });
    await open();
    expect(
      await screen.findByRole("button", { name: "Edit secrets…" }),
    ).toBeInTheDocument();
    expect(screen.queryByRole("button", { name: "Reveal…" })).not.toBeInTheDocument();
  });

  it("test10_servers_secret_remove_asks_first_in_upstreams_dialog_and_then_deletes_the_value", async () => {
    const { onRegistryChange } = setup();
    const row = await secretsRow();
    await userEvent.click(
      await within(row).findByRole("button", { name: "Edit secrets…" }),
    );
    const dialog = await screen.findByRole("dialog");
    await userEvent.click(
      await within(dialog).findByRole("button", { name: "Remove MAIL_TOKEN" }),
    );
    expect(await screen.findByText("Remove the Mail API key?")).toBeInTheDocument();
    expect(invokeLog.some((entry) => entry.command === "delete_secret")).toBe(false);
    await userEvent.click(screen.getByRole("button", { name: "Remove" }));
    await waitFor(() => expect(toast.success).toHaveBeenCalledWith("Removed MAIL_TOKEN"));
    expect(invokeLog.find((entry) => entry.command === "delete_secret")!.args).toEqual({
      serverId: "srv-foxtrot",
      key: "MAIL_TOKEN",
    });
    await waitFor(() => expect(onRegistryChange).toHaveBeenCalled());
  });

  it("test11_servers_secret_remove_is_only_offered_for_a_key_the_vault_holds", async () => {
    setup({ vault: [] });
    const row = await secretsRow();
    await userEvent.click(
      await within(row).findByRole("button", { name: "Edit secrets…" }),
    );
    const dialog = await screen.findByRole("dialog");
    await within(dialog).findByPlaceholderText("paste your Mail API key");
    expect(
      within(dialog).queryByRole("button", { name: "Remove MAIL_TOKEN" }),
    ).not.toBeInTheDocument();
  });

  it("test12_a_locked_keychain_says_so_and_offers_to_try_again", async () => {
    setup({ vaultLocked: true });
    const row = await secretsRow();
    expect(
      await within(row).findByText(/The keychain did not answer: the keychain is locked/),
    ).toBeInTheDocument();
    expect(within(row).getByText("Locked")).toBeInTheDocument();
    expect(within(row).getByRole("button", { name: "Try again" })).toBeInTheDocument();
  });

  it("test13_a_login_the_server_rejected_marks_a_row_whose_keys_are_all_set", async () => {
    setup({ logins: { "srv-foxtrot": rejected } });
    const row = await secretsRow();
    expect(
      await within(row).findByText(
        /MAIL_TOKEN is set · 1 plain setting · the server rejected it/,
      ),
    ).toBeInTheDocument();
    expect(within(row).getByText("Set, rejected")).toBeInTheDocument();
  });

  it("test14_a_server_that_declares_no_secret_has_no_secrets_row", async () => {
    setup({
      server: stdioServer("srv-foxtrot", "foxtrot-mail", {
        env: [{ key: "REGION", secret: false }],
      }),
    });
    expect(await screen.findByText("Runtime")).toBeInTheDocument();
    expect(await screen.findByText("Who can reach it")).toBeInTheDocument();
    expect(screen.queryByText("Secrets")).not.toBeInTheDocument();
  });
});
