"use strict";

const el = (tag, cls, text) => {
  const node = document.createElement(tag);
  if (cls) node.className = cls;
  if (text !== undefined) node.textContent = text;
  return node;
};

const $ = (id) => document.getElementById(id);

let library = null;
let toastTimer = null;

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
    throw new Error(body.error || `the server answered ${response.status}`);
  }
  return body;
}

function minutes(playtime) {
  if (!playtime || playtime < 60) return null;
  const hours = Math.floor(playtime / 3600);
  const mins = Math.round((playtime % 3600) / 60);
  return hours > 0 ? `${hours}h ${mins}m played` : `${mins}m played`;
}

function tile(game) {
  const button = el("button", "tile");
  button.type = "button";
  button.appendChild(el("span", "title", game.title));
  const played = minutes(game.playtime);
  button.appendChild(el("span", "meta", played ? `${game.system} — ${played}` : game.system));
  button.addEventListener("click", async () => {
    button.disabled = true;
    try {
      await api(`/api/launch/${game.id}`, { method: "POST" });
      toast(`Started ${game.title} on the machine the shelf lives on.`);
      refreshStatus();
    } catch (error) {
      toast(error.message);
    } finally {
      button.disabled = false;
    }
  });
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

function render() {
  const shelves = $("shelves");
  const note = $("note");
  shelves.textContent = "";
  if (!library) return;

  const filter = $("search").value.trim().toLowerCase();
  const games = filter
    ? library.games.filter((g) => g.title.toLowerCase().includes(filter))
    : library.games;

  if (library.games.length === 0) {
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

  if (!filter) {
    if (library.continue_game) {
      shelves.appendChild(band("Continue where you left off", [library.continue_game]));
    }
    if (library.recent.length > 0) {
      shelves.appendChild(band("Recent", library.recent));
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

async function refreshStatus() {
  try {
    const status = await api("/api/status");
    $("status").textContent =
      status.running === 0 ? "" : status.running === 1 ? "1 game running" : `${status.running} games running`;
  } catch {
    $("status").textContent = "";
  }
}

async function refreshLibrary() {
  try {
    library = await api("/api/library");
    render();
  } catch (error) {
    const note = $("note");
    note.textContent = `The shelf did not answer: ${error.message}`;
    note.hidden = false;
  }
}

$("search").addEventListener("input", render);
window.addEventListener("focus", () => {
  refreshLibrary();
  refreshStatus();
});
setInterval(refreshStatus, 15000);

refreshLibrary();
refreshStatus();
