# Toolport+ extensions

Toolport+ is a fork of Toolport. Everything the fork adds lives under `src-tauri/src/plus/` (Rust) and `src/plus/` (React), so upstream merges rarely conflict. The desktop app reaches the Rust side through one generic IPC command, `plus_invoke`, and the `toolportctl` binary reaches it through a command table.

## Module map

| Module (`src-tauri/src/plus/`) | What it does                                                                                                                                               |
| ------------------------------ | ---------------------------------------------------------------------------------------------------------------------------------------------------------- |
| `mod.rs`                       | Handler table (`plus.*` names) and `dispatch`; the only IPC surface (`plus_invoke`).                                                                       |
| `ctl/`                         | `toolportctl` command line: parsing, the `COMMANDS` table, JSON envelope output, per-area command files.                                                   |
| `direct/`                      | Opt-in direct client entries: ownership record, `direct run` launcher, core of `client direct`.                                                            |
| `auth/`                        | Expired-login detection: stdio and remote auth probes, status cache, gateway state, Google OAuth refresh, status surfaces (statusline, SessionStart hook). |
| `context/`                     | Context engine: layered rules, `CLAUDE.local.md` deploy, settings union, legacy MCP dedupe, shims, "what loads" viewer, folder profiles.                   |
| `compression/`                 | Compression policy, provider health, shims, launch plan and the token-savings ledger.                                                                      |
| `skills/`                      | SKILL.md parser, lockfile, lint, collision backups, per-client transpilers (agents, styles, assets), skill taps (add, update, search, install).            |
| `sync/`                        | Encrypted cross-machine sync (Fernet bundle, KDF, git transport, origins).                                                                                 |
| `import_mcpm/`                 | Pure mapping from an mcpm config root to registry servers, name map, tool-reference rewrite.                                                               |
| `selfmcp/`                     | Self-management MCP server: tool and resource catalog, confirm-tier enforcement, backends onto skills, registry and client sync.                           |
| `sources/`                     | Where every skill, command, agent, rule and CLAUDE.md comes from: ten read-only detectors, a bounded scan with a cache, `sources root` (see Sources).      |
| `plan.rs`                      | `PlanV1` and `ResultV1`, the plan and result shapes of the GUI-wave writers (`sources root add\|rm` first).                                                |
| `update/`                      | Update checks and apply for registry servers (git ff-only, release binaries, npx/uvx pins).                                                                |
| `obs/`                         | Claude Code transcript indexer, loopback OTLP receiver and its settings switch, monitor database import.                                                   |
| `cc/`                          | Claude Code plugin list and update.                                                                                                                        |
| `council/`                     | Council as a downstream MCP server definition.                                                                                                             |
| `jsonfs.rs`                    | Tolerant JSON file read shared by the modules above.                                                                                                       |
| `testutil.rs`                  | Test-only fixtures (see Test conventions).                                                                                                                 |

Frontend (`src/plus/`): `api.ts` (`plusInvoke` wrapper), `bridge/` (the `toolportctl` child-process bridge), `ui/` (the shared kit), `allcommands/`, `PlusViews`, `nav.ts`, `AuthRows` and `AuthNotifier`, `FolderProfiles`, `WhatLoads`, with fixtures under `src/plus/fixtures/` (see Frontend shell below). New IPC commands must be registered in `src/plus/fixtures/plusInvoke.ts`, which `src/test/browser-fixture.tsx` serves.

## toolportctl commands

Global flag `--json` prints one envelope (`schemaVersion`, `command`, `data`). Exit codes: 0 ok, 1 error, 2 usage. The table below mirrors `COMMANDS` in `ctl/mod.rs`; group rows (no handler) print help.

| Command                                                                 | Purpose                                                            |
| ----------------------------------------------------------------------- | ------------------------------------------------------------------ |
| `status`                                                                | Registry, profile, secrets backend and gateway state               |
| `doctor`                                                                | Read-only health checks                                            |
| `commands`                                                              | The command registry: tier, dry-run, flags (`commands --json`)     |
| `server ls / search / install / uninstall / info / new / edit`          | Catalog and registry server management                             |
| `inspect`, `profile inspect`                                            | List tools of a server or of a whole profile (connects live)       |
| `profile ls / create / edit / rm`                                       | List, create, edit and remove profiles; `rm` cleans clients        |
| `client ls / sync / edit / import`                                      | Detect clients; sync; set a client's profile; import entries       |
| `client direct add / rm / ls`                                           | Opt-in: give one client a direct entry for one stdio server        |
| `direct run`                                                            | Stdio launcher a direct client entry starts (not run by hand)      |
| `auth statusline / hook / probe / login`                                | Auth-health JSON; probe now (`--server`, `--force`); sign in again |
| `secret set / get / rm`                                                 | Server secrets (stdin or `--value-env`; `get --reveal` prints)     |
| `context loads / folders / checkpoint-status / plan / apply / sync`     | What a session loads, folder profiles, checkpoint, context deploy  |
| `context measure`                                                       | Real token count of a folder from Claude Code (spends tokens)      |
| `context init / status / client / profile / disable`                    | Scaffold layers and launch profiles, show state                    |
| `compression status / presets / run / verify / ledger / proxy / update` | Compression policy and launch                                      |
| `import mcpm / rename-refs`                                             | Import an mcpm root; rewrite tool references                       |
| `council`                                                               | Council server install, uninstall, doctor, tools                   |
| `mcp`                                                                   | Self-management server: install, uninstall, doctor, tools          |
| `skills init/add/ls/lint/audit/bundle/unbundle/sync/diff`               | Skills repo init, add, list (`--source <id>`), lint, audit, ...    |
| `sources ls / root ls / root add / root rm`                             | Where skills, commands, agents, rules and CLAUDE.md come from      |
| `plugins ls / show`                                                     | Installed Claude Code plugins: what each brings, costs and enables |
| `hooks ls`                                                              | Every hook Claude Code would start in a folder (never runs one)    |
| `skills status / clean / uninstall / resolve`                           | Lock vs outputs, remove managed outputs, uninstall, collisions     |
| `skills tap add/ls/remove/update, search, install`                      | Skill taps: register, list, pull, search, install (`--dry-run`)    |
| `agents add/ls/lint/audit/diff/status/clean/uninstall/sync`             | Agents: template, list, lint, audit, drift, sync, remove           |
| `styles add/ls/lint/diff/status/sync/apply/remove/clean`                | Output styles: template, list, lint, drift, sync, apply, remove    |
| `sync`                                                                  | Encrypted sync (init, push, pull, diff, status, reset, ...)        |
| `cc`                                                                    | Claude Code plugins: list, update                                  |
| `update`                                                                | Server updates (`--check`, `--apply`, `--init`, `--dry-run`)       |
| `usage`                                                                 | Token and MCP usage from Claude Code transcripts                   |
| `obs otel enable / disable / status`                                    | Loopback OTLP receiver and Claude Code telemetry env in settings   |

### Command registry and policy

`toolportctl commands --json` prints the registry the GUI is built from (D-058, D-059). `data.commands` has one row per `COMMANDS` path and per sub-command of the rows that dispatch further (`sync`, `council`, `mcp`, `cc`, `compression proxy`, `compression ledger`); `data.tools` has one row per self-management tool. Rows that are only a prefix of other rows are `kind: "group"` and carry no policy. A `kind: "command"` row has:

