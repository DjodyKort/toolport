import { lazy } from "react";
import { Guard } from "../Guard";
import AccessCompare, { type AccessCompareProps } from "../servers/AccessCompare";

const AccessWrite = lazy(() => import("./AccessWrite"));

/** What the Access page of Settings gets from the fork: the compare table, then the editor for
 * the sets, the clients and the folder routes. */
export default function AccessSection(props: AccessCompareProps) {
  return (
    <>
      <AccessCompare {...props} />
      <Guard name="The access set editor">
        <AccessWrite {...props} />
      </Guard>
    </>
  );
}
