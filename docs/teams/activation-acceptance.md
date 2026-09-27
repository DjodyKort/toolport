# Two-member activation acceptance

Use a synthetic control plane and accounts only. Run native clients in separate Omabox instances with private HOME and keyring. Never reuse production credentials, publish production configuration, or test a write-capable upstream tool without explicit approval.

## Prerequisites

- Compatible Teams backend with activation receipts and pairing, then the matching desktop build.
- Admin A: three working personal server definitions, one harmless read-only test server selected for sharing; no Teams connection.
- Developer B: clean Toolport state, same harmless server executable available where required, upstream credentials available locally if the server requires them.
- A supported AI client installed. OpenCode was used in the implementation run.
- Separate authenticated accounts, with invitation sent to B's verified address.

## Owner

1. Sign in and create the named team. The initial screen must lead with Connect Toolport, not analytics or a trial.
2. Connect Toolport, launch the app, confirm the control-plane origin, and compare the browser/native device check. Approve under A's account. The portal waits for actual client contact.
3. Open desktop Teams → Share selected servers. Choose exactly one personal server. Review Added/Changed/Removed and credential requirements. Publish; unrelated remote definitions must remain.
4. Review the complete command/arguments/working directory or endpoint. Use the managed version for this profile explicitly; retain the personal original and other profile selections. Complete local authentication where required.
5. Desktop Clients → connect the supported AI client. Restart its MCP connection if requested.
6. In that client, explicitly request a harmless managed tool action. Verify its result and the portal's first-success state.
7. Invite B only after owner success.

## Invited developer

1. Open the named invitation. Sign in, confirm the accepting account and role, and choose Join [Team Name]. If wrong, Switch account; do not create a replacement organization.
2. Pair Toolport using the same origin/device-check flow. The same team must appear in desktop and portal.
3. Receive the managed definition, review it, supply credentials locally, and enable it.
4. Connect the AI client and approve a harmless call to that same managed server.
5. Owner portal must now show collaborative activation. Two devices on one member or successful calls to unrelated servers must not qualify.

## Hidden/background gate

1. Hide B's window while leaving the application running.
2. Publish a safe observable change, e.g. disable the synthetic echo tool. Verify the next call is rejected and an applied-version receipt arrives without reopening.
3. Re-enable it in a new version. Verify a successful call and acknowledged success evidence while still hidden.
4. Repeat native lifecycle checks with GTK and Tauri. Interrupt the synthetic server, make an offline-local call, restore the server, and verify retained counters catch up without duplicates. Finally remove the synthetic membership and verify managed configuration is removed.

## Evidence and limits

Record raw shared ID, member/device, config version and server-received first/latest success timestamps; do not capture prompts, arguments, results or credentials in operational receipts. Test fixture output can be retained separately because it is synthetic.

The September 27 implementation run passed two-member GTK setup, hidden GTK/Tauri delivery and successful reporting, SQLite/Postgres contracts, and one real OpenCode model-driven call. Browser/native URL handoffs were routed by the isolation harness because the collaborative browser lives outside Omabox. Packaged OS handler installation across platforms and a timed human under-five-minute run remain separate release checks. The synthetic echo server requires no upstream OAuth; credential transfer was covered separately with synthetic local secrets, not a third-party OAuth service.

One active installation per member seat remains the current limitation. Pending pairing is single-process/sticky-routed and expires on server restart. See the Teams repository's `docs/device-pairing.md` and `docs/activation-evidence.md` before release.
