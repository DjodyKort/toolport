import { expect } from "vitest";
import { render, screen, waitFor, within } from "@testing-library/react";
import userEvent, { type UserEvent } from "@testing-library/user-event";
import { PlusViews } from "../PlusViews";

/** Walks to the Compression tab the way a person does: the Tokens screen, then its tab.
 * Everything after that runs through the real panels. */
export async function openCompression() {
  const user = userEvent.setup();
  render(<PlusViews view="tokens" onSelectView={() => {}} />);
  const tabs = await screen.findByRole("tablist", { name: "Tokens sections" });
  await user.click(within(tabs).getByRole("tab", { name: "Compression" }));
  await screen.findByLabelText("Compression status");
  return user;
}

export const stat = (label: string) => {
  const strip = within(screen.getByLabelText("Compression status"));
  return strip.getByText(label).parentElement as HTMLElement;
};

export const section = (name: string) => within(screen.getByRole("region", { name }));

/** One confirmed write: wait for the plan, type the phrase when the tier asks for one,
 * confirm, wait for the result, close it. A write without a preview confirms the same way;
 * its result is not a plan, so `done` may be left out. */
export async function confirm(
  user: UserEvent,
  label: string,
  options: { typed?: string; done?: string | RegExp } = {},
) {
  const box = await screen.findByRole("dialog", { name: /\?$/ });
  await within(box).findByRole("button", { name: label });
  if (options.typed) {
    expect(within(box).getByRole("button", { name: label })).toBeDisabled();
    await user.type(within(box).getByRole("textbox"), options.typed);
  }
  await user.click(within(box).getByRole("button", { name: label }));
  if (options.done) await screen.findByText(options.done);
  const done = await screen.findByRole("dialog", { name: (name) => !name.endsWith("?") });
  await waitFor(() =>
    expect(within(done).getAllByRole("button", { name: "Close" }).length).toBeGreaterThan(
      1,
    ),
  );
  await user.click(within(done).getAllByRole("button", { name: "Close" }).at(-1)!);
  await waitFor(() => expect(screen.queryByRole("dialog")).toBeNull());
}
