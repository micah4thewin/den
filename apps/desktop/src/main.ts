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
  source: string | null;
  chosen: boolean;
  problem: string | null;
  searched: string[];
  runtime_dir: string;
}

interface LibraryView {
  path: string;
  games: Game[];
  systems: SystemRow[];
  continue_game: Game | null;
  recent: Game[];
  retroarch: RetroArchStatus;
}

interface CoreStatus {
  name: string;
  installed: boolean | null;
  unsupported: string | null;
}

interface GameView {
  game: Game;
  saves: Save[];
  retroarch: RetroArchStatus;
  core: CoreStatus;
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
  identity: string;
  name: string;
  player: number | null;
  index: number | null;
}

interface KeyBinding {
  action: string;
  key: string;
}

interface ControllerView {
  pads: ControllerInfo[];
  keyboard: KeyBinding[];
  players: number;
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
/** Whether the "Where Den looked" list was left open. */
let whereOpen = false;

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
    const clear = el("button", "ghost", "Use the automatic search again");
    clear.type = "button";
    clear.addEventListener("click", () => void resetRetroArch());
    row.appendChild(clear);
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

  if (status.chosen) {
    const clear = el("button", "ghost", "Use the automatic search again");
    clear.type = "button";
    clear.addEventListener("click", () => void resetRetroArch());
    actions.appendChild(clear);
  }
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
async function pickRetroArch(): Promise<void> {
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

function renderControllers(view: ControllerView): void {
  const cards = $<HTMLElement>("pad-cards");
  cards.replaceChildren();

  if (view.pads.length === 0) {
    cards.appendChild(
      el(
        "p",
        "empty",
        "No gamepads found. Plug one in and press Refresh — or play on the keyboard, below.",
      ),
    );
  }

  for (const pad of view.pads) {
    const card = el("div", "pad-card");
    card.appendChild(el("div", "pad-name", pad.name));

    // A pad arrives already assigned, because a controller you plugged in
    // should be Player 1 without being asked. This is here to change it.
    const row = el("div", "pad-assign");
    const label = el("label", undefined, "Plays as");
    const select = el("select");
    select.id = `player-${pad.identity}`;
    label.htmlFor = select.id;

    const none = el("option", undefined, "Nobody");
    none.value = "";
    select.appendChild(none);
    for (let player = 1; player <= view.players; player += 1) {
      const option = el("option", undefined, `Player ${player}`);
      option.value = String(player);
      select.appendChild(option);
    }
    select.value = pad.player ? String(pad.player) : "";
    select.addEventListener("change", () => {
      const chosen = select.value === "" ? null : Number(select.value);
      void assignPad(pad.identity, chosen);
    });

    row.appendChild(label);
    row.appendChild(select);
    card.appendChild(row);
    cards.appendChild(card);
  }

  renderKeyboard(view.keyboard);
}

/** The keys Den binds, listed where somebody can read them. */
function renderKeyboard(keyboard: KeyBinding[]): void {
  const cards = $<HTMLElement>("pad-cards");
  const card = el("div", "pad-card keyboard-card");
  card.appendChild(el("div", "pad-name", "Keyboard"));
  card.appendChild(
    el("p", "quiet", "Always available, whether or not a pad is plugged in."),
  );
  const list = el("dl", "keymap");
  for (const binding of keyboard) {
    // Each pair in its own row, so the dotted lead runs between the action
    // and its key rather than across the whole grid.
    const pair = el("div");
    pair.appendChild(el("dt", undefined, binding.action));
    pair.appendChild(el("span", "lead"));
    const key = el("dd");
    key.appendChild(el("kbd", undefined, binding.key));
    pair.appendChild(key);
    list.appendChild(pair);
  }
  card.appendChild(list);
  cards.appendChild(card);
}

async function assignPad(identity: string, player: number | null): Promise<void> {
  try {
    const view = await invoke<ControllerView>("assign_pad", { identity, player });
    renderControllers(view);
    const pad = view.pads.find((p) => p.identity === identity);
    toast(
      pad?.player ? `${pad.name} plays as Player ${pad.player}` : "That pad plays for nobody",
    );
  } catch (error) {
    toast(String(error));
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
  $<HTMLButtonElement>("btn-pads").addEventListener("click", () => void refreshControllers());
}

async function refreshControllers(): Promise<void> {
  try {
    renderControllers(await invoke<ControllerView>("list_controllers"));
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
