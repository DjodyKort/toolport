import { vi } from "vitest";
import { render, screen } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { ServersScreen } from "./ServersScreen";
import type { Bridge } from "./testkit";

/** Wires the mocked Tauri modules of a test file to a bridge and renders the screen. */
export function wire(
  mocks: { invoke: ReturnType<typeof vi.fn>; listen: ReturnType<typeof vi.fn> },
  bridge: Bridge,
) {
  mocks.listen.mockReset().mockResolvedValue(() => {});
  mocks.invoke.mockReset().mockImplementation(bridge.invoke);
}

export function renderScreen(options: { pollMs?: number } = {}) {
  const onOpenCommands = vi.fn();
  const onOpenClassic = vi.fn();
  const user = userEvent.setup();
  const view = render(
    <ServersScreen
      onOpenCommands={onOpenCommands}
      onOpenClassic={onOpenClassic}
      pollMs={options.pollMs ?? 0}
    />,
  );
  return { ...view, user, onOpenCommands, onOpenClassic };
}

/** The tab of the screen with this name, counts such as `Profiles 3` included. */
export const tab = (name: string) =>
  screen.getByRole("tab", { name: new RegExp(`^${name}(?![\\w-])`) });
