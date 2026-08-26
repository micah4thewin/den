import { refreshControllers } from "./controllers";
import { $, el, showScreen, toast } from "./dom";
import { chooseAndIntake, wireDrop } from "./intake";
import { renderLibrary } from "./library";
import { icon } from "./ui/icons";
import { invoke } from "@tauri-apps/api/core";

function injectIcons(): void {
  for (const target of document.querySelectorAll<HTMLElement>("[data-icon]")) {
    const name = target.dataset.icon;
    if (!name) continue;
    const glyph = icon(name);
    if (glyph) target.replaceWith(glyph);
  }
  const mark = icon("brandDen");
  if (mark) $<HTMLElement>("brand-mark").appendChild(mark);
}

function wireNavigation(): void {
  const nav = $<HTMLElement>("nav");
  nav.replaceChildren();
  for (const [screen, label, glyph] of [
    ["library", "Library", "box"],
    ["intake", "Intake", "folder"],
    ["controllers", "Controllers", "gamepad"],
  ] as const) {
    const button = el("button", undefined);
    button.type = "button";
    button.dataset.screen = screen;
    const glyphNode = icon(glyph);
    if (glyphNode) button.appendChild(glyphNode);
    button.appendChild(el("span", undefined, label));
    button.addEventListener("click", () => {
      showScreen(screen);
      if (screen === "library") void renderLibrary();
      if (screen === "controllers") void refreshControllers();
    });
    nav.appendChild(button);
  }

  $<HTMLButtonElement>("btn-refresh").addEventListener("click", () => void renderLibrary());
  $<HTMLButtonElement>("btn-back").addEventListener("click", () => showScreen("library"));
  $<HTMLButtonElement>("btn-choose").addEventListener("click", () => void chooseAndIntake());
  $<HTMLButtonElement>("btn-pads").addEventListener("click", () => void refreshControllers());
}

async function showRemoteUrl(): Promise<void> {
  try {
    const urls = await invoke<string[]>("web_remote_urls");
    if (urls.length > 0) {
      $<HTMLElement>("remote-url").textContent = `On this network: ${urls[0]}`;
    }
  } catch {
    // The shelf works the same with or without the remote.
  }
}

async function boot(): Promise<void> {
  injectIcons();
  wireNavigation();
  await wireDrop();
  showScreen("library");
  void showRemoteUrl();
  try {
    await renderLibrary();
  } catch (error) {
    toast(String(error));
  }
}

void boot();
