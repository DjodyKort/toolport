import { failure, goldenData, type Bridge } from "./testkit";

/** The reads of the tabs This folder, Profiles and Layers over the real golden envelopes, for
 * the tests that drive the screen against a plain (not stateful) bridge. A test replaces a
 * reply with `bridge.set`. The folders are synthetic: the goldens only supply the shapes. */
export const HOME_DIR = "/fixture/home";
export const FOLDER = "/fixture/work/erp/clients/acme-erp";
export const OTHER = "/fixture/work/erp/clients/acme-two";

export const CANARY = "sk-canary-0123456789abcdef";

export const loadsHome = () => ({ ...goldenData("context-loads.home"), cwd: HOME_DIR });
export const loadsFolder = (cwd = FOLDER) => ({
  ...goldenData("context-loads.folder"),
  cwd,
});
export const composeFor = (cwd = FOLDER) => ({
  ...goldenData("context-compose.layers"),
  cwd,
});
export const bundleList = () => {
  const data = goldenData("context-bundle-ls.applied");
  data.bundles[0].appliedTo[0].folder = FOLDER;
  return data;
};
export const bundleShow = (name = "acme-dev") => {
  const show = goldenData(
    name === "default" ? "context-bundle-show.legacy" : "context-bundle-show.bundle",
  );
  const row = bundleList().bundles.find((one: { name: string }) => one.name === name);
  return { ...show, name, appliedTo: row?.appliedTo ?? [], yaml: `# ${CANARY}\n` };
};
export const layerList = () => goldenData("context-client-add.list");
export const orgSources = () => {
  const data = goldenData("sources-ls.org");
  data.sources[0].freshness.lastSync = "2026-10-05T04:00:00Z";
  return { ...data, items: [] };
};

export function seedHere(bridge: Bridge) {
  bridge.set("context loads --measured", loadsHome());
  bridge.set(`context loads --cwd ${FOLDER} --measured`, loadsFolder());
  bridge.set(`context loads --cwd ${OTHER} --measured`, loadsFolder(OTHER));
  for (const cwd of [HOME_DIR, FOLDER, OTHER]) {
    bridge.set(`context compose --cwd ${cwd}`, composeFor(cwd));
    bridge.set(
      `context bundle status --cwd ${cwd}`,
      goldenData("context-bundle-status.none"),
    );
  }
  bridge.set("context bundle ls", bundleList());
}

export function seedProfiles(bridge: Bridge) {
  bridge.set("context bundle ls", bundleList());
  for (const name of ["acme-dev", "default"]) {
    bridge.set(`context bundle show ${name}`, () => bundleShow(name));
  }
  bridge.set(
    "context bundle show broken",
    failure("invalid", "bundle broken: skills must be a mapping"),
  );
  bridge.set("context bundle config", goldenData("context-bundle-config.show"));
}

export function seedLayers(bridge: Bridge) {
  bridge.set("context client list", layerList());
  bridge.set("sources ls --source org", orgSources());
  for (const cwd of [HOME_DIR, FOLDER, OTHER]) {
    bridge.set(`context compose --cwd ${cwd}`, composeFor(cwd));
  }
}
