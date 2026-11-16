const REFUSED_HOSTS = ["toolport.app"];
const REFUSED_GITHUB_OWNER = "btsouth";

export const UPSTREAM_DISABLED_MESSAGE =
  "Upstream hosted services are disabled in this build.";

// Vitest runs in mode "test", so upstream's own tests keep covering the guarded
// code; the fork's tests stub MODE to exercise the guard.
export function upstreamEgressDisabled(): boolean {
  return import.meta.env.MODE !== "test";
}

function isUpstreamUrl(url: string): boolean {
  let parsed: URL;
  try {
    parsed = new URL(url.trim());
  } catch {
    return false;
  }
  const host = parsed.hostname.toLowerCase().replace(/\.$/, "");
  if (REFUSED_HOSTS.some((domain) => host === domain || host.endsWith(`.${domain}`)))
    return true;
  return (
    host === "github.com" &&
    parsed.pathname.toLowerCase().startsWith(`/${REFUSED_GITHUB_OWNER}/`)
  );
}

export function isEgressRefused(url: string): boolean {
  return upstreamEgressDisabled() && isUpstreamUrl(url);
}

export function refuseUpstreamUrl(url: string): boolean {
  if (!isEgressRefused(url)) return false;
  console.warn(`${UPSTREAM_DISABLED_MESSAGE} Not opening ${url}`);
  return true;
}
