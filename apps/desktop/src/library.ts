import { invoke } from "@tauri-apps/api/core";
import { $, el, toast } from "./dom";
import { openGame } from "./game";
import type { Game, LibraryView, RetroArchStatus } from "./types";

export async function renderLibrary(): Promise<void> {
  const data = await invoke<LibraryView>("get_library");
  $<HTMLElement>("library-path").textContent = data.path;
  $<HTMLElement>("game-count").textContent =
    data.games.length === 1 ? "1 game" : `${data.games.length} games`;

  renderRetroArchNotice(data.retroarch);

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

/** Whether the "Where Den looked" list was left open. */
let whereOpen = false;

function searchAgainButton(): HTMLButtonElement {
  const clear = el("button", "ghost", "Use the automatic search again");
  clear.type = "button";
  clear.addEventListener("click", () => void resetRetroArch());
  return clear;
}

function renderRetroArchNotice(status: RetroArchStatus): void {
  const notice = $<HTMLElement>("retroarch-notice");
  // A re-render replaces every node, which drops focus to <body> and folds
  // the disclosure shut under whoever was reading it. Both are put back.
  const hadFocus = notice.contains(document.activeElement)
    ? (document.activeElement as HTMLElement).textContent
    : null;
  notice.replaceChildren();
  notice.hidden = status.available && !status.chosen;

  // A choice that works still has to be undoable, or picking one is a
  // one-way door: the search never runs again, however the machine changes.
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
      "Den can shelve and name games without RetroArch, but it needs RetroArch to play them.",
    ),
  );
  if (status.problem) notice.appendChild(el("p", "quiet", status.problem));

  // The search cannot cover every place an emulator might be. A person
  // looking at their own filesystem can, so the way out is always offered
  // rather than left for them to find out about.
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
          ? "Where Den looked"
          : `Where Den looked (${status.searched.length} places)`,
      ),
    );
    const list = el("ul", "extra-list mono");
    for (const place of status.searched) list.appendChild(el("li", undefined, place));
    details.appendChild(list);
    notice.appendChild(details);
  }

  restoreFocus(notice, hadFocus);
}

/** Put focus back on the control with the same label, after a re-render. */
function restoreFocus(within: HTMLElement, label: string | null): void {
  if (!label) return;
  for (const node of within.querySelectorAll<HTMLElement>("button, summary")) {
    if (node.textContent === label) {
      node.focus();
      return;
    }
  }
}

/** Point Den at a RetroArch by hand. */
export async function pickRetroArch(): Promise<void> {
  try {
    const status = await invoke<RetroArchStatus>("choose_retroarch");
    if (status.available) toast(`RetroArch: ${status.path}`);
    // renderLibrary draws the notice from the same answer, so drawing it
    // here as well would only make the live region speak twice.
    await renderLibrary();
  } catch (error) {
    toast(String(error));
  }
}

/** Hand the choice back to the automatic search. */
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