| Field                     | Meaning                                                                                                                                                                                          |
| ------------------------- | ------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------ |
| `id`, `path`, `parent`    | Space-joined path (`sync push`), its words, and the dispatching row for a sub-command                                                                                                            |
| `tier`                    | Highest tier the command can reach: `read` changes nothing, `write` changes files, `destructive` deletes or overwrites                                                                           |
| `baseTier`                | Tier of the bare invocation; lower than `tier` when flags in `escalators` (`--select`, `--apply`, ...) turn a read into a write                                                                  |
| `dryRun`, `preview`       | Whether the command can preview; `preview.mode` is `flag` (`--dry-run` or `--plan` previews), `unless-applied` (previews until the apply flag is given) or `none`                                |
| `needs`                   | `stdin` (a secret goes over the pipe), `browser`, `long-running`, `network`, `terminal-only`                                                                                                     |
| `cost`                    | Calls a paid or rate-limited service                                                                                                                                                             |
| `surface`                 | `screen` or `terminal` (D-062): `direct run` and `compression run` replace themselves with another process and never run through the app                                                         |
| `operands`, `maxOperands` | Positional arguments in order, with `required` and `variadic`; `maxOperands` is null when unlimited                                                                                              |
| `flags`                   | Per flag: `name`, `aliases`, `valueType` (`bool`, `string`, `integer`, `path`, `list`, `paths`, `choice` with `choices`), `required`, `repeatable`, `escalates`, `hidden`, `sensitive`, `effect` |
| `oneOf`                   | Groups of flags of which at least one is needed                                                                                                                                                  |
| `tools`                   | Self-management tools that run the same core                                                                                                                                                     |

The policy is data in `ctl/policy.rs`; flag types and effect text are in `ctl/commands_json.rs`. A tool row says `tier` (self-MCP tiers 1 to 3 read or write, 4 destructive), `toolTier`, `dryRun` (`none`, `param`, `default-on`: D-048) and the `command` it maps to. Tests (`plus::ctl::policy_tests`, `tests/gui_parity.rs`) fail on a command without a policy row, a row without a command, a flag the parser does not know, a flag without a type, a preview flag the parser rejects, and a tool whose tier or dry-run default differs from its row or from the command it maps to. The bridge refuses a terminal-only argv (`ctl::terminal_only`).

### GUI parity manifest

`src/plus/gui-parity.json` says where every registry command and every self-management tool lives in the app (D-058, rule R3 of `index/gui-parity.md`). Typed in `src/plus/guiParity.ts`.

```json
{
  "schemaVersion": 1,
  "routes": { "all-commands": { "title": "All commands", "status": "planned" } },
  "actions": {
    "all-commands.run": { "route": "all-commands", "status": "planned", "summary": "..." }
  },
  "owners": { "skills": "MIG-GUI-3" },
  "commands": {
    "skills ls": {
      "route": "all-commands",
      "action": "all-commands.run",
      "surface": "screen"
    }
  },
  "tools": {
    "skills_list": {
      "route": "all-commands",
      "action": "all-commands.run",
      "surface": "screen"
    }
  }
}
```

- `commands` has one entry per registry command, sub-commands included (`sync push`, `compression ledger record`); groups have none. `tools` has one entry per self-management tool; a tool that runs a command normally uses that command's route and action.
- `surface` is `screen` or `terminal` (D-062). It must equal the registry: only `direct run` and `compression run` are `terminal`.
- A route is `planned` or `built`; `built` needs `component`, an existing file. An action is `planned` or `built`; `built` needs its route built and `test`, an existing component test that names the action id (a `data-action` query or the test title). Every route has an action and every action is used.
- A row is pending while its route is `all-commands` or its action is `planned`. The tests print the pending count per owner; `GUI_PARITY_STRICT=1` makes any pending row a failure (MIG-GUI-9 sets it).
- `owners` maps each command group to the item that gives it a screen.

`cargo test --test gui_parity` (Rust) and `vitest src/plus/guiParity.test.ts` (reads the blessed `src-tauri/tests/fixtures/ctl-envelopes/commands.json`) fail on a command or tool without an entry (printing the line to paste), an entry for something that no longer exists, a missing route or action, an action that belongs to another route, a surface that differs from the registry, and a built route or action without its file or test. A change to the registry changes `commands.json`, the golden of `toolportctl commands` (bless it with `CTL_ENVELOPE_BLESS=1 cargo test --test ctl_contract`, rejected when `CI` is set), which in turn makes the manifest test ask for the new entry.

A screen item adds its routes and actions, points its rows at them, and flips `status` to `built` together with the component and its test. A command that does not exist yet has no entry: whoever adds the command adds the row.

### Command contract: golden envelopes and TS shapes

The GUI parses what `toolportctl --json` prints, so each command has a golden envelope and a TS shape for its `data` (D-058).

