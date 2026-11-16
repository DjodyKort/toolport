# Mac check for step 1.4: login-keychain secrets on layout A

Goal: the signed v2 app reads the real login-keychain master key, opens a copy of the
backup data dir and shows the same secrets, with no keychain prompt. Run it on the Mac,
with Djody at the keyboard. Nothing here installs into `/Applications` and nothing touches
the live data dir; the real login keychain is only read in pass A.

Never `cat` or print `secrets.enc`, and never paste a secret value into a chat or a note.
Names only.

## Setup

1. Quit every running Toolport (app and gateway): `pgrep -fl 'Toolport|toolport-gateway'`
   must print nothing. A running 1.x app would take the single-instance slot.
2. Build: `scripts/fork/build-app.sh` (3 to 7 minutes). Output:
   `src-tauri/target/release/bundle/macos/Toolport.app`.
3. Stage and sign a copy, with the `Toolport Plus Local Signing` identity:

   ```sh
   CHECK=~/toolport-mac-check; mkdir -p "$CHECK"
   ditto src-tauri/target/release/bundle/macos/Toolport.app "$CHECK/Toolport.app"
   xattr -dr com.apple.quarantine "$CHECK/Toolport.app"
   scripts/fork/fork-codesign.sh --app "$CHECK/Toolport.app" sign
   scripts/fork/fork-codesign.sh --app "$CHECK/Toolport.app" verify   # exit 0
   codesign -d --entitlements - "$CHECK/Toolport.app" 2>&1 | grep -c keychain-access-groups   # 0
   ```

4. Copy the backup data dir and check the marker (the marker is not a secret):

   ```sh
   cp -R ~/.toolport-backups/2026-11-16-pre-2x/data-dir "$CHECK/data"
   cat "$CHECK/data/secrets-backend"      # login-keychain
   test -f "$CHECK/data/secrets.enc" && echo vault present
   ```

## Pass A: read only, no keychain write

`TOOLPORT_KEYCHAIN_ACL_REFRESH=off` skips the one keychain write the app can do at start
(the trusted-application rewrite), so pass A only reads.

```sh
TOOLPORT_DATA_DIR="$CHECK/data" TOOLPORT_KEYCHAIN_ACL_REFRESH=off \
  "$CHECK/Toolport.app/Contents/MacOS/conduit"
```

| #   | Check                                                                      | Expect                                                  |
| --- | -------------------------------------------------------------------------- | ------------------------------------------------------- |
| A1  | Launch, wait for the window                                                | no keychain dialog (title mentions `conduit-mcp`)       |
| A2  | Servers view: secret env names marked as saved                             | 13 names, across 9 stdio servers; write the names down  |
| A3  | The 5 remote servers (clickup, miro, slack, figma-mcp-server, mail-tester) | each shows a session, 5 in all                          |
| A4  | Start one stdio server that holds a saved secret                           | it starts; no dialog (the gateway is a separate binary) |
| A5  | Quit, launch again                                                         | still no dialog                                         |
| A6  | `log show --last 10m --predicate 'process == "SecurityAgent"'`             | no lines                                                |

If a dialog appears: choose Deny the first time and keep a screenshot (which binary, which
item). The app must then show a plain error, not an empty vault: Servers must not read
"no secrets saved". Then repeat and choose Always Allow once, and record whether the next
launch is quiet.

Optional, headless: the ignored test uses the real adapter and prints names only. Its test
binary is not signed with the identity, so macOS asks once for it; Deny shows the error text
(`login-keychain mode: could not read the vault master key ...`), Always Allow shows the counts.

```sh
bash -c 'source scripts/fork/env.sh; TOOLPORT_MAC_CHECK_DATA_DIR="$HOME/toolport-mac-check/data" \
  app_cargo test --locked --manifest-path src-tauri/Cargo.toml --lib \
  --no-default-features --features search-static,test-support,plus \
  -- --ignored mac_check --nocapture'
```

## Pass B: the trusted-application refresh (only after A is clean)

Pass B rewrites the master key item's trusted-application list, rebuilt from scratch: it
then names the staged copy under `~/toolport-mac-check`, its gateway and its sidecars, and no
longer the installed 1.x app. Do this together with the real switch, not before.

1. Without `TOOLPORT_KEYCHAIN_ACL_REFRESH`, launch as in pass A.
2. Expect no dialog, and `$CHECK/data/.keychain-acl-fingerprint` exists afterwards.
3. Quit, launch again: no dialog, and the fingerprint file is unchanged (`shasum` before and after).
4. Keychain Access, item `conduit-mcp` / `__conduit_master_key__`, Access Control tab: the
   staged `conduit`, `toolport-gateway`, `toolportctl` and `toolport-selfmcp` are listed.
5. Rebuild and re-sign (new cdhash), launch: record whether a dialog appears. If `toolportctl` or
   `toolport-selfmcp` prompts after Always Allow, run `scripts/fork/keychain-partition-list.sh`
   (asks for the login-keychain password on a terminal) and retry.

## Afterwards

Quit the app, then `rm -rf ~/toolport-mac-check`. Send back: the A1 to A6 and B results,
the dialog texts if any, and the names (not values) from A2 and A3.

To make it a real install later: `scripts/fork/mac-install.sh --dry-run`, then without
`--dry-run`. It is a draft and has not been run.
