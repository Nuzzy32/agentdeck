import { Menu } from "@tauri-apps/api/menu";

/**
 * Tauri's default macOS menu binds ⌘W to Close Window, so the key never reaches the page and ⌘W would close the app
 * instead of a pane. This is the same menu without Close Window; Edit stays so ⌘C/⌘V keep working in the WebView.
 */
export async function installMenu(): Promise<void> {
  const menu = await Menu.new({
    items: [
      {
        text: "agentdeck",
        items: [
          { item: { About: null } },
          { item: "Separator" },
          { item: "Services" },
          { item: "Separator" },
          { item: "Hide" },
          { item: "HideOthers" },
          { item: "ShowAll" },
          { item: "Separator" },
          { item: "Quit" },
        ],
      },
      {
        text: "Edit",
        items: [
          { item: "Undo" },
          { item: "Redo" },
          { item: "Separator" },
          { item: "Cut" },
          { item: "Copy" },
          { item: "Paste" },
          { item: "SelectAll" },
        ],
      },
      { text: "View", items: [{ item: "Fullscreen" }] },
      { text: "Window", items: [{ item: "Minimize" }, { item: "Maximize" }] },
    ],
  });
  await menu.setAsAppMenu();
}
