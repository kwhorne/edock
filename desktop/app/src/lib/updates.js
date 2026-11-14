// "Check for Updates…": the runtime's toast shows a newer release and
// installs it; here, also say when there's none, or why the check failed.
import { checkForUpdate, toast } from "@elyra/runtime";

export async function checkForUpdates() {
  try {
    const r = await checkForUpdate();
    // A release without update information (eDock before 0.2.0) answers 404.
    if (r.error && /\b404\b/.test(r.error)) toast("eDock is up to date", { variant: "success" });
    else if (r.error) toast(`Couldn't check for updates: ${r.error}`, { variant: "danger", duration: 6000 });
    else if (!r.available && !r.ready) toast("eDock is up to date", { variant: "success" });
  } catch (e) {
    toast(`Couldn't check for updates: ${e?.message ?? e}`, { variant: "danger", duration: 6000 });
  }
}
