"use strict";

const el = (tag, cls, text) => {
  const node = document.createElement(tag);
  if (cls) node.className = cls;
  if (text !== undefined) node.textContent = text;
  return node;
};

const $ = (id) => document.getElementById(id);

const state = {
  library: null,
  playing: [],
  polledAt: Date.now(),
  system: null,
  reachable: true,
  showing: null,
};

let toastTimer = null;
let statusTimer = null;

function toast(message) {
  const box = $("toast");
  box.textContent = message;
  box.hidden = false;
  clearTimeout(toastTimer);
  toastTimer = setTimeout(() => {
    box.hidden = true;
  }, 3600);
}

async function api(path, options) {
  const response = await fetch(path, options);
  const body = await response.json().catch(() => ({}));
  if (!response.ok) {
    throw new Error(body.error || `the shelf answered ${response.status}`);
  }
  return body;
}

// ---- words for numbers -----------------------------------------------------

function played(seconds) {
  if (!seconds || seconds < 60) return null;
  const total = Math.round(seconds / 60);
  const hours = Math.floor(total / 60);
  const mins = total % 60;
  return hours > 0 ? `${hours}h ${mins}m` : `${mins}m`;
}

function forHowLong(seconds) {
  if (seconds < 60) return "just started";
  return `for ${played(seconds)}`;
}

function size(bytes) {
  if (!bytes || bytes <= 0) return null;
  const mb = bytes / (1024 * 1024);
  if (mb < 1) return `${Math.max(1, Math.round(bytes / 1024))} KB`;
  if (mb < 1024) return `${mb < 10 ? mb.toFixed(1) : Math.round(mb)} MB`;
  return `${(mb / 1024).toFixed(1)} GB`;
}

function day(seconds) {
  if (!seconds) return null;
  return new Date(seconds * 1000).toLocaleDateString(undefined, {
    year: "numeric",
    month: "short",
    day: "numeric",
  });
}

// ---- what is playing -------------------------------------------------------

function isPlaying(id) {
  return state.playing.some((live) => live.game_id === id);
}

// Seconds it has been playing, carried forward from the last poll so the
// bar counts up between them.
function playingFor(live) {
  const since = Math.max(0, Math.round((Date.now() - state.polledAt) / 1000));
  return live.seconds + since;
}

async function stopGame(id, name) {
  try {
    const answer = await api(id === null ? "/api/stop" : `/api/stop/${id}`, { method: "POST" });
    toast(answer.stopped > 0 ? `Stopped ${name}.` : `${name} had already finished.`);
  } catch (error) {
    toast(error.message);
  }
  await refreshStatus();
  await refreshLibrary();
}

function renderPlaying() {
  const bar = $("playing");
  bar.replaceChildren();
  if (state.playing.length === 0) {
    bar.hidden = true;
    return;
  }
  bar.hidden = false;

  const first = state.playing[0];
  const what = el("div", "what");
  what.appendChild(
    el(
      "div",
      "name",
      state.playing.length === 1 ? first.title : `${first.title} and ${state.playing.length - 1} more`
    )
  );
  what.appendChild(el("div", "since", `${first.system} — ${forHowLong(playingFor(first))}`));
  bar.appendChild(what);

  const stop = el("button", "btn", state.playing.length === 1 ? "Stop" : "Stop all");
  stop.type = "button";
  stop.addEventListener("click", async () => {
    stop.disabled = true;
    if (state.playing.length === 1) {
      await stopGame(first.game_id, first.title);
    } else {
      await stopGame(null, "everything");
    }
    stop.disabled = false;
  });
  bar.appendChild(stop);
}

// ---- the shelf -------------------------------------------------------------

function tile(game) {
  const button = el("button", "tile");
  button.type = "button";
  if (isPlaying(game.id)) button.dataset.playing = "yes";
  button.appendChild(el("span", "title", game.title));
  const time = played(game.playtime);
  const words = isPlaying(game.id)
    ? `${game.system} — playing`
    : time
      ? `${game.system} — ${time} played`
      : game.system;
  button.appendChild(el("span", "meta", words));
  button.setAttribute("aria-haspopup", "dialog");
  button.addEventListener("click", () => void openSheet(game.id));
  return button;
}

