import { arr, obj, str, type Infer, type Shape } from "../bridge/shape";

/** `data` of the profile commands, checked against the golden envelopes by `data.test.ts`. */

export const profileInspectData = obj({
  profile: str,
  servers: arr(
    obj({
      id: str,
      tools: arr(
        obj({
          description: str,
          name: str,
        }),
      ),
    }),
  ),
});
export type ProfileInspectData = Infer<typeof profileInspectData>;

/** Golden file stem to the shape of its envelope `data`. */
export const profileShapes: Record<string, Shape<unknown>> = {
  "profile-inspect": profileInspectData,
};
