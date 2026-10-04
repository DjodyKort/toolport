import { vi } from "vitest";
import { render } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { IntegrationsTab } from "./IntegrationsTab";
import { LoginsScreen, type LoginTabId } from "./LoginsScreen";
import { LoginsTab } from "./LoginsTab";
import { SecretsTab } from "./SecretsTab";
import type { Bridge } from "./testkit";

type Mocks = { invoke: ReturnType<typeof vi.fn>; listen: ReturnType<typeof vi.fn> };

type Listener = (event: { payload: unknown }) => void;

/** The bridge subscribes to job events once per module load, so the callback it hands to
 * `listen` is kept here and every test's fake bridge sends its stderr lines through it. */
let route: Listener | null = null;

/** Wires the mocked Tauri modules of a test file to a bridge. A held run (a sign-in) prints
 * its stderr through the callback the job hook registered with `listen`. */
export function wire(mocks: Mocks, bridge: Bridge) {
  mocks.listen
    .mockReset()
    .mockImplementation(async (_name: string, callback: Listener) => {
      route = callback;
      return () => {};
    });
  mocks.invoke.mockReset().mockImplementation(bridge.invoke);
  bridge.onEvent((event) => route?.(event));
}

const tabs = { logins: LoginsTab, secrets: SecretsTab, integrations: IntegrationsTab };

export function renderTab(id: LoginTabId, options: { pollMs?: number } = {}) {
  const onOpenCommands = vi.fn();
  const user = userEvent.setup();
  const Tab = tabs[id];
  const view = render(
    <Tab onOpenCommands={onOpenCommands} pollMs={options.pollMs ?? 0} />,
  );
  return { ...view, user, onOpenCommands };
}

export function renderScreen(options: { pollMs?: number; initialTab?: LoginTabId } = {}) {
  const onOpenCommands = vi.fn();
  const user = userEvent.setup();
  const view = render(
    <LoginsScreen
      initialTab={options.initialTab}
      onOpenCommands={onOpenCommands}
      pollMs={options.pollMs ?? 0}
    />,
  );
  return { ...view, user, onOpenCommands };
}
