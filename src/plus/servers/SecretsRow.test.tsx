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
import { resetRemovalTier } from "./commands";
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
  resetRemovalTier();
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
    expect(within(row).getByRole("button", { name: "Set…" })).toBeInTheDocument();
    expect(
      within(row).queryByRole("button", { name: "Reveal…" }),
    ).not.toBeInTheDocument();
    expect(
      within(row).queryByRole("button", { name: "Remove…" }),
    ).not.toBeInTheDocument();
  });

  it("test03_servers_secret_set_sends_the_value_on_stdin_only_through_a_password_field", async () => {
    setup();
    await open();
    await userEvent.click(await screen.findByRole("button", { name: "Replace…" }));
    const field = screen.getByLabelText("New value");
    expect(field).toHaveAttribute("type", "password");
    await userEvent.type(field, NEW_VALUE);
    await userEvent.click(screen.getByRole("button", { name: "Replace" }));
    expect(
      await screen.findByText("Saved MAIL_TOKEN for foxtrot-mail."),
    ).toBeInTheDocument();
    expect(ranArgv()).toContainEqual(["secret", "set", "srv-foxtrot", "MAIL_TOKEN"]);
    const run = invokeLog.find(
      (entry) =>
        entry.command === "plus_ctl" &&
        (entry.args as { argv: string[] }).argv[1] === "set",
    );
    expect((run!.args as { stdinSecret: string }).stdinSecret).toBe(NEW_VALUE);
    expect(JSON.stringify(ranArgv())).not.toContain(NEW_VALUE);
    expect(document.body.textContent).not.toContain(NEW_VALUE);
    expect(toast.error).not.toHaveBeenCalled();
  });

  it("test04_servers_secret_set_refuses_a_placeholder_with_the_backends_message", async () => {
    setup();
    await open();
    await userEvent.click(await screen.findByRole("button", { name: "Replace…" }));
    await userEvent.type(screen.getByLabelText("New value"), "<VAULTED>");
    await userEvent.click(screen.getByRole("button", { name: "Replace" }));
    expect(await screen.findByRole("alert")).toHaveTextContent(
      "Placeholder values cannot be saved.",
    );
    expect(screen.queryByText(/Saved MAIL_TOKEN/)).not.toBeInTheDocument();
  });

  it("test05_servers_secret_set_checks_the_login_right_after_saving_and_says_how_it_went", async () => {
    setup({ logins: { "srv-foxtrot": rejected } });
    await open();
    await userEvent.click(await screen.findByRole("button", { name: "Replace…" }));
    await userEvent.type(screen.getByLabelText("New value"), NEW_VALUE);
    await userEvent.click(screen.getByRole("button", { name: "Replace" }));
    expect(
      await screen.findByText(/got a normal answer: foxtrot-mail is signed in\./),
    ).toBeInTheDocument();
    expect(ranArgv()).toContainEqual([
      "auth",
      "probe",
      "--server",
      "srv-foxtrot",
      "--force",
    ]);
    await userEvent.click(
      screen
        .getAllByRole("button", { name: "Close" })
        .find((button) => button.textContent === "Close")!,
    );
    await waitFor(() => expect(screen.getByText("Signed in")).toBeInTheDocument());
  });

  it("test06_servers_secret_set_skips_the_check_when_it_is_unticked", async () => {
    setup({ logins: { "srv-foxtrot": rejected } });
    await open();
    await userEvent.click(await screen.findByRole("button", { name: "Replace…" }));
    await userEvent.click(screen.getByLabelText("Check the login right after saving"));
    await userEvent.type(screen.getByLabelText("New value"), NEW_VALUE);
    await userEvent.click(screen.getByRole("button", { name: "Replace" }));
    await screen.findByText("Saved MAIL_TOKEN for foxtrot-mail.");
    expect(ranArgv().some(([verb, sub]) => verb === "auth" && sub === "probe")).toBe(
      false,
    );
  });

  it("test07_servers_secret_reveal_asks_first_shows_the_value_and_hides_it_after_ten_seconds", async () => {
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

  it("test08_servers_secret_reveal_hides_at_once_when_the_window_loses_focus_or_on_hide_now", async () => {
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

  it("test09_servers_secret_reveal_does_nothing_when_cancelled_and_never_goes_through_the_self_mcp", async () => {
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

  it("test10_servers_secret_reveal_is_never_offered_for_an_access_token", async () => {
    const remoteToken = stdioServer("srv-foxtrot", "foxtrot-mail", {
      env: [{ key: "__http_auth__", secret: true }],
    });
    setup({ server: remoteToken, vault: ["srv-foxtrot/__http_auth__"] });
    await open();
    expect(await screen.findByRole("button", { name: "Replace…" })).toBeInTheDocument();
    expect(screen.queryByRole("button", { name: "Reveal…" })).not.toBeInTheDocument();
  });

  it("test11_servers_secret_remove_lists_what_goes_and_wants_the_key_typed_for_a_destructive_tier", async () => {
    const { onRegistryChange } = setup();
    await open();
    await userEvent.click(
      within(await secretsRow()).getByRole("button", { name: "Remove…" }),
    );
    const dialog = await screen.findByRole("dialog");
    expect(within(dialog).getByText("Remove MAIL_TOKEN?")).toBeInTheDocument();
    expect(
      within(dialog).getByText(/The value in the keychain; it cannot be shown again/),
    ).toBeInTheDocument();
    const confirm = within(dialog).getByRole("button", { name: "Remove secret" });
    expect(confirm).toBeDisabled();
    await userEvent.type(
      within(dialog).getByLabelText(/Type .* to confirm/),
      "MAIL_TOKEN",
    );
    expect(confirm).toBeEnabled();
    expect(ranArgv().some(([verb, sub]) => verb === "secret" && sub === "rm")).toBe(
      false,
    );
    await userEvent.click(confirm);
    await waitFor(() =>
      expect(ranArgv()).toContainEqual(["secret", "rm", "srv-foxtrot", "MAIL_TOKEN"]),
    );
    await waitFor(() =>
      expect(toast.success).toHaveBeenCalledWith("Removed MAIL_TOKEN from foxtrot-mail"),
    );
    await waitFor(() => expect(onRegistryChange).toHaveBeenCalled());
  });

  it("test12_servers_secret_remove_with_a_wrong_key_typed_stays_disabled", async () => {
    setup();
    await open();
    await userEvent.click(
      within(await secretsRow()).getByRole("button", { name: "Remove…" }),
    );
    const dialog = await screen.findByRole("dialog");
    await userEvent.type(
      within(dialog).getByLabelText(/Type .* to confirm/),
      "mail_token",
    );
    expect(within(dialog).getByRole("button", { name: "Remove secret" })).toBeDisabled();
  });

  it("test13_a_locked_keychain_makes_the_row_read_only_and_says_so", async () => {
    setup({ vaultLocked: true });
    const row = await secretsRow();
    expect(
      await within(row).findByText(/The keychain did not answer: the keychain is locked/),
    ).toBeInTheDocument();
    expect(within(row).getByText("Locked")).toBeInTheDocument();
    expect(within(row).getByRole("button", { name: "Try again" })).toBeInTheDocument();
    expect(within(row).getByRole("button", { name: "Set…" })).toBeDisabled();
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
