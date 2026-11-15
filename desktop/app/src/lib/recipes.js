// Starting a recipe's service, from ⌘K or the Containers page.
import { clipboard, dialog, navigate, toast } from "@elyra/runtime";
import { api } from "../bindings";

export async function startRecipe(r, name = null, folder = null) {
  // An app server serves a folder from the Mac: asked for when not given.
  if (r.needs_folder && !folder) {
    const [dir] = await dialog.open({ title: `The project ${r.title} serves`, directory: true });
    if (!dir) return;
    folder = dir.path;
  }
  const pending = toast(`Starting ${r.title}… (the first time, its image is pulled)`, { duration: 0 });
  try {
    const s = await api.recipe_start(r.id, name || null, folder || null);
    pending.dismiss();
    await clipboard.writeText(s.connect).catch(() => {});
    toast(`${r.title} runs as ${s.container}: ${s.connect} (copied)${s.ui ? `. Its UI: ${s.ui}` : ""}`, { variant: "success", duration: 9000 });
    navigate("/");
    return s;
  } catch (e) {
    pending.dismiss();
    toast(e?.message ?? String(e), { variant: "error", duration: 6000 });
  }
}
