import { invoke } from "@tauri-apps/api/core";
import { $, el, showScreen, toast } from "./dom";
import { pickRetroArch } from "./library";
import { icon } from "./ui/icons";
import type { GameView } from "./types";

export async function openGame(id: number): Promise<void> {
  try {
    const view = await invoke<GameView>("get_game", { id });
    renderGame(view);
    showScreen("game");
  } catch (error) {
    toast(String(error));
  }
}

/** Why Play will not start this game, or null when it will. */
function playBlockedBecause(
  view: GameView,
): { reason: string; choose: boolean } | null {
  if (view.core.unsupported) {
    return { reason: view.core.unsupported, choose: false };
  }
  if (!view.retroarch.available) {
    return { reason: "RetroArch was not found.", choose: true };
  }
  if (view.core.installed === false) {
    // RetroArch is here; the core for this system is not. Said in the same
    // place and the same voice, because to somebody holding a controller it
    // is the same problem: this game will not start yet.
    const named = view.core.name || "the";
    return {
      reason:
        `RetroArch has no ${named} core yet. Open RetroArch, then ` +
        `Main Menu → Online Updater → Core Downloader.`,
      choose: false,
    };
  }
  return null;
}

function renderGame(view: GameView): void {
  const layout = $<HTMLElement>("game-layout");
  layout.replaceChildren();

  const art = el("div", "game-art");
  art.appendChild(el("div", "no-art", view.game.title));

  const meta = el("div", "game-meta");
  meta.appendChild(el("h1", undefined, view.game.title));
  meta.appendChild(el("div", "system-word", view.game.system));

  const playRow = el("div", "play-row");
  const play = el("button", "primary", "Play");
  const playIcon = icon("play");
  if (playIcon) play.prepend(playIcon);
  // Say it before the press, not after it. A button that looks ready and then
  // answers with an error is the interface withholding what it already knew.
  // One reason, in one place, so the button and the sentence beside it can
  // never disagree about why it will not start.
  const blocked = playBlockedBecause(view);
  play.disabled = blocked !== null;
  play.addEventListener("click", () => {
    void (async () => {
      try {
        const info = await invoke<{ pid: number; core: string }>("launch_game", { id: view.game.id });
        toast(`Launched with ${info.core}`);
      } catch (error) {
        toast(String(error));
      }
    })();
  });
  playRow.appendChild(play);
  if (blocked) {
    // The reason is tied to the button, not merely placed near it, so a
    // screen reader announces why it is disabled rather than just that it is.
    const note = el("span", "quiet play-note", blocked.reason);
    note.id = `play-reason-${view.game.id}`;
    play.setAttribute("aria-describedby", note.id);
    playRow.appendChild(note);
    if (blocked.choose) {
      const link = el("button", "ghost", "Choose RetroArch…");
      link.type = "button";
      link.addEventListener("click", () => {
        void (async () => {
          await pickRetroArch();
          // Re-read the screen so the button comes back enabled.
          await openGame(view.game.id);
        })();
      });
      playRow.appendChild(link);
    }
  }
  meta.appendChild(playRow);

  if (view.saves.length > 0) {
    const field = el("div", "field");
    field.appendChild(el("h2", "section-label", "Saves"));
    const list = el("ul", "extra-list");
    for (const save of view.saves) {
      list.appendChild(el("li", undefined, `${save.kind} · ${new Date(save.created_at * 1000).toLocaleString()}`));
    }
    field.appendChild(list);
    meta.appendChild(field);
  }

  const field = el("div", "field");
  field.appendChild(el("h2", "section-label", "Details"));
  const list = el("ul", "extra-list");
  list.appendChild(el("li", undefined, view.game.path));
  if (view.game.playtime > 0) {
    list.appendChild(el("li", undefined, `${Math.round(view.game.playtime / 60)} minutes played`));
  }
  field.appendChild(list);
  meta.appendChild(field);

  layout.appendChild(art);
  layout.appendChild(meta);
}
