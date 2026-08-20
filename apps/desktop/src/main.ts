// The Den shell: vanilla TypeScript over the thin IPC layer in src-tauri.
//
// One rule: this file never sets innerHTML. Every node is built with the
// same createElementNS/createElement discipline as the icon set, so a title
// that came out of a filename is always text, never markup.

import { invoke } from "@tauri-apps/api/core";
import { getCurrentWebview } from "@tauri-apps/api/webview";
import { icon } from "./ui/icons";

// ---- types mirroring the Rust shapes ------------------------------------

interface Game {
  id: number;
  title: string;
  system: string;
  path: string;
  hash: string | null;
  size: number | null;
  status: string;
  core: string | null;
  art: string | null;
  created_at: number;
  updated_at: number;
  playtime: number;
  last_played: number | null;
}

interface Save {
  id: number;
  game_id: number;
  kind: string;
  path: string;
  created_at: number;
}

interface SystemRow {
  name: string;
  count: number;
}

interface RetroArchStatus {
  available: boolean;
  path: string | null;
  problem: string | null;
  searched: string[];
}

interface LibraryView {
  path: string;
  games: Game[];
  systems: SystemRow[];
  continue_game: Game | null;
  recent: Game[];
  retroarch: RetroArchStatus;
}

interface GameView {
  game: Game;
  saves: Save[];
  retroarch: RetroArchStatus;
}

interface Outcome {
  word: string;
  detail: Record<string, string>;
}

interface ReportEntry {
  input: string;
  outcome: Outcome;
}

interface Report {
  started: number;
  finished: number;
  entries: ReportEntry[];
}

interface ControllerInfo {
  id: string;
  name: string;
  player: number | null;
}

// ---- small helpers --------------------------------------------------------

const $ = <T extends HTMLElement>(id: string): T => {
  const node = document.getElementById(id);
  if (!node) throw new Error(`missing element #${id}`);
  return node as T;
};

const el = <K extends keyof HTMLElementTagNameMap>(
  tag: K,
  className?: string,
  text?: string,
): HTMLElementTagNameMap[K] => {
  const node = document.createElement(tag);
  if (className) node.className = className;
  if (text !== undefined) node.textContent = text;
  return node;
};

let toastTimer: number | undefined;
function toast(message: string): void {
  const node = $<HTMLDivElement>("toast");
  node.textContent = message;
  node.classList.remove("hidden");
  window.clearTimeout(toastTimer);
  toastTimer = window.setTimeout(() => node.classList.add("hidden"), 3600);
}

function showScreen(name: string): void {
  for (const screen of document.querySelectorAll<HTMLElement>(".screen")) {
    screen.classList.toggle("active", screen.dataset.screen === name);
  }
  for (const nav of document.querySelectorAll<HTMLButtonElement>(".nav button")) {
    if (nav.dataset.screen === name) nav.setAttribute("aria-current", "page");
    else nav.removeAttribute("aria-current");
  }
}

// ---- screens ---------------------------------------------------------------

