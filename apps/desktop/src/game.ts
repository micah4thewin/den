import { invoke } from "@tauri-apps/api/core";
import { $, el, showScreen, toast } from "./dom";
import { pickRetroArch } from "./library";
import { icon } from "./ui/icons";
import type { GameView, SetupStatus, Stopped } from "./types";

export async function openGame(id: number): Promise<void> {
  try {
    const [view, setup] = await Promise.all([
      invoke<GameView>("get_game", { id }),
      invoke<SetupStatus>("setup_status"),
    ]);
    renderGame(view, setup);
    showScreen("game");
  } catch (error) {
    toast(String(error));
  }
}

function playBlockedBecause(
  view: GameView,
  setup: SetupStatus,
): { reason: string; choose: boolean } | null {
  if (view.core.unsupported) {
    return { reason: view.core.unsupported, choose: false };
  }
  if (!view.retroarch.available) {
    if (setup.working) {
      return { reason: `Play is still getting ready: ${setup.doing ?? "downloading"}.`, choose: false };
    }
    return { reason: "RetroArch was not found.", choose: true };
  }
  // A missing core is not a block: Play downloads it on the way in.
  return null;
}

function renderGame(view: GameView, setup: SetupStatus): void {
  const layout = $<HTMLElement>("game-layout");
  layout.replaceChildren();

  const art = el("div", "game-art");
  art.appendChild(el("div", "no-art", view.game.title));

  const meta = el("div", "game-meta");
  meta.appendChild(el("h1", undefined, view.game.title));
  meta.appendChild(el("div", "system-word", view.game.system));

  const playRow = el("div", "play-row");
  const play = el("button", "primary", view.playing ? "Playing" : "Play");
  const playIcon = icon("play");
  if (playIcon) play.prepend(playIcon);
  const blocked = playBlockedBecause(view, setup);
  // One game, one process: while it is playing there is a Stop, not a Play.
  play.disabled = blocked !== null || view.playing;
  play.addEventListener("click", () => {
    void (async () => {
      play.disabled = true;
      if (view.core.installed === false) play.lastChild!.textContent = "Getting ready…";
      try {
        const info = await invoke<{ pid: number; core: string }>("launch_game", { id: view.game.id });
        toast(`Launched with ${info.core}`);
        await openGame(view.game.id);
      } catch (error) {
        toast(String(error));
        await openGame(view.game.id);
      }
    })();
  });
  playRow.appendChild(play);

  if (view.playing) {
    const stop = el("button", "ghost", "Stop");
    stop.type = "button";
    stop.addEventListener("click", () => {
      void (async () => {
        try {
          const answer = await invoke<Stopped>("stop_game", { id: view.game.id });
          toast(answer.stopped > 0 ? `Stopped ${view.game.title}` : "It had already finished");
          await openGame(view.game.id);
        } catch (error) {
          toast(String(error));
        }
      })();
    });
    playRow.appendChild(stop);
  }
  if (!blocked && view.core.installed === false && !view.playing) {
    playRow.appendChild(
      el("span", "quiet play-note", `The ${view.core.name} core downloads the first time you play.`),
    );
  }
  if (blocked) {
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
