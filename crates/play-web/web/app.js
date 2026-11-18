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
  if (game.art) {
    const img = el("img", "cover");
    img.alt = "";
    img.loading = "lazy";
    img.decoding = "async";
    img.src = `/api/cover/${game.id}?v=${game.updated_at}`;
    img.addEventListener("error", () => img.remove(), { once: true });
    button.appendChild(img);
  }
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
      "The shelf is empty. Drop games on Add games below, or on the desktop app.";
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

// ---- adding games -----------------------------------------------------------
//
// Files go up one at a time, each streamed straight to disk on the machine,
// then the whole batch is filed as one drop and its report shown here.

function batchName() {
  const bytes = new Uint8Array(8);
  crypto.getRandomValues(bytes);
  return "web-" + Array.from(bytes, (b) => b.toString(16).padStart(2, "0")).join("");
}

function uploadPath(path) {
  return path
    .split("/")
    .filter((part) => part && part !== "." && part !== "..")
    .map(encodeURIComponent)
    .join("/");
}

// A dropped folder arrives as an entry to walk, not as files.
async function walkEntry(entry, prefix, out) {
  if (entry.isFile) {
    const file = await new Promise((resolve, reject) => entry.file(resolve, reject));
    out.push({ file, path: prefix + file.name });
    return;
  }
  if (entry.isDirectory) {
    const reader = entry.createReader();
    for (;;) {
      const batch = await new Promise((resolve, reject) => reader.readEntries(resolve, reject));
      if (batch.length === 0) break;
      for (const child of batch) await walkEntry(child, prefix + entry.name + "/", out);
    }
  }
}

async function filesFromDrop(dataTransfer) {
  const out = [];
  const items = Array.from(dataTransfer.items || []);
  const entries = items
    .map((item) => (item.webkitGetAsEntry ? item.webkitGetAsEntry() : null))
    .filter(Boolean);
  if (entries.length > 0) {
    for (const entry of entries) await walkEntry(entry, "", out);
    return out;
  }
  for (const file of Array.from(dataTransfer.files || [])) out.push({ file, path: file.name });
  return out;
}

const adding = { busy: false, batch: null, files: [] };

function addStatus(text) {
  $("add-status").textContent = text;
}

function renderAddReport(report) {
  const box = $("add-report");
  box.replaceChildren();
  box.hidden = false;
  const tally = new Map();
  for (const entry of report.entries) {
    tally.set(entry.outcome.word, (tally.get(entry.outcome.word) || 0) + 1);
  }
  box.appendChild(
    el(
      "p",
      "quiet",
      [...tally].map(([word, count]) => `${count} ${word}`).join(" · ") || "Nothing needed filing."
    )
  );
  const list = el("ul", "add-lines");
  for (const entry of report.entries) {
    const row = el("li");
    row.appendChild(el("span", "word", entry.outcome.word));
    row.appendChild(el("span", "file", entry.input.split("/").pop()));
    const detail = Object.values(entry.outcome.detail || {}).filter(Boolean).join(" · ");
    if (detail) row.appendChild(el("span", "quiet", detail));
    list.appendChild(row);
  }
  box.appendChild(list);
  const locked = report.entries.some(
    (e) => e.outcome.word === "quarantined" && /password/i.test((e.outcome.detail || {}).reason || "")
  );
  $("add-password").hidden = !locked;
}

async function fileBatch(password) {
  const report = await api(`/api/intake/${adding.batch}`, {
    method: "POST",
    headers: { "Content-Type": "application/json" },
    body: JSON.stringify({ password: password || null }),
  });
  renderAddReport(report);
  await refreshLibrary();
  // Covers come a moment after the games do.
  setTimeout(() => void refreshLibrary(), 6000);
  setTimeout(() => void refreshLibrary(), 20000);
}

async function addFiles(files) {
  if (adding.busy || files.length === 0) return;
  adding.busy = true;
  adding.batch = batchName();
  $("add-zone").classList.add("working");
  $("add-report").hidden = true;
  $("add-password").hidden = true;
  try {
    let sent = 0;
    const total = files.reduce((sum, f) => sum + f.file.size, 0);
    for (const [i, { file, path }] of files.entries()) {
      addStatus(`Sending ${i + 1} of ${files.length}: ${file.name} (${size(sent)} of ${size(total)})`);
      await api(`/api/upload/${adding.batch}/${uploadPath(path)}`, { method: "PUT", body: file });
      sent += file.size;
    }
    addStatus("Filing…");
    await fileBatch(null);
    addStatus("Done.");
  } catch (error) {
    addStatus(`That did not work: ${error.message}`);
  } finally {
    adding.busy = false;
    $("add-zone").classList.remove("working");
  }
}

function wireAdding() {
  const zone = $("add-zone");
  const pick = $("add-files");
  const pickFolder = $("add-folder");
  zone.addEventListener("dragover", (event) => {
    event.preventDefault();
    zone.classList.add("over");
  });
  zone.addEventListener("dragleave", () => zone.classList.remove("over"));
  zone.addEventListener("drop", (event) => {
    event.preventDefault();
    zone.classList.remove("over");
    void (async () => addFiles(await filesFromDrop(event.dataTransfer)))();
  });
  // A file dropped anywhere on the page should not open in the tab instead.
  window.addEventListener("dragover", (event) => event.preventDefault());
  window.addEventListener("drop", (event) => {
    if (!zone.contains(event.target)) {
      event.preventDefault();
      void (async () => addFiles(await filesFromDrop(event.dataTransfer)))();
    }
  });
  $("btn-add-files").addEventListener("click", () => pick.click());
  $("btn-add-folder").addEventListener("click", () => pickFolder.click());
  const fromInput = (input) => () => {
    const files = Array.from(input.files || []).map((file) => ({
      file,
      path: file.webkitRelativePath || file.name,
    }));
    input.value = "";
    void addFiles(files);
  };
  pick.addEventListener("change", fromInput(pick));
  pickFolder.addEventListener("change", fromInput(pickFolder));
  $("add-password").addEventListener("submit", (event) => {
    event.preventDefault();
    const value = $("add-password-value").value;
    if (!value || !adding.batch) return;
    void (async () => {
      try {
        addStatus("Trying the password…");
        await fileBatch(value);
        addStatus("Done.");
      } catch (error) {
        addStatus(`That did not work: ${error.message}`);
      }
    })();
  });
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

wireAdding();

void (async () => {
  await refreshLibrary();
  await refreshStatus();
  schedule();
})();
