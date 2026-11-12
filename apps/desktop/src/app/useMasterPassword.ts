import { useCallback, useEffect, useState } from "react";
import { masterStatus } from "../terminal/api";

/** Master password state (ADR-032): saved passwords start locked when one is set. */
export function useMasterPassword() {
  const [unlockOpen, setUnlockOpen] = useState(false);
  const [masterProtected, setMasterProtected] = useState(false);
  const refreshMaster = useCallback(() => {
    masterStatus().then((s) => setMasterProtected(s.protected), () => {});
  }, []);
  useEffect(() => {
    masterStatus().then((s) => {
      setUnlockOpen(s.locked);
      setMasterProtected(s.protected);
    }, () => {});
  }, []);
  return { unlockOpen, setUnlockOpen, masterProtected, refreshMaster };
}
