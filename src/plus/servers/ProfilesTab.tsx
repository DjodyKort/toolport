import { useState } from "react";
import { Info, Layers, Plus } from "lucide-react";
import { Button } from "@/components/ui/button";
import { Callout } from "@/components/Callout";
import { EmptyState } from "@/components/ui/empty-state";
import type { ProfileCreateData, ProfileEditData, ProfileRmData } from "../bridge/data";
import { Avatar, Gate, Pill } from "./atoms";
import { useWrite } from "./useWrite";
import { WriteDialogs } from "./WriteDialogs";
import { profileUsers, type ProfileData } from "./model";
import { profileCreatePlan, profileEditPlan, profileRmPlan } from "./plans";
import {
  CreateProfileDialog,
  DeleteProfileDialog,
  EditProfileDialog,
  InspectProfileDialog,
} from "./ProfileDialogs";
import { useServers } from "./useServers";

function usedBy(profile: ProfileData, users: string[]): string {
  const list = users.join(", ");
  if (profile.active) {
    return list
      ? `Used by clients without their own profile: ${list}`
      : "Used by clients without their own profile";
  }
  return list ? `Used by ${list}` : "Not used by any client";
}

export function ProfilesTab() {
  const { profiles, servers, clients, views, go } = useServers();
  const write = useWrite();
  const [creating, setCreating] = useState(false);
  const [editing, setEditing] = useState<ProfileData | null>(null);
  const [deleting, setDeleting] = useState<ProfileData | null>(null);
  const [inspecting, setInspecting] = useState<ProfileData | null>(null);
  const names = profiles.data?.profiles.map((profile) => profile.name) ?? [];
  const usersOf = (profile: ProfileData) =>
    profileUsers(profile, profile.active, clients.data?.clients ?? null);

  return (
    <div className="flex flex-col gap-4">
      <Gate
        queries={[profiles, servers]}
        title="Couldn't load the profiles"
        context="profile ls, server ls"
      >
        {() => {
          const list = profiles.data?.profiles ?? [];
          return (
            <>
              <div className="flex items-center justify-between gap-2">
                <p className="text-sm text-muted-foreground">
                  A profile is a named set of servers. A client uses the one it is pointed
                  at, or the active one.
                </p>
                <Button onClick={() => setCreating(true)}>
                  <Plus /> New profile
                </Button>
              </div>
              {list.length === 0 ? (
                <EmptyState
                  icon={<Layers />}
                  title="No profiles yet"
                  description="Create one, then add servers to it."
                  action={
                    <Button onClick={() => setCreating(true)}>
                      <Plus /> New profile
                    </Button>
                  }
                />
              ) : (
                <ul aria-label="Profiles" className="flex flex-col rounded-lg border">
                  {list.map((profile) => (
                    <li
                      key={profile.id}
                      className="flex flex-wrap items-center gap-3 border-b px-3 py-2 last:border-b-0"
                    >
                      <Avatar name={profile.name} />
                      <div className="min-w-0 flex-1">
                        <p className="flex items-center gap-2 text-sm font-medium">
                          {profile.name}
                          {profile.active && <Pill>active</Pill>}
                        </p>
                        <p className="text-xs text-muted-foreground">
                          {usedBy(profile, usersOf(profile))}
                        </p>
                      </div>
                      <Pill>
                        {profile.servers.length} server
                        {profile.servers.length === 1 ? "" : "s"}
                      </Pill>
                      <div className="flex gap-2">
                        <Button
                          size="sm"
                          variant="outline"
                          aria-label={`Inspect ${profile.name}`}
                          onClick={() => setInspecting(profile)}
                        >
                          Inspect
                        </Button>
                        <Button
                          size="sm"
                          variant="outline"
                          aria-label={`Edit ${profile.name}`}
                          disabled={write.busy}
                          onClick={() => setEditing(profile)}
                        >
                          Edit
                        </Button>
                        <Button
                          size="sm"
                          variant="destructive"
                          aria-label={`Delete ${profile.name}`}
                          disabled={write.busy}
                          onClick={() => setDeleting(profile)}
                        >
                          Delete…
                        </Button>
                      </div>
                    </li>
                  ))}
                </ul>
              )}
              <Callout variant="info" className="flex items-start gap-2">
                <Info className="mt-0.5 size-4 shrink-0" aria-hidden="true" />
                <span>
                  Inspecting a whole profile skips a server that needs a login and reports
                  it, instead of stopping at the first 401.
                </span>
              </Callout>
            </>
          );
        }}
      </Gate>
      {creating && (
        <CreateProfileDialog
          open
          onOpenChange={setCreating}
          existing={names}
          onSubmit={(name) => {
            setCreating(false);
            write.begin({
              command: "profile create",
              title: `Create ${name}`,
              argv: ["profile", "create", name],
              adapt: (data, done) => profileCreatePlan(data as ProfileCreateData, done),
            });
          }}
        />
      )}
      {editing && (
        <EditProfileDialog
          profile={editing}
          servers={views ?? []}
          users={usersOf(editing)}
          existing={names}
          open
          onOpenChange={(open) => !open && setEditing(null)}
          onSubmit={(_, argv) => {
            const profile = editing;
            setEditing(null);
            write.begin({
              command: "profile edit",
              title: `Edit ${profile.name}`,
              argv,
              adapt: (data, done) =>
                profileEditPlan(data as ProfileEditData, done, {
                  users: usersOf(profile),
                }),
            });
          }}
        />
      )}
      {deleting && (
        <DeleteProfileDialog
          profile={deleting}
          users={usersOf(deleting)}
          open
          onOpenChange={(open) => !open && setDeleting(null)}
          onContinue={({ noClients }) => {
            const profile = deleting;
            setDeleting(null);
            write.begin({
              command: "profile rm",
              title: `Delete ${profile.name}`,
              confirmLabel: "Delete profile",
              phrase: profile.name,
              argv: ["profile", "rm", profile.id, ...(noClients ? ["--no-clients"] : [])],
              adapt: (data, done) =>
                profileRmPlan(data as ProfileRmData, done, { profile }),
            });
          }}
        />
      )}
      {inspecting && (
        <InspectProfileDialog
          profile={inspecting}
          onClose={() => setInspecting(null)}
          onOpenLogins={() => {
            setInspecting(null);
            go("logins");
          }}
        />
      )}
      <WriteDialogs write={write} />
    </div>
  );
}