- `src-tauri/tests/ctl_contract.rs` runs the real `toolportctl` against the synthetic world in `tests/common/ctl_world.rs` (no real credentials, the child's home, data directory and working directory all inside the world). One `case(<command id>, steps)` per command; a writer has a `--dry-run` step, which must leave the world byte-identical, and then the apply step. Every step also checks the single-line envelope, the exit code, and that no secret value reaches stdout or stderr.
- Goldens live in `tests/fixtures/ctl-envelopes/<id>[.<step>].json` as `{argv, exitCode, envelope}`. Paths, times and the keys `version`, `pid`, `elapsedMs`, `durationMs`, `tookMs` are replaced by placeholders. They are compared as JSON, so a formatter does not matter. `CTL_ENVELOPE_BLESS=1 cargo test --no-default-features --test ctl_contract` rewrites them and is rejected when `CI` is set; review the diff like any contract change.
- `src/plus/bridge/data.ts` holds one shape per golden (`obj`, `arr`, `nullable`, `opt`, `lit`, ...: the TS type is `Infer<typeof shape>`). `src/plus/bridge/data.test.ts` validates every golden against its shape, strictly, so a field added to the CLI output fails the test until the shape follows. A golden without a shape is listed by the test; `CTL_CONTRACT_STRICT=1` turns the list into a failure (also for commands without a case in `ctl_contract`).
- A failed command may still carry `data` next to `error` (`skills diff` exits 1 with the drift list), so a screen reads `data` of a failed envelope too (`CtlError.data`).

To add a command: one `case(...)` row, bless, read the new file, add the shape to `data.ts` and its stem to `ctlShapes`. A command that needs more of the world than `CtlWorld` offers gets a setup step or a fixture added to `CtlWorld`: `sources_case(...)` adds the sources fixture home (`tests/common/sources_world.rs`, the contract's section 13 world) to the case's world. A backup directory name (`20261004-154128`) becomes `<STAMP>` in a golden. The snapshot that proves a read or a dry run changed nothing leaves out `<data dir>/plus/cache`, which is derived state a scan may rewrite.

`npm run screenshots:gui` (`scripts/screenshots.mjs`) runs the browser smoke and keeps its `gui-<screen>.png` files in `docs/assets/` (`guiShot` in `scripts/browser-smoke.mjs`; a screen adds one line there). It fails when a page shot is missing or not 1280x800. Chromium's PNGs are already smaller than a zlib level 9 recompression, so they are kept as they are. The committed shots:

| File                                      | Shows                                                                        |
| ----------------------------------------- | ---------------------------------------------------------------------------- |
| `gui-shell-light.png`, `-dark`            | The shell: sidebar B (groups, Attention counter) around an upstream view     |
| `gui-all-commands-light.png`, `-dark`     | The All commands page with a command picked and its form filled in           |
| `gui-plan-confirm-light.png`              | The typed confirmation of a destructive command, with its plan               |
| `gui-library-light.png`                   | A screen that is not built yet: the tabs of the mockup and the marked notice |
| `gui-auth-rows.png`, `gui-what-loads.png` | Element shots of the login health and the "what loads" panels                |

The Servers screen (MIG-GUI-1) adds:

| File                                    | Shows                                                                           |
| --------------------------------------- | ------------------------------------------------------------------------------- |
| `gui-servers-light.png`, `-dark`        | Servers grouped by attention, a server that needs a login with its live inspect |
| `gui-servers-gateway-light.png`         | Element shot of the gateway strip                                               |
| `gui-servers-plan-light.png`            | The preview of a write (a server switched on in a profile) before Apply         |
| `gui-servers-remove-light.png`          | The typed confirmation of removing a server, with its plan                      |
| `gui-servers-profiles-light.png`        | The Profiles tab                                                                |
| `gui-servers-profile-inspect-light.png` | A profile inspected while one of its servers needs a login                      |
| `gui-servers-clients-light.png`         | The Clients tab: what each client sees, and the orphan list                     |
| `gui-servers-health-light.png`          | The Health tab: status facts, named fixes and the doctor checks                 |

## Frontend shell, UI kit and All commands page

`src/plus/` after MIG-GUI-0:

- `bridge/` runs `toolportctl --json` as a child process and types what comes back (`ctl.ts`, `data.ts`, `shape.ts`).
- `ui/` is the kit every Toolport+ screen is built from (below).
- `nav.ts` holds the grouped sidebar B (`NAV_GROUPS`, 16 items, labels as in the approved mockup; the Servers entry opens the Toolport+ view `control`, and the upstream `servers` view is lit under it), the title and subtitle of each Toolport+ screen (`PLUS_SCREENS`) and the item that builds each. The tabs of the mockup live in `notBuiltTabs.ts`, which only the placeholder reads, so they stay out of the startup bundle. `SidebarNav.tsx` draws the groups with the sidebar's own row, `attention.ts` reads the Attention counter.
- `PlusViews.tsx` is the one entry for Toolport+ screens; each is a `React.lazy` chunk. `NotBuilt.tsx` is the marked placeholder (tabs of the mockup, a "Not built yet" notice naming the item, a button to the All commands page on the right command group).
- `allcommands/` is the All commands page.
- `servers/` is the Servers screen (MIG-GUI-1, below).

**Adding a screen.** Put it under `src/plus/<screen>/`, import it with `React.lazy` in `PlusViews.tsx` in place of the `NotBuilt` branch, add its view to `PLUS_VIEWS` if it is new, and flip its rows in `gui-parity.json`. A screen reads data with `useCtlQuery` inside `AsyncView` (skeleton, error with Retry and "Copy diagnostics", empty) so it never renders nothing, and writes through the preview, confirm and apply flow below.

**The Attention counter** comes from one function, `readAttentionCount` in `attention.ts`: `counts.needsYou` of `toolportctl attention ls`, `null` (no badge) while the CLI has no such row. The badge shows only above zero. It is polled once a minute and a failed read keeps the last number.

**UI kit (`src/plus/ui/`).**

- `Tabs`: roving tabindex, arrows, Home and End, `aria-selected`, panel linked to its tab.
- `PlanPreview` and `DataView`: a `PlanV1` as a list of steps with collapsible diffs, the token effect, warnings and the undo command; older shapes as a readable list; anything else as pretty JSON.
- `TypedConfirmDialog`: the upstream `ConfirmDialog` with a field; the button works only when the phrase matches exactly. Enter never confirms. Used for the `destructive` tier (D-081).
- `JobProgress` with `useCtlJob`: stderr as it arrives, Cancel (kills the child), the final envelope as Done, Failed or Cancelled. Leaving the screen cancels a running job.
- `SecretField`: write-only password input, no React state for the value, no reveal; the value goes to the child's stdin and the field is empty afterwards.
- `AsyncView`, `ScreenSkeleton`, `ErrorState`, `useCtlQuery`: the loading, error and empty states.
- `keyboard.ts`: the number keys stay at the shipped 7 views; the new screens add no global chord; a bare-key shortcut inside a screen is allowed only through `screenKeyAllowed` (not while typing, never with Ctrl, Cmd or Alt, never `/`, `?` or Escape); Escape never cancels a job.

**All commands (`src/plus/allcommands/`).** The page is generated from `toolportctl commands --json`: search over id, summary and flag names, a group filter, a tier badge on every row and a form per command from its operands and flags (switch, text, comma list, one-per-line for a repeatable flag, a select for choices). Rules, all in `model.ts` and covered by tests over the real registry golden:

- Hidden flags, flags that take a secret and the preview flag are never offered or sent. A command that reads stdin gets a `SecretField`; `--passphrase-stdin` is added when it holds something.
- A read runs at once (a row that reads until a flag escalates it stays a read until the flag is set). A change runs preview (the row's own dry-run flag; `unless-applied` rows preview without their flag), plan, confirm, apply. A change without a preview, or one that reads stdin, is confirmed with its exact command line. `destructive` types a phrase: the first operand when it is short, else the command id.
- A terminal-only command shows its exact command line and a Copy button, never a Run button.
- "Run a tool" calls `toolportctl mcp call <tool> --args-stdin` for tools that no command covers (arguments as JSON on stdin; `dry_run` and `confirm` per the tool's tier). It is disabled, with a tooltip and a note, while `commands --json` has no `mcp call` row.

**Servers (`src/plus/servers/`).** The control center: tabs Servers, Profiles, Clients and Health (Logins, Secrets and Integrations are placeholder slots until MIG-GUI-2), a gateway strip above them, a "Run doctor" button and, on the Servers tab, a "Classic view" button. Every read is one `toolportctl --json` call through `ServersProvider` (`server ls`, `profile ls`, `client ls`, `status`, polled and kept on screen when a reload fails); every write goes through `useWrite`.

- **State** is derived in `model.ts` from `server ls`, `status` (gateway, tool cache, `auth.servers`), `profile ls` and `client ls`: connected, login needed, failed (with the CLI's reason), disabled. Servers are grouped by attention. "What a client sees" is the tool count per server of the profile the client uses.
- **Writes** follow D-059 and D-081 (`plans.ts`, `useWrite.tsx`, `WriteDialogs.tsx`): a command with a dry-run flag is previewed with it, then confirmed, applied and shown with its undo; a command without one (`server new`, `server install`, `server edit`) is confirmed with its exact command line and a client-side plan marked "no preview"; only a `destructive` row asks for a typed phrase. `direct run` is a terminal-surface action: the command is shown with Copy and "Open in Terminal" stays disabled.
- **Profile inspect** runs `profile inspect`; when that stops at the first failing server (a 401), the dialog asks each server with `inspect` (three at a time) and shows what answered, who needs a login and why.
- **Dialogs** have no trigger element, so `useRestoreFocus` returns focus to the control that opened a dialog once the last one closes.
- **View id and the Classic view (D-088).** The screen is the view `control`; the sidebar entry labelled Servers opens it. The upstream `servers` view and its page are untouched, so the health filter (Checking, Ready, Disabled), the team-enable review dialog and the upstream tests stay reachable and green: "Classic view" in the header of the Servers tab opens it, and the sidebar entry stays lit there. The app still starts on the upstream view. The button is not in the mockup; whether it stays is judged on the Mac.
- **Fixtures:** `fixtures/servers.ts` is one synthetic registry (eight servers including a failing one and two that need a login, three profiles, four clients, direct entries, a doctor with a named fix) served by the dev browser fixture and the tests; `servers/testkit.ts` is the fake bridge.

**Edits to upstream files.** Everything else is under `src/plus/`.

| File                                 | Edit                                                                                                                                                         |
| ------------------------------------ | ------------------------------------------------------------------------------------------------------------------------------------------------------------ |
| `src/App.tsx`                        | 3 imports; the title and subtitle ternaries end in `PLUS_SCREENS[view]`; one render branch `isPlusView(view) ? <PlusViews>`; `<AllCommandsLink>` in Settings |
| `src/components/AppSidebar.tsx`      | the nav list is `<SidebarNav>`; `navItem` takes `badgeLabel` and `urgent`; the icons that moved to `nav.ts` are no longer imported                           |
| `src/components/AppSidebar.test.tsx` | mocks `@/plus/attention`; a block of tests for sidebar B; no existing test changed                                                                           |
| `src/lib/types.ts`                   | `View` includes `PlusView`                                                                                                                                   |
| `src/test/browser-fixture.tsx`       | `plus_ctl` serves the rows of `plusCtlFixtures`; `plus_ctl_result` answers a held run (`plusCtlHeld`, a sign-in) when it is cancelled                        |
| `scripts/browser-smoke.mjs`          | sidebar B, a not-built screen and a run of `status` on the All commands page; the shots above in both themes                                                 |
| `scripts/screenshots-gui.mjs`        | renamed to `scripts/screenshots.mjs`                                                                                                                         |
| `package.json`                       | `screenshots:gui` points at `scripts/screenshots.mjs`                                                                                                        |
| `src/plus/gui-parity.json`           | the `all-commands` route and its `run` and `terminal` actions are `built`; `tool` stays `planned` until the CLI has `mcp call`                               |
| `src/App.tsx` (logins)               | `<AuthPanel onOpenLogins>`; the `AuthNotifier` Review button selects the `logins` view instead of Settings                                                   |
| `src/plus/nav.ts`, `PlusViews.tsx`   | `logins` is a Toolport+ view ("Logins & secrets") that lights Servers; one lazy branch renders `LoginsScreen`                                                |
| `src/plus/AuthRows.tsx`              | `AuthPanel` takes `onOpenLogins` and shows an "Open Logins & secrets" button                                                                                 |
| `scripts/browser-smoke.mjs` (logins) | Logins, the sign-in dialog, Secrets, the set and reveal dialogs and Integrations; `screenshots.mjs` lists the seven shots                                    |
| `src/plus/gui-parity.json` (logins)  | the `logins` route and its seven actions are `built`; `auth login`, `probe`, `statusline`, `hook` and `secret get`, `set`, `rm` point at them                |

MIG-GUI-1 edits:

| File                                       | Edit                                                                                                                                                                                                                             |
| ------------------------------------------ | -------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------- |
| `src/App.tsx`, `src/lib/types.ts`          | not edited (D-088): the upstream `servers` view, its header controls, the first view and every upstream App test stay as they are; `View` already includes `PlusView`, so `control` needs no line there                          |
| `src/plus/nav.ts`, `PlusViews.tsx`         | `control` joins `PLUS_VIEWS` and `PLUS_SCREENS`; the Servers entry of `NAV_GROUPS` opens it; `servers` lives under that entry (`LIVES_UNDER`), so the entry is lit on both; `PlusViews` lazy-loads `ServersScreen` for `control` |
| `src/plus/gui-parity.json`, `guiParity.ts` | route `servers` (view `control`; `GuiRoute.view` names the view when it differs from the route id) and 23 `servers.*` actions; 23 commands and 12 tools point at them                                                            |
| `src/plus/fixtures/plusCtl.ts`             | the `status` and `commands` rows come from `fixtures/servers.ts`; a `FixtureFailure` makes a failed envelope                                                                                                                     |
| `scripts/browser-smoke.mjs`                | `serversScreen` opens the Servers entry, walks the tabs and dialogs, writes the `gui-servers-*` shots and checks the Classic view round trip                                                                                     |
| `scripts/screenshots.mjs`                  | the `gui-servers-*` page shots are checked for size                                                                                                                                                                              |
| `src/plus/PlusViews.tsx` (library)         | `library` renders the lazy `LibraryScreen`: the Skills tab, a placeholder for the tabs not built yet                                                                                                                             |
| `src/plus/NotBuilt.tsx`                    | `Placeholder` is exported as `NotBuiltPanel` for the Library tabs that are not built yet                                                                                                                                         |
| `src/plus/fixtures/plusCtl.ts`             | spreads `skillsBrowserFixtures` (the stateful skills world); a function row is called; `commands` is `commandsWithSkills`                                                                                                        |
| `plus/fixtures/commandsRegistry.ts`        | exports `command`, `flag` and `DRY_RUN` for `skills/commandRows.ts`                                                                                                                                                              |
| `scripts/browser-smoke.mjs` (skills)       | Library opens on Skills and Plugins is the not-built tab; the Skills walk in both themes; `screenshots.mjs` lists its eight shots                                                                                                |
| `src/plus/gui-parity.json` (library)       | the `library` route and its 19 actions are `built`; 19 skills commands and 18 skills tools point at them                                                                                                                         |
| `src/plus/nav.ts` (agents)           | `agents` is a Plus view ("Agents & styles") that lights Library; `PlusViews.tsx` lazy-loads `AgentsScreen`                                                   |
| `src/plus/notBuiltTabs.ts`           | `PlusTab.opens`: the Agents and Styles tabs of the Library placeholder open the `agents` view                                                                |
| `src/plus/NotBuilt.tsx`              | `onOpenView` prop: an "Open Agents & styles" button on a tab that has `opens`                                                                                |
| `src/plus/fixtures/plusCtl.ts`       | `commands` appends `agentsCommandRows`; the agents rows are `createAgentsWorld()`; a function row is called                                                  |
| `scripts/browser-smoke.mjs` (agents) | the Agents and Styles walk in both themes; `screenshots.mjs` lists the seven shots                                                                           |
| `src/plus/gui-parity.json` (agents)  | the `agents` route and its 18 actions are `built`; the 18 agents and styles commands point at them                                                           |

## Sources

`plus/sources/` answers "where does this skill, command, agent, rule or CLAUDE.md come from" (D-063). `toolportctl sources ls [--source <id>] [--kind <k>] [--items] [--cwd <dir>] [--root <dir>]... [--deep] [--refresh]`, the self-MCP tool `sources_ls` and `skills ls --source <id>` read the same scan; `sources root ls|add|rm` edits `sourceRoots` in `context.json` (the folders the repo, client and vendored detectors look through) with a `PlanV1` preview.

- One `SourceDetector` per kind of place, in result order: `repo`, `client`, `vendored`, `plugin`, `org`, `account`, `library`, `tap`, `loose`, `inert`. The default roots are the clients root of `context.json` and its parent when that is a git checkout (the ODH root); `sourceRoots`, `--root` and the git top level of `--cwd` add to them.
- **Read-only by construction.** Detectors only get `fsx` (stat, list, read) and `gitx` (`git --git-dir=... rev-parse|for-each-ref|rev-list|ls-tree|cat-file|config --get`, `GIT_OPTIONAL_LOCKS=0`). Nothing fetches, nothing enters a worktree, and linked worktrees are never sources. A repo that lags behind its remote (ODH, 114 commits on the Mac) still lists the items of `origin/main` (`inCheckout=false`, path `origin/main:<file>`). A test greps the detector sources for write APIs and another compares a tree snapshot before and after.
- **Bounded.** Each detector gets depth 4 and 2 s, the whole scan 10 s; `--deep` doubles both. A budget hit never fails the command: the source gets `status.state = "partial"`, the detector is listed in `skipped` and `partial` is true. The hidden flag `--budget <detector>=<ms>` forces one detector's budget (0 skips it) so tests prove the mechanism without a wall-clock bound.
- **Cached** in `<data dir>/plus/cache/sources.json`, keyed on mtime and size per file, on the commit of a git tree, and on the clone head for the library; `--refresh` ignores the cache. The cache is derived state: deleting it changes nothing but speed.
- `org` hash-compares `~/.claude/CLAUDE.md` and the deployed commands against the corp-tools clone and never writes there; `shadowedBy` on a library skill names the org command that shadows it (`org:command:<name>`). `CLAUDE_CONFIG_DIR` moves the Claude home that the plugin, org, account and loose detectors read, and `CLAUDE_SYNC_INTERVAL` (seconds, default 14400) is the interval `managedBy` names.
- Repo and client items carry an `audit` value from `skills::audit`; a high finding is shown, never fixed.

## Library: the Skills tab

`src/plus/skills/` builds the Library screen of the approved mockup (nav item "Library", layout B). `LibraryScreen` has the five tabs Skills, Agents, Styles, Plugins and Sources; `PANELS` in it lists the tabs that are built (today `skills`), the others stay the marked placeholder of `NOT_BUILT_TABS` until their item adds its panel to that map. `PlusViews` lazy-loads the screen, so the whole tab is one chunk.

`SkillsTab` has four sections, one `useWrite` and one `WriteDialogs` for all of them:

| Section          | Reads                                                                                                                                                | Writes (policy tier from `toolportctl commands`)                                                                                                                                       |
| ---------------- | ---------------------------------------------------------------------------------------------------------------------------------------------------- | -------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------- |
| Installed        | `skills ls`, `sources ls`, `skills ls --source <id>`, `skills status`, `lint` (and `--name`), `audit`, `diff`, `sync --dry-run`, `resolve --dry-run` | `skills sync --client <k>...`, `skills add <name>`, `skills resolve --migrate`, `skills init` (write); `skills clean` and `skills uninstall <name>` (destructive: the phrase is typed) |
| Taps             | `skills tap ls`                                                                                                                                      | `tap add <user/repo or url> [--name]`, `tap update [name]`, `tap remove <name>` (all write: a plain confirmation)                                                                      |
| Find and install | `skills search <query>`                                                                                                                              | `skills install <@user/repo[/skill]>` (write); a high audit finding blocks it, and only `--no-audit` with a typed phrase gets past                                                     |
| Bundles          | none                                                                                                                                                 | `skills bundle --skills <a,b> --output <zip>` and `skills unbundle <zip> --path <dir>` (write; the files an unbundle overwrites are listed in the plan)                                |

- Every write is the D-059 flow: the command's own `--dry-run`, a plan worded from its answer (`plans.ts`: these commands answer in their own shape, not as `data.plan`), the confirmation, the apply in `JobProgress`, then every read of the open section runs again. A failed preview never applies, a command the registry does not classify is refused with a message, and `--home` is never sent.
- A sync always asks for the clients. The picker starts with the clients of the lock (or Claude Code when no sync has chosen yet, said on the screen), never all of them. The dropped fields (`allowed-tools` for Cursor) come from the warnings of the dry run and show before the sync.
- Scope: "your user level" or "one project" (a folder) for sync, clean, resolve and uninstall (`--project [--repo <dir>]`). The reads have no `--project` flag, so they stay at the user level; the preview of each write is run with the scope.
- `skills diff`, `lint` and `audit` exit 1 with a normal `data`; the panels read that data instead of showing an error. `Retry` on a failed list reads every failed read again, so Sync is not left disabled by an outage.
- A tap URL with a credential (`https://user:token@host/...`) is refused in the form: it would go on a command line and on the screen. Use an SSH URL or git's credential helper.
- With no repository, the list is the empty state "No skills repository yet" with "Create repository..." (`skills init`, folder through the native picker or typed).
- Editing a body or frontmatter and `skills_git_push` have no CLI command: their buttons are disabled with the reason (`mcp call`, MIG-GUI-14). `skills_scaffold` is `skills add --with-progressive`, so New skill covers it.
- Tests: `testkit.ts` is a fake `plus_ctl` bridge over the static fixtures (a command without a reply fails the test). `createBridge({ world: true })` and the dev browser fixture use `world.ts` instead, where an applied write changes the next read: a sync clears the drift, a clean removes the outputs and the lock, an uninstall or install changes the list, a resolve ends the collision; a preview changes nothing. `SkillsTab.e2e.test.tsx` walks the real screen through that world, one test per parity action id (keyboard use, bridge down then Retry and a credential canary included); `world.test.ts` checks every world reply against the golden shapes and `commandRows.test.ts` checks the browser registry rows against the golden registry.

Screenshots (1280x800, from `npm run screenshots:gui`): `docs/assets/gui-skills-light.png` and `gui-skills-dark.png` (35 items, source chips, one skill with its sync state), `gui-skills-checks-light.png` and `gui-skills-checks-dark.png` (lint, audit, changes since the last sync and drift), `gui-skills-sync-plan-light.png` (the sync plan with the dropped field), `gui-skills-uninstall-light.png` (the typed confirmation), `gui-skills-taps-light.png` and `gui-skills-install-blocked-light.png` (an install blocked by the audit). `gui-library-light.png` is now the not-built Plugins tab.

## OTel receiver

`toolportctl obs otel enable` turns on Claude Code telemetry and a loopback OTLP/HTTP-JSON receiver that stores it next to the transcript index. It is off until you run it.

- `enable [--port <n>] [--home <dir>] [--dry-run]` writes `CLAUDE_CODE_ENABLE_TELEMETRY=1`, `OTEL_METRICS_EXPORTER=otlp`, `OTEL_LOGS_EXPORTER=otlp`, `OTEL_EXPORTER_OTLP_PROTOCOL=http/json` and `OTEL_EXPORTER_OTLP_ENDPOINT=http://127.0.0.1:<port>` into the `env` block of `~/.claude/settings.json` and records the choice in `<data dir>/obs/otel.json` (`{"enabled": true, "port": 4318}`). Other keys are kept; a key that already holds a different value is a conflict (exit 1, nothing written). Prompt, tool-content and e-mail logging is never switched on, and those attributes are dropped when received.
- `disable [--home <dir>] [--dry-run]` removes only what `enable` wrote (and a settings file or `env` block it created), keeps a key you changed since, and stops the receiver.
- `status [--home <dir>]` reports the receiver (`disabled`, `listening`, `stopped`, `port-in-use`), the settings (`configured`, `partial`, `missing`, `unreadable`) and how many events are stored.

All three accept `--json` and run the same code as the `plus.obs.otel.enable`, `plus.obs.otel.disable` and `plus.obs.otel.status` IPC handlers (`home`, `port`, `dryRun` arguments).

The receiver runs inside the `toolport-gateway` process, which lives as long as a Claude Code session that uses Toolport. The gateway re-reads `obs/otel.json` every two seconds, so `enable` and `disable` take effect without a restart, and a second gateway on the same machine waits and takes over if the first one exits. If the port is taken by another program the gateway logs it once and retries every ten seconds; `status` shows `port-in-use`. Without a running gateway nothing receives, and Claude Code drops the export.

It binds `127.0.0.1` only and answers `POST /v1/metrics` and `POST /v1/logs` (`application/json`, optionally gzip, at most 4 MiB), `GET /healthz`, and nothing else. A request whose `Host` is not a loopback name is refused. Success returns `{"partialSuccess":{}}`; bad input returns 400, 404, 405, 411, 413, 415 or 431 with a `{"code","message"}` body. Request content is never logged.

`toolportctl usage` and `plus.obs.summary` merge the stored `api_request` events with the transcripts: a request that appears in both (same request id, or an id-less match on the same model and token counts within ten minutes) is counted once, and requests only seen through OTel are added to the totals. The summary reports the split under `sources`.

## Plugins and hooks

`plus/plugins/` is the one reader of Claude Code plugins and hooks (D-074, D-075). `toolportctl plugins ls [--cwd <dir>] [--refresh]`, `plugins show <id> [--cwd <dir>]`, `hooks ls [--cwd <dir>] [--tool Bash|Edit|Write|Read] [--event <name>] [--owner <kind>]` and the self-MCP tools `plugins_ls`, `plugins_show` and `hooks_ls` answer from it, and `cc list|update` and the `plugin` source detector use the same reader.

- **CLI first, files second.** `claude plugin list --json`, `plugin details` and `plugin configure --json` are the preferred source (`from: "claude-cli"`); with no `claude` on `PATH`, or when it fails (a warning says so), the plugin files are read (`from: "files"`) and the figures only the CLI knows are `null` (`cost.projected`, option `current` values from `plugin configure`). `TOOLPORT_CLAUDE_BIN` names another binary. What a plugin brings (skills, agents, commands, hooks, MCP servers, LSP servers) is always counted from its install folder, because `plugin details` counts command shims as skills.
- **Effective enablement** merges `enabledPlugins` of the user, project and local settings files of `--cwd` (the project files of the git top level too); managed settings win and are reported as a warning. A `--settings` flag of a running session cannot be seen from files. Without `--cwd` only the user layer is read.
- **Cost.** `cost.projected` is the always-on token figure of `plugin details`; `cost.measured` is the delta of the cached `context measure` for `without plugin:<id>` in that folder (`<data dir>/plus/cache/measure/`), `null` until one was measured.
- **MCP servers outside the gateway.** A plugin's `.mcp.json` servers start in Claude Code itself, not through Toolport; `mcpOutsideGateway` lists them keyed `plugin:<plugin>:<server>` and says whether `deniedMcpServers` denies each one. Commands show masked arguments and URLs without credentials; environment blocks are never read.
- **Options.** `plugins show` lists the manifest's `userConfig` with the current value from `pluginConfigs`; a sensitive option has no value in the output, only `sensitive` and `configured`.
- **`hooks ls` never runs anything.** It reads the user, project and local settings, the hooks files of effectively enabled plugins, the skills lock (`hooks_installed`), Toolport's own hooks (`--toolport-hook`, `--toolport-guard`, launch-profile files) and managed settings, and honours `disableAllHooks` (only managed hooks remain, unless managed settings set it too). A test runs the whole path over a sentinel script and checks that nothing starts.
- **Matchers** are an exact list (letters, digits, `_`, `-`, space, `,` and `|` only) or an unanchored regex; `*` and an empty matcher mean every tool. `counts.perTool` covers Bash, Edit, Write and Read for PreToolUse and PostToolUse, `counts.otherEvents` sums the other events, and `conflicts` names tools where hooks of two or more owners can answer the same PreToolUse (the strictest answer wins). Command text has the home folder shortened, the values of `NAME=value` prefixes dropped and credential-looking flags, headers and URL parts masked.

## Self-management MCP server

`toolport-plus-self` is the `toolport-selfmcp` binary registered as a stdio server: 85 tools and 11 `mcpm://` resources for skills, agents, styles, compression, servers, clients, sources, plugins, hooks and sync. `import mcpm`, `toolportctl mcp install` and `plus.selfmcp.ensure` register it and switch it on in the active (default) profile and in the profile of every connected client (`clientScopes`), so every client connected through the gateway sees its tools. A profile created later (a new client) gets it the next time one of those runs. The binary must sit next to `toolportctl` or in `<data dir>/bin`; `toolportctl mcp doctor` checks it.

**What it costs.** The 85 tool definitions are about 42 KB of JSON, roughly 10,000 tokens, in every client that lists all tools (full discovery, which import sets for Claude Code). A client in lazy discovery (the import default for every other client) lists none of them and pays only when it searches with `toolport_search_tools`. Exposed names are at most 46 characters (`toolport_plus_self__servers_remove_profile_tag`), which keeps `mcp__toolport__...` inside the 64 character limit of Claude Code. Tier 3 and tier 4 tools refuse unless `confirm` is true.

**Which tools preview by default.** The tap tools, `skills_install`, `client_direct_add|rm` and the lifecycle tools added after them take `dry_run` and default it to true: the call only reports what it would do until `dry_run` is passed as false. The later ones are `skills_bundle`, `skills_unbundle`, `skills_clean`, `skills_uninstall`, `skills_resolve`, `agents_clean`, `agents_uninstall`, `styles_clean` and `compression_enable`, `compression_disable`, `compression_set_provider`, `compression_use`, `compression_sync` and `compression_seal`. For the tier 3 and tier 4 ones `confirm` is needed only to apply (`dry_run=false` and `confirm=true`). Each runs the same core as its `toolportctl` command, and the read-only counterparts are `skills_diff`, `skills_audit`, `agents_diff`, `agents_audit`, `agents_status`, `styles_diff`, `styles_status` and `compression_status`. `skills_bundle` only creates a new `.zip` and never overwrites a file, `skills_unbundle` refuses a bundle that holds files outside `skills/`, `rules/` and `mcpm-skills.yaml`, and `servers_auth` starts the server through the same screened launcher as the gateway (`auth::stdio::start`). The older write tools (`skills_sync`, `agents_sync`, `styles_sync_tier1`, `styles_apply`, `styles_remove`, `clients_sync`, `sync_push`) keep their own `dry_run` without a default. Not exposed, because they drive the engine or read a caller-chosen directory: `compression disable --teardown`, `compression sync --mcpm-root`, and `compression pin`, `presets`, `proxy`, `update`, `run`, `verify` and `ledger`.

**Turning it off.**

- Everywhere: `toolportctl mcp uninstall` removes the server and records the opt-out. Import and `plus.selfmcp.ensure` never add it again; `toolportctl mcp install` does.
- In one profile: switch the server off in that profile (the profile toggle in the app). A profile Toolport already switched it on in is never switched on again by import or `install`; `toolportctl mcp install --profile <id>` switches it on there on purpose.
- One client only: set that client to lazy discovery, or scope it to a profile without the server.

**Where the choice lives.** `registry.json`, top level: `"plus": {"selfmcp": {"enabledIn": ["default", ...], "optOut": true}}`. `enabledIn` lists the profiles it was switched on in once; `optOut` is written by `mcp uninstall`. Deleting the server in the app counts as an opt-out too.

`toolportctl mcp doctor` reports the state: `enabled`, `disabled` (switched off on purpose in a profile, healthy), `opted-out` (uninstalled, healthy), `not-enabled` and `missing` (both fail with exit code 1, and `toolportctl mcp install` repairs them).

The gateway removes every `TOOLPORT_*` variable from the servers it starts. If you move the data directory with `TOOLPORT_DATA_DIR`, give the self server the same variable in its own `env`, or its tools manage the default data directory instead.

## Direct client entries (opt-in)

By default every client reaches every server through the one gateway entry. For a few servers that is the wrong shape, for example a local stdio server a single client should start itself. `toolportctl client direct add <server> --client <id>` gives that client an entry named after the server, next to the gateway entry:

```
toolportctl client direct add odoo --client claude-code --dry-run   # plan, write nothing
toolportctl client direct add odoo --client claude-code
toolportctl client direct ls [--client claude-code]
toolportctl client direct rm odoo --client claude-code
```

The entry runs `toolportctl direct run <server id>`, never the server's own command, and holds no secret. The launcher reads the server's command, arguments and environment from the registry and the secret store when the client starts it, applies the same spawn checks as the gateway, removes `TOOLPORT_*` from the child's environment, hands over stdin and stdout and gives back the server's exit status (on Unix it replaces itself with the server, so nothing sits between the client and the server). If you moved the data directory with `TOOLPORT_DATA_DIR`, that one variable is repeated in the entry's `env`; the encrypted secret file's `TOOLPORT_SECRET_KEY` never is, so a client that uses it must have it in its own environment.

**What you give up.** A direct entry bypasses the gateway. Its tools are not covered by profile tool scopes, approvals (HITL), receipts or lazy discovery, and every client with a direct entry starts its own process of the server instead of sharing one. `direct add` prints this, and so does its dry run.

**What is allowed.** Local stdio servers only. A remote (http, sse) server, a server whose sign-in Toolport manages (OAuth) and a server with a `${ROOT}` working directory only work through the gateway; `direct add` refuses them with the reason, exits with code 1 and writes nothing. An existing entry with the same name that Toolport did not write needs `--force`.

**Ownership.** The registry records every entry it wrote under `plus.directEntries.<client>.<entry>`. Because of that record:

- `client sync` leaves the entry alone, and still prunes foreign direct entries that duplicate or orphan registered servers.
- `client ls`, `status` and `doctor` report launcher entries; `client direct ls` gives each a state: `ok`, `stale` (the launcher binary moved), `customized` (edited after it was written; `rm` and `add` need `--force`), `missing` (recorded, gone from the client), `orphan` (the server was removed) or `unrecorded` (launcher-shaped but not recorded).
- `server uninstall` removes the entries in every client, and `client import` does not import them back.
- Writes use the normal client writers: a backup first, every other key and server kept, every supported format, and JSON comments kept where the format has them.

The desktop handlers are `plus.client.directAdd`, `plus.client.directRm` and `plus.client.directLs` (arguments `server`, `client`, `force`, `dryRun`); the self-management server has `client_direct_ls` (tier 1) and `client_direct_add` and `client_direct_rm` (tier 2). Like the tap tools, the two writing tools plan only unless `dry_run` is passed as false.

## Agents & styles

`src/plus/agents/` builds the two tabs of the approved mockup nav item "Agents & styles": `AgentsTab` and `StylesTab` (no props), and `AgentsScreen`, a host with both. The Library shell (MIG-GUI-3) mounts the same two panels as its Agents and Styles tabs; until it does, the Agents and Styles tabs of the Library placeholder lead to the `agents` view.

| Tab    | Reads                                                                                                                  | Writes (policy tier from `toolportctl commands`)                                                                                               |
| ------ | ---------------------------------------------------------------------------------------------------------------------- | ---------------------------------------------------------------------------------------------------------------------------------------------- |
| Agents | `agents ls`, `agents sync --dry-run` (the per-client output and the dropped fields), `status`, `lint`, `audit`, `diff` | `agents sync [--client]` (write), `agents add <name>` (write), `agents clean` and `agents uninstall <name>` (destructive: the phrase is typed) |
| Styles | `styles ls`, `status`, `lint`, `diff`                                                                                  | `styles add <name>`, `sync`, `apply <name>` (write), `styles remove` and `styles clean` (destructive: the phrase is typed)                     |

- Every write is the D-059 flow: the command's own `--dry-run`, a plan worded from its answer (`plans.ts`: these commands answer in their own shape, not as `data.plan`; a `data.plan` wins when a command starts sending one), the confirmation, the apply in `JobProgress`, then every read of the tab runs again. A failed preview never applies, and a command the registry does not classify is refused with a message.
- `--home` and `--path` are never sent: the app uses the configured skills repository and the real home. The dropped fields come from the warnings of `agents sync --dry-run`, which writes nothing, so they show before the first sync.
- Lint, audit and `status --strict` exit 1 with a normal `data`; the panels read that data instead of showing an error.
- Editing a body has no CLI command: the button is disabled and says it waits for `mcp call agents_edit_body` and `styles_edit_body` (MIG-GUI-14), with the path and Copy path next to it.
- The Styles tab starts empty with one action, "Create your first style", and nothing else to press.
- Tests: `testkit.ts` is a fake `plus_ctl` bridge over `src/plus/fixtures/agents.ts` (a command without a reply fails the test). `createBridge({ world: true })` and the dev browser fixture use `src/plus/fixtures/agentsWorld.ts` instead, where an applied write changes the next read: a sync writes the outputs, a clean removes them but keeps the lockfile (so the drift shows), a created style appears in the list. `AgentsTab.e2e.test.tsx` and `StylesTab.e2e.test.tsx` walk the real screen through that world, one test per parity action id; `agentsWorld.test.ts` checks every world reply against the golden shapes.

Screenshots (1280x800, from `npm run screenshots:gui`): `docs/assets/gui-agents-light.png` and `gui-agents-dark.png` (one agent in four clients with the dropped `tools` of Codex CLI and Cursor), `gui-agents-plan-light.png` (the sync plan with its warnings), `gui-agents-clean-light.png` (the typed confirmation of a clean), `gui-styles-empty-light.png` (no styles yet) and `gui-styles-light.png`, `gui-styles-dark.png` (a style synced to the native clients and always on in four others).

## Login health

`toolportctl auth probe` runs the due probes, `--force` ignores the cache and `--server <id>` limits the run to one server; it writes the same cache as the gateway due-scan. That scan runs from the registry-watch loop of every long-lived gateway mode, on its own thread, at most once a minute after a 60 second delay (`TOOLPORT_AUTH_SCAN=off` disables it), so probes never hold up requests.

`toolportctl auth login <server>` is the fix the status surfaces point at. A remote OAuth server runs the gateway browser flow; a stdio server runs `<command> <args> auth` and prints the consent URL it prints (`--no-open` leaves the browser to you). A server that signs in with an API token or client credentials gets the next step instead. A stdio server joins the probes by opting in with `"plus": {"authProbe": {"kind": "stdio"}}` in its registry entry.

The app shows the same rows under Settings as "Sign-in health" (`plus.auth.rows`). A reauth or reconsent row signs in through `plus.auth.login`, the same core as `toolportctl auth login` and it opens the browser; a retry row runs the `plus.auth.probe` route it carries; a misconfigured row only names what to check. Each fix reloads the rows. While the window is visible, `AuthNotifier` asks `plus.auth.notifications` once a minute and raises a toast for each login that just moved to needs-reauth or expiring and is still in that state. A login is announced once per six hours; what was announced lives in `auth/notified.json` next to `status.json`. There is no operating-system notification yet.

## Logins & secrets

`src/plus/logins/` builds the screen of the approved mockup nav item "Logins & secrets": three tabs, `LoginsTab`, `SecretsTab` and `IntegrationsTab` (props `{ onOpenCommands?, pollMs? }`), and `LoginsScreen`, a host with the three of them. The Servers shell (MIG-GUI-1) mounts the same three panels as its tabs; until it does, the `logins` view is where the AUTH-5 notification (Review), the "Sign-in health" button under Settings and the All commands page lead. The Servers screen (`src/plus/servers/ServersScreen.tsx`) mounts the three panels as its Logins, Secrets and Integrations tabs; the `logins` view (`LoginsScreen`) stays for the notification and the Settings link, and the `logins` route of the manifest points at the Servers entry.

| Tab          | Reads                                                           | Writes (policy tier)                                                                                                           |
| ------------ | --------------------------------------------------------------- | ------------------------------------------------------------------------------------------------------------------------------ |
| Logins       | `status` (auth rows), `server ls`, one `server info` per server | `auth login <server> [--no-open]` (write, a held run with Cancel), `auth probe [--server <id>] [--force]` (read), `secret set` |
| Secrets      | the same, plus one `secret get <server> <key>` per declared key | `secret set` (stdin), `secret get --reveal` and `secret rm` (tier from `commands`; `rm` is typed when destructive)             |
| Integrations | `auth statusline`, `auth hook`                                  | none; copy buttons for the output and the Claude Code snippets                                                                 |

- The CLI has no per-server login kind and no `secret ls`: a server that declares a secret key is an API-token row, any other server with a login is an OAuth row, and presence is one `secret get` per key (not found means unset). Rows are ordered broken first.
- A secret value never sits in React state or in the DOM: `SecretField` hands it to `secret set` on stdin and is empty afterwards. A reveal asks first (a plain confirmation, or the key typed when the tier of `secret get` in `toolportctl commands` is `destructive`), shows the value for 10 seconds (Hide now, Escape, a hidden window and the timer all drop it) and is never read while a list is shown. `secret set` and `secret rm` have no dry run: the dialogs show a plan written locally and the exact command line.
- Sign-in runs as a held job: stderr streams into the dialog with the consent address and a Copy button, "Don't open a browser" restarts it with `--no-open`, Cancel kills the child and Escape never stops it. A token server answers `unsupported` and the dialog offers Set secret.
- Every tab has a loading skeleton, an empty state, the CLI's error with Retry, a stale-data note and an offline state (`toolportctl` itself cannot run) with a link to the doctor. The login state is polled every 15 seconds while the window is visible.
- Tests: `testkit.ts` is a fake `plus_ctl` bridge with a reply for every argv of the fixture world (`src/plus/fixtures/logins.ts`; a command without a reply fails the test) and `harness.tsx` renders a tab or the screen against it. `canary.test.tsx` watches every DOM mutation, toast, storage entry and IPC payload for a typed value and for a stored one.

Screenshots (1280x800, from `npm run screenshots:gui`): `docs/assets/gui-logins-light.png` and `gui-logins-dark.png` (the list), `gui-logins-signin-light.png` (sign-in waiting for the browser), `gui-secrets-light.png`, `gui-secrets-set-light.png`, `gui-secrets-reveal-light.png` (the confirmation, never the value) and `gui-integrations-light.png`.

## Environment variables

| Variable                                                                                                                                                                                                                                       | Effect                                                                                                               |
| ---------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------- | -------------------------------------------------------------------------------------------------------------------- |
| `TOOLPORT_SECRET_KEY`                                                                                                                                                                                                                          | Enables the headless encrypted-file secrets backend (key is the SHA-256 of the value). Tests use a synthetic value.  |
| `TOOLPORT_SECRETS_BACKEND`                                                                                                                                                                                                                     | `login-keychain` selects the macOS login-keychain master key (also a `secrets-backend` marker file in the data dir). |
| `TOOLPORT_ALLOW_BARE_SECRET_ENV`                                                                                                                                                                                                               | Legacy: resolve secrets from bare process env names.                                                                 |
| `TOOLPORT_CORP_TOOLS_DIR`                                                                                                                                                                                                                      | Directory of a corporate tools checkout the context engine coexists with.                                            |
| `TOOLPORT_CLIENTS_ROOT`                                                                                                                                                                                                                        | Root directory holding client repositories for context deploy.                                                       |
| `TOOLPORT_AUTH_SCAN`                                                                                                                                                                                                                           | `0`, `off` or `false` turns off the gateway due-scan of login health (default on).                                   |
| `TOOLPORT_SKILL_ASSET_EXTENSIONS`                                                                                                                                                                                                              | Extra file extensions copied as skill assets.                                                                        |
| `TOOLPORT_CLAUDE_BIN`                                                                                                                                                                                                                          | Claude binary used by `cc` list/update.                                                                              |
| `TOOLPORT_DATA_DIR`, `TOOLPORT_REGISTRY`                                                                                                                                                                                                       | Data directory and registry path overrides.                                                                          |
| `TOOLPORT_LOCK_TIMEOUT_MS`                                                                                                                                                                                                                     | Lock wait (default 5 s).                                                                                             |
| `TOOLPORT_PROFILE`, `TOOLPORT_CLIENT_ID`, `TOOLPORT_ROOT`                                                                                                                                                                                      | Gateway profile, client identity and root selection.                                                                 |
| `TOOLPORT_HTTP_TOKEN`, `TOOLPORT_RESULT_BUDGET`, `TOOLPORT_NO_DIRECT_SPAWN`, `TOOLPORT_DEBUG`, `TOOLPORT_CODE_MODE`, `TOOLPORT_SEMANTIC`, `TOOLPORT_EMBED_*`, `TOOLPORT_METRICS`, `TOOLPORT_GATEWAY_TOPOLOGY`, `TOOLPORT_DAEMON_IDLE_GRACE_MS` | Upstream gateway tuning, see `docs/configuration.md`.                                                                |

Legacy `CONDUIT_*` names are still read through `brand::env_var`.

## Test conventions

- Anything touching the data dir takes `registry::data_dir_test_lock()` and sets `registry::DataDirOverride`. `plus::testutil::DataDirFx` does both, creates a fresh temp dir, and removes it on drop; wrap it (with `Deref`) to add per-area setup.
- Fixture field order matters: fields drop in declaration order, so the override must be declared before the lock guard. `DataDirFx` encodes this once; do not reorder.
- Per-thread overrides (`clients::TEST_HOME`, `selfmcp` `TEST_REPO`) must be cleared in the fixture's `Drop`, which runs before its fields drop.
- Tests that read or set process-global env (`XDG_*`, `CLAUDE_CONFIG_DIR`, `TOOLPORT_LOCK_TIMEOUT_MS`) hold `clients::env_test_lock()`, taken before the data-dir lock.
- The data-dir override is process-global. A libtest thread that does not hold the lock resolves the real data dir while another test holds it, so it cannot leave `gateway.log`, `secret-service.lock` or similar files in that test's dir; threads a test spawns itself keep seeing its override. A test that spawns its own threads and touches the data dir must therefore hold `DataDirFx`.
- Executable fixture scripts go through `write_executable` (`tests/common/exec.rs`; lib tests use `plus::testutil::exec`, integration tests include it with `#[path]`). It returns only once the file runs, because a script written while another thread forks fails with `ETXTBSY`.
- `tests/one_gateway_conformance.rs` counts gateway daemons and mock servers by this build's own binary path, so gateway tests running from another target dir or slot on the same host do not disturb it. Two runs of that suite from the _same_ target dir at the same time are unsupported.
- Secrets tests use `secrets::tests::with_isolated_vault` (isolated vault dir plus the secret-key env lock) and synthetic keys only; never real credentials.
- Never run `cargo fmt` on the tree; format only new files. Run suites with `cargo test --no-default-features --tests --no-fail-fast` and `npm run verify`.
- Use synthetic fixtures and `.invalid` hosts; no real organization or customer names.
