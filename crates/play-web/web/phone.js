"use strict";

// The game, running here: the machine hands over the emulator and the game
// file, and the phone does the rest. Nothing is installed and nothing on the
// machine changes; saves stay in this browser.

const said = (words) => {
  document.getElementById("words").textContent = words;
  document.getElementById("said").hidden = false;
};

async function start() {
  const id = Number(new URLSearchParams(location.search).get("id"));
  if (!Number.isInteger(id) || id <= 0) {
    said("Pick a game from the shelf first.");
    return;
  }

  let view;
  try {
    const response = await fetch(`/api/game/${id}`);
    view = await response.json();
    if (!response.ok) throw new Error(view.error || `the shelf answered ${response.status}`);
  } catch (error) {
    said(`The shelf did not answer: ${error.message}`);
    return;
  }

  const phone = view.phone || {};
  if (!phone.system || !phone.rom) {
    said(phone.problem || "This game does not play on the phone.");
    return;
  }

  document.title = `${view.game.title} — Play`;
  document.getElementById("said").hidden = true;

  window.EJS_player = "#game";
  window.EJS_core = phone.system;
  window.EJS_gameUrl = new URL(phone.rom, location.href).href;
  window.EJS_gameName = view.game.title;
  window.EJS_pathtodata = "/emulator/";
  window.EJS_color = "#5b6067";
  window.EJS_backgroundColor = "#000000";
  window.EJS_startOnLoaded = true;

  const loader = document.createElement("script");
  loader.src = "/emulator/loader.js";
  loader.onerror = () => {
    said(
      "The machine could not hand over the emulator. It downloads it the first time a phone asks, so it needs the internet once."
    );
  };
  document.body.appendChild(loader);
}

void start();
