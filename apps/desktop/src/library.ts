import { invoke } from "@tauri-apps/api/core";
import { $, el, toast } from "./dom";
import { openGame } from "./game";
import type { Game, LibraryView, RetroArchStatus, SetupStatus } from "./types";

export async function renderLibrary(): Promise<void> {
  const [data, setup] = await Promise.all([
    invoke<LibraryView>("get_library"),
    invoke<SetupStatus>("setup_status"),
  ]);
  $<HTMLElement>("library-path").textContent = data.path;
  $<HTMLElement>("game-count").textContent =
    data.games.length === 1 ? "1 game" : `${data.games.length} games`;

  renderRetroArchNotice(data.retroarch, setup);
  watchSetup(setup);

  const continueBand = $<HTMLElement>("continue-band");
  const continueShelf = $<HTMLElement>("continue-shelf");
  continueShelf.replaceChildren();
  if (data.continue_game) {
    continueBand.hidden = false;
    continueShelf.appendChild(tile(data.continue_game));
  } else {
    continueBand.hidden = true;
  }

  const recentBand = $<HTMLElement>("recent-band");
  const recentShelf = $<HTMLElement>("recent-shelf");
  recentShelf.replaceChildren();
  const recent = data.recent.filter((g) => g.id !== data.continue_game?.id);
  if (recent.length > 0) {
    recentBand.hidden = false;
    for (const game of recent) recentShelf.appendChild(tile(game));
  } else {
    recentBand.hidden = true;
  }

  renderShelves(data);
}

let whereOpen = false;
let setupTimer: number | undefined;

/** While Play is downloading, look again every couple of seconds, and redraw
 * the shelf once when it finishes so the notice goes away by itself. */
function watchSetup(setup: SetupStatus): void {
  window.clearTimeout(setupTimer);
  if (!setup.working) return;
  setupTimer = window.setTimeout(() => {
    void (async () => {
      const next = await invoke<SetupStatus>("setup_status");
      if (next.working) {
        const retroarch = (await invoke<LibraryView>("get_library")).retroarch;
        renderRetroArchNotice(retroarch, next);
        watchSetup(next);
      } else {
        await renderLibrary();
      }
    })();
  }, 2000);
}

function renderSetup(notice: HTMLElement, setup: SetupStatus): void {
  if (setup.working) {
    notice.appendChild(
      el("p", "notice-word", "Getting Play ready to play on this machine. This happens once."),
    );
    if (setup.doing) notice.appendChild(el("p", "quiet", `${setup.doing}…`));
    if (setup.ready.length > 0) {
      notice.appendChild(el("p", "quiet", `Ready: ${setup.ready.join(", ")}`));
    }
  }
  if (setup.problems.length > 0) {
    const details = el("details", "notice-where") as HTMLDetailsElement;
    details.appendChild(
      el("summary", undefined, setup.problems.length === 1 ? "1 download failed" : `${setup.problems.length} downloads failed`),
    );
    const list = el("ul", "extra-list mono");
    for (const problem of setup.problems) list.appendChild(el("li", undefined, problem));
    details.appendChild(list);
    notice.appendChild(details);
  }
}

function searchAgainButton(): HTMLButtonElement {
  const clear = el("button", "ghost", "Use the automatic search again");
  clear.type = "button";
  clear.addEventListener("click", () => void resetRetroArch());
  return clear;
}

function renderRetroArchNotice(status: RetroArchStatus, setup: SetupStatus): void {
  const notice = $<HTMLElement>("retroarch-notice");
  const hadFocus = notice.contains(document.activeElement)
    ? (document.activeElement as HTMLElement).textContent
    : null;
  notice.replaceChildren();
  const quietSetup = !setup.working && setup.problems.length === 0;
  notice.hidden = status.available && !status.chosen && quietSetup;

  renderSetup(notice, setup);
  // Still downloading RetroArch: the setup lines say so, and asking the
  // person to find one by hand would be asking them to do Play's job.
  if (!status.available && setup.working) return;

  if (status.available) {
    if (!status.chosen) return;
    const row = el("div", "row");
    row.appendChild(el("span", "quiet", `RetroArch: ${status.path}`));
    row.appendChild(searchAgainButton());
    notice.appendChild(row);
    return;
  }

  notice.appendChild(
    el(
      "p",
      "notice-word",
      "Play can shelve and name games without RetroArch, but it needs RetroArch to play them.",
    ),
  );
  if (status.problem) notice.appendChild(el("p", "quiet", status.problem));

  const actions = el("div", "row");
  const choose = el("button", "primary", "Choose RetroArch…");
  choose.type = "button";
  choose.addEventListener("click", () => void pickRetroArch());
  actions.appendChild(choose);

  if (status.chosen) actions.appendChild(searchAgainButton());
  notice.appendChild(actions);

  if (status.searched.length > 0) {
    const details = el("details", "notice-where") as HTMLDetailsElement;
    details.open = whereOpen;
    details.addEventListener("toggle", () => {
      whereOpen = details.open;
    });
    details.appendChild(
      el(
        "summary",
        undefined,
        status.searched.length === 1
          ? "Where Play looked"
          : `Where Play looked (${status.searched.length} places)`,
      ),
    );
    const list = el("ul", "extra-list mono");
    for (const place of status.searched) list.appendChild(el("li", undefined, place));
    details.appendChild(list);
    notice.appendChild(details);
  }

  restoreFocus(notice, hadFocus);
}

function restoreFocus(within: HTMLElement, label: string | null): void {
  if (!label) return;
  for (const node of within.querySelectorAll<HTMLElement>("button, summary")) {
    if (node.textContent === label) {
      node.focus();
      return;
    }
  }
}

export async function pickRetroArch(): Promise<void> {
  try {
    const status = await invoke<RetroArchStatus>("choose_retroarch");
    if (status.available) toast(`RetroArch: ${status.path}`);
    await renderLibrary();
  } catch (error) {
    toast(String(error));
  }
}

async function resetRetroArch(): Promise<void> {
  try {
    await invoke<RetroArchStatus>("clear_retroarch");
    await renderLibrary();
  } catch (error) {
    toast(String(error));
  }
}

function renderShelves(data: LibraryView): void {
  const shelves = $<HTMLElement>("shelves");
  shelves.replaceChildren();

  const bySystem = new Map<string, Game[]>();
  for (const game of data.games) {
    const list = bySystem.get(game.system) ?? [];
    list.push(game);
    bySystem.set(game.system, list);
  }

  if (bySystem.size === 0) {
    shelves.appendChild(el("p", "empty", "Nothing on the shelves yet. Drop a pile of downloads on Intake."));
    return;
  }

  const names = [...bySystem.keys()].sort((a, b) => a.localeCompare(b));
  for (const system of names) {
    const games = bySystem.get(system) ?? [];
    const band = el("div", "row-band");
    band.appendChild(el("h2", "section-label", system));
    const shelf = el("div", "shelf");
    for (const game of games) shelf.appendChild(tile(game));
    band.appendChild(shelf);
    shelves.appendChild(band);
  }
}

function tile(game: Game): HTMLButtonElement {
  const button = el("button", "tile");
  button.type = "button";
  button.addEventListener("click", () => void openGame(game.id));

  const art = el("div", "art");
  art.appendChild(el("div", "no-art", game.title));
  button.appendChild(art);

  button.appendChild(el("div", "title", game.title));
  button.appendChild(el("div", "subtitle", game.system));
  return button;
}