function band(label, games) {
  const section = el("section", "band");
  section.appendChild(el("h2", null, label));
  const shelf = el("div", "shelf");
  for (const game of games) shelf.appendChild(tile(game));
  section.appendChild(shelf);
  return section;
}

function renderChips() {
  const chips = $("chips");
  chips.replaceChildren();
  const systems = state.library ? state.library.systems : [];
  if (systems.length < 2) {
    chips.hidden = true;
    return;
  }
  chips.hidden = false;
  const add = (label, system) => {
    const chip = el("button", "chip", label);
    chip.type = "button";
    chip.setAttribute("aria-pressed", String(state.system === system));
    chip.addEventListener("click", () => {
      state.system = state.system === system ? null : system;
      renderChips();
      render();
    });
    chips.appendChild(chip);
  };
  add("Everything", null);
  for (const row of systems) add(`${row.name} ${row.count}`, row.name);
}

function render() {
  const shelves = $("shelves");
  const note = $("note");
  shelves.replaceChildren();
  if (!state.library) return;

  const filter = $("search").value.trim().toLowerCase();
  const games = state.library.games.filter(
    (game) =>
      (!filter || game.title.toLowerCase().includes(filter)) &&
      (!state.system || game.system === state.system)
  );

  if (state.library.games.length === 0) {
    note.textContent =
      "The shelf is empty. Add games in the desktop app on the machine that keeps the library.";
    note.hidden = false;
    return;
  }
  if (games.length === 0) {
    note.textContent = "Nothing on the shelf matches that.";
    note.hidden = false;
    return;
  }
  note.hidden = true;

  if (!filter && !state.system) {
    if (state.library.continue_game) {
      shelves.appendChild(band("Continue where you left off", [state.library.continue_game]));
    }
    if (state.library.recent.length > 0) {
      shelves.appendChild(band("Recent", state.library.recent));
    }
  }

  const bySystem = new Map();
  for (const game of games) {
    if (!bySystem.has(game.system)) bySystem.set(game.system, []);
    bySystem.get(game.system).push(game);
  }
  for (const [system, list] of bySystem) {
    shelves.appendChild(band(`${system} — ${list.length}`, list));
  }
}

// ---- one game --------------------------------------------------------------

function fact(key, value) {
  const row = el("li");
  row.appendChild(el("span", "key", key));
  row.appendChild(el("span", "value", value));
  return row;
}

function coreWords(view) {
  if (view.core.unsupported) return view.core.unsupported;
  if (!view.core.name) return "no core for this system yet";
  if (view.core.installed === false) return `${view.core.name} — not installed`;
  if (view.core.installed === true) return `${view.core.name} — installed`;
  return view.core.name;
}

// Why the machine could not start this, or nothing if it can.
function whyNot(view) {
  if (view.core.unsupported) return view.core.unsupported;
  if (!view.retroarch.available) {
    return view.retroarch.problem || "Play has not found RetroArch on that machine.";
  }
  if (view.core.installed === false) {
    return `The ${view.core.name} core is not installed on that machine. RetroArch downloads it: Online Updater, then Core Downloader.`;
  }
  return null;
}

function sheetFor(view) {
  const body = el("div");
  const game = view.game;
  body.appendChild(el("h2", null, game.title));
  body.appendChild(el("p", "quiet", game.system));

  const facts = el("ul", "facts");
  const time = played(game.playtime);
  if (time) facts.appendChild(fact("Played", time));
  const last = day(game.last_played);
  if (last) facts.appendChild(fact("Last played", last));
  const bytes = size(game.size);
  if (bytes) facts.appendChild(fact("Size", bytes));
  facts.appendChild(fact("Core", coreWords(view)));
  facts.appendChild(
    fact("Saves", view.saves.length === 0 ? "none yet" : String(view.saves.length))
  );
  body.appendChild(facts);

  const blocked = whyNot(view);
  if (blocked) {
    const why = el("p", "quiet", blocked);
    why.style.marginTop = "0.9rem";
    body.appendChild(why);
  }

  const actions = el("div", "actions");
  // Straight from the machine, a moment ago — fresher than the last poll.
  const playing = view.playing;

  const go = el("button", "btn loud", playing ? "Playing now" : "Play on the machine");
  go.type = "button";
  go.disabled = Boolean(blocked) || playing;
  go.addEventListener("click", async () => {
    go.disabled = true;
    try {
      await api(`/api/launch/${game.id}`, { method: "POST" });
      toast(`Started ${game.title} on the machine the shelf lives on.`);
      closeSheet();
      await refreshStatus();
      await refreshLibrary();
    } catch (error) {
      toast(error.message);
      go.disabled = false;
    }
  });
  actions.appendChild(go);

  const row = el("div", "row");
  if (playing) {
    const stop = el("button", "btn", "Stop");
    stop.type = "button";
    stop.addEventListener("click", async () => {
      stop.disabled = true;
      await stopGame(game.id, game.title);
      closeSheet();
    });
    row.appendChild(stop);
  }
  const close = el("button", "btn", "Close");
  close.type = "button";
  close.addEventListener("click", closeSheet);
  row.appendChild(close);
  actions.appendChild(row);

  body.appendChild(actions);
  return body;
}

