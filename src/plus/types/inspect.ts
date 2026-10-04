import { arr, nullable, obj, str, type Infer, type Shape } from "../bridge/shape";

/** `data` of the inspect commands, checked against the golden envelopes by `data.test.ts`. */

export const inspectData = obj({
  profile: nullable(str),
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
export type InspectData = Infer<typeof inspectData>;

/** Golden file stem to the shape of its envelope `data`. */
export const inspectShapes: Record<string, Shape<unknown>> = {
  inspect: inspectData,
};
