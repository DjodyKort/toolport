import { plusAuthRowsFixture } from "./authRows";
import { plusWhatLoadsFixture } from "./whatLoads";

/** Replies the dev browser fixture gives each `plus_invoke` command. A new `plus.*`
 * command needs a row here or the fixture rejects it as unimplemented. */
export const plusInvokeFixtures = new Map<string, unknown>([
  [
    "plus.ping",
    { name: "toolport-plus", version: "0.0.0-fixture", forkEgressDisabled: true },
  ],
  ["plus.auth.rows", plusAuthRowsFixture],
  ["plus.auth.notifications", { notifications: [] }],
  ["plus.auth.probe", { server: "odoo", ran: true, skipped: null }],
  [
    "plus.auth.login",
    {
      server: "figma",
      name: "figma",
      flow: "browser",
      consentUrl: null,
      signedIn: true,
      message: "Signed in to figma.",
    },
  ],
  ["plus.context.whatLoads", plusWhatLoadsFixture],
  [
    "plus.context.folderProfiles",
    {
      enabled: false,
      mappings: [{ path: "/proj/work", profile: "Work" }],
      folders: [
        {
          root: "/proj/work/app",
          applies: false,
          profile: null,
          wouldApply: "Work",
          rule: "/proj/work",
          reason: "mapping /proj/work matches but folder profiles are disabled",
          launchProfile: null,
          tokens: 120,
        },
      ],
    },
  ],
  ["plus.context.folderProfilesSet", { enabled: true }],
]);