function closeSheet() {
  state.showing = null;
  const sheet = $("sheet");
  if (sheet.open) sheet.close();
}

async function openSheet(id) {
  const sheet = $("sheet");
  const body = $("sheet-body");
  state.showing = id;
  body.replaceChildren(el("p", "quiet", "Looking…"));
  if (!sheet.open) sheet.showModal();
  try {
    const view = await api(`/api/game/${id}`);
    if (state.showing !== id) return;
    body.replaceChildren(sheetFor(view));
  } catch (error) {
    if (state.showing !== id) return;
    const said = el("div");
    said.appendChild(el("p", null, error.message));
    const close = el("button", "btn", "Close");
    close.type = "button";
    close.addEventListener("click", closeSheet);
    said.appendChild(close);
    body.replaceChildren(said);
  }
}

// ---- keeping up ------------------------------------------------------------

function trouble(message) {
  const box = $("trouble");
  if (!message) {
    box.hidden = true;
    return;
  }
  box.textContent = message;
  box.hidden = false;
}

async function refreshLibrary() {
  try {
    state.library = await api("/api/library");
    renderChips();
    render();
  } catch (error) {
    // A shelf already on screen is better than an error where it was.
    if (state.library) return;
    const note = $("note");
    note.textContent = `The shelf did not answer: ${error.message}`;
    note.hidden = false;
  }
}

function whatIsPlaying() {
  return state.playing
    .map((live) => live.game_id)
    .sort()
    .join(",");
}

async function refreshStatus() {
  try {
    const before = whatIsPlaying();
    const status = await api("/api/status");
    state.playing = status.playing || [];
    state.polledAt = Date.now();
    if (!state.reachable) {
      // It came back; the shelf itself may have moved on while it was away.
      state.reachable = true;
      trouble(null);
      await refreshLibrary();
    }
    renderPlaying();
    // The shelf only changes here when a tile's "playing" mark does, and a
    // poll every few seconds must not rebuild it under somebody's thumb.
    if (whatIsPlaying() !== before) render();
  } catch {
    state.reachable = false;
    trouble("Cannot reach the machine the shelf lives on. Still trying.");
  }
}

// Ask often while something is playing, rarely while nothing is, and never
// at all while the phone is in a pocket.
function schedule() {
  clearTimeout(statusTimer);
  if (document.hidden) return;
  const wait = !state.reachable ? 5000 : state.playing.length > 0 ? 5000 : 20000;
  statusTimer = setTimeout(async () => {
    await refreshStatus();
    schedule();
  }, wait);
}

$("search").addEventListener("input", render);
$("search").addEventListener("keydown", (event) => {
  if (event.key === "Enter") event.target.blur();
});

const sheet = $("sheet");
sheet.addEventListener("close", () => {
  state.showing = null;
});
// Outside the card is outside the sheet.
sheet.addEventListener("click", (event) => {
  if (event.target === sheet) closeSheet();
});

document.addEventListener("visibilitychange", () => {
  if (document.hidden) {
    clearTimeout(statusTimer);
    return;
  }
  void (async () => {
    await refreshStatus();
    await refreshLibrary();
    schedule();
  })();
});

if ("serviceWorker" in navigator) {
  // Only offered on a secure origin, so a plain http:// shelf simply has none.
  window.addEventListener("load", () => {
    navigator.serviceWorker.register("/sw.js").catch(() => {});
  });
}

void (async () => {
  await refreshLibrary();
  await refreshStatus();
  schedule();
})();
