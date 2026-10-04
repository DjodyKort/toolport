import { expect } from "vitest";
import { act, render, screen, waitFor, within } from "@testing-library/react";
import userEvent, { type UserEvent } from "@testing-library/user-event";
import type { ReactElement } from "react";
import type { Bridge } from "./testkit";

/** Renders a panel and waits until the command list has been read, so a write button the
 * test presses already has its policy. */
export async function open(element: ReactElement, bridge: Bridge) {
  const user = userEvent.setup();
  const view = render(element);
  await waitFor(() => expect(bridge.count("commands")).toBeGreaterThan(0));
  await act(async () => {});
  return { user, ...view };
}

/** One confirmed write: press the button, wait for the plan, type the phrase when the tier
 * asks for one, confirm, wait for the result, close it. */
export async function write(
  user: UserEvent,
  button: string | RegExp,
  confirm: string,
  options: { typed?: string; plan?: string | RegExp; done: string | RegExp },
) {
  await user.click(screen.getByRole("button", { name: button }));
  const box = await screen.findByRole("dialog");
  await within(box).findByRole("button", { name: confirm });
  if (options.plan)
    expect(await within(box).findByText(options.plan)).toBeInTheDocument();
  if (options.typed) {
    expect(within(box).getByRole("button", { name: confirm })).toBeDisabled();
    await user.type(within(box).getByRole("textbox"), options.typed);
  }
  await user.click(within(box).getByRole("button", { name: confirm }));
  await screen.findByText(options.done);
  const done = screen.getByRole("dialog");
  await user.click(within(done).getAllByRole("button", { name: "Close" }).at(-1)!);
  await waitFor(() => expect(screen.queryByRole("dialog")).toBeNull());
}