async function renderLibrary(): Promise<void> {
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

/** A sentence under the Library heading when there is nothing to play with. */
function renderRetroArchNotice(status: RetroArchStatus): void {
  const notice = $<HTMLElement>("retroarch-notice");
  notice.replaceChildren();
  notice.hidden = status.available;
  if (status.available) return;

  notice.appendChild(
    el(
      "p",
      "notice-word",
      "Den can shelve and name games without RetroArch, but it needs RetroArch to play them.",
    ),
  );
  if (status.problem) notice.appendChild(el("p", "quiet", status.problem));

  if (status.searched.length > 0) {
    const details = el("details", "notice-where");
    details.appendChild(el("summary", undefined, "Where Den looked"));
    const list = el("ul", "extra-list mono");
    for (const place of status.searched) list.appendChild(el("li", undefined, place));
    details.appendChild(list);
    notice.appendChild(details);
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
    band.appendChild(el("h2", undefined, system));
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

async function openGame(id: number): Promise<void> {
  try {
    const view = await invoke<GameView>("get_game", { id });
    renderGame(view);
    showScreen("game");
  } catch (error) {
    toast(String(error));
  }
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
  play.disabled = !view.retroarch.available;
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
  if (!view.retroarch.available) {
    playRow.appendChild(
      el("span", "quiet play-note", "RetroArch was not found — see the Library screen."),
    );
  }
  meta.appendChild(playRow);

  if (view.saves.length > 0) {
    const field = el("div", "field");
    field.appendChild(el("h2", undefined, "Saves"));
    const list = el("ul", "extra-list");
    for (const save of view.saves) {
      list.appendChild(el("li", undefined, `${save.kind} · ${new Date(save.created_at * 1000).toLocaleString()}`));
    }
    field.appendChild(list);
    meta.appendChild(field);
  }

  const field = el("div", "field");
  field.appendChild(el("h2", undefined, "Details"));
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

/** The last component of a path, whichever separator wrote it. */
function baseName(path: string): string {
  const cut = Math.max(path.lastIndexOf("/"), path.lastIndexOf("\\"));
  return cut >= 0 ? path.slice(cut + 1) : path;
}

function renderReport(report: Report): void {
  const card = $<HTMLElement>("report-card");
  card.replaceChildren();
  card.classList.remove("hidden");

  card.appendChild(el("h2", undefined, "Intake report"));

  if (report.entries.length === 0) {
    card.appendChild(el("p", "empty", "Nothing in that folder needed filing."));
    void renderLibrary();
    return;
  }

  // The same words, counted: what happened, before which file it happened to.
  const tally = new Map<string, number>();
  for (const entry of report.entries) {
    tally.set(entry.outcome.word, (tally.get(entry.outcome.word) ?? 0) + 1);
  }
  const summary = el("div", "report-tally");
  for (const [word, count] of [...tally].sort((a, b) => a[0].localeCompare(b[0]))) {
    const pair = el("span");
    pair.appendChild(el("span", "count", String(count)));
    pair.appendChild(document.createTextNode(` ${word}`));
    summary.appendChild(pair);
  }
  card.appendChild(summary);

  for (const entry of report.entries) {
    const row = el("div", "report-entry");
    row.appendChild(el("span", "word", entry.outcome.word));

    const file = el("span", "file");
    file.textContent = baseName(entry.input);
    file.title = entry.input;
    row.appendChild(file);

    const detail = Object.values(entry.outcome.detail ?? {})
      .filter(Boolean)
      .join(" · ");
    if (detail) row.appendChild(el("span", "reason", detail));

    card.appendChild(row);
  }

  void renderLibrary();
}

function renderControllers(controllers: ControllerInfo[]): void {
  const cards = $<HTMLElement>("pad-cards");
  cards.replaceChildren();
  if (controllers.length === 0) {
    cards.appendChild(el("p", "empty", "No gamepads detected. Plug one in and it should appear here."));
    return;
  }
  for (const pad of controllers) {
    const card = el("div", "pad-card");
    card.appendChild(el("div", "pad-name", pad.name));
    card.appendChild(el("div", "pad-player", pad.player ? `Player ${pad.player}` : "Unassigned"));
    cards.appendChild(card);
  }
}

// ---- intake ----------------------------------------------------------------

async function intakeFolder(path: string): Promise<void> {
  const status = $<HTMLElement>("intake-status");
  status.textContent = "Working…";
  status.classList.add("working");
  try {
    const report = await invoke<Report>("run_intake", { folder: path, password: null });
    renderReport(report);
    status.textContent = "Done.";
  } catch (error) {
    toast(String(error));
    status.textContent = "Intake failed.";
  } finally {
    status.classList.remove("working");
  }
}

async function chooseAndIntake(): Promise<void> {
  try {
    const folder = await invoke<string | null>("choose_folder");
    if (folder) await intakeFolder(folder);
  } catch (error) {
    toast(String(error));
  }
}

// ---- wiring -----------------------------------------------------------------

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
}

async function refreshControllers(): Promise<void> {
  try {
    renderControllers(await invoke<ControllerInfo[]>("list_controllers"));
  } catch (error) {
    toast(String(error));
  }
}

async function wireDrop(): Promise<void> {
  const dropzone = $<HTMLElement>("dropzone");

  // It is announced as a button and reachable with Tab, so it has to do what
  // a button does. Native drag-and-drop is the fast path; this is the one
  // that works with a keyboard, or with no pointer at all.
  dropzone.addEventListener("click", () => void chooseAndIntake());
  dropzone.addEventListener("keydown", (event) => {
    if (event.key === "Enter" || event.key === " ") {
      event.preventDefault();
      void chooseAndIntake();
    }
  });

  try {
    await getCurrentWebview().onDragDropEvent((event) => {
      const payload = event.payload;
      if (payload.type === "enter" || payload.type === "over") {
        dropzone.classList.add("drag-over");
      } else if (payload.type === "leave") {
        dropzone.classList.remove("drag-over");
      } else if (payload.type === "drop") {
        dropzone.classList.remove("drag-over");
        for (const path of payload.paths) void intakeFolder(path);
      }
    });
  } catch {
    // Native drag-drop is unavailable in a plain browser; the Choose button
    // still works, so this is only a quiet degradation.
  }
}

async function boot(): Promise<void> {
  injectIcons();
  wireNavigation();
  await wireDrop();
  showScreen("library");
  try {
    await renderLibrary();
  } catch (error) {
    toast(String(error));
  }
}

void boot();
