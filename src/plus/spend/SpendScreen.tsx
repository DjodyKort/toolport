import { useState } from "react";
import { isFiltered } from "./model";
import SpendPage from "./SpendPage";
import { useSpendDialogs } from "./useSpendDialogs";

/** The Spend page with its dialogs: every action that writes opens its plan or confirmation
 * first, and the page reads its status again when one finishes. */
export default function SpendScreen() {
  const [version, setVersion] = useState(0);
  const dialogs = useSpendDialogs(() => setVersion((v) => v + 1));
  return (
    <>
      <SpendPage
        version={version}
        onExport={(range, filter) => dialogs.exportCsv(range, isFiltered(filter))}
        onSetup={dialogs.setup}
        onUsePort={dialogs.usePort}
        onPrune={dialogs.prune}
        onUndo={dialogs.undo}
      />
      {dialogs.element}
    </>
  );
}
