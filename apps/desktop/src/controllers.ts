import { invoke } from "@tauri-apps/api/core";
import { $, el, toast } from "./dom";
import type { ControllerView, KeyBinding } from "./types";

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

function renderKeyboard(keyboard: KeyBinding[]): void {
  const cards = $<HTMLElement>("pad-cards");
  const card = el("div", "pad-card keyboard-card");
  card.appendChild(el("div", "pad-name", "Keyboard"));
  card.appendChild(
    el("p", "quiet", "Always available, whether or not a pad is plugged in."),
  );
  const list = el("dl", "keymap");
  for (const binding of keyboard) {
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

export async function refreshControllers(): Promise<void> {
  try {
    renderControllers(await invoke<ControllerView>("list_controllers"));
  } catch (error) {
    toast(String(error));
  }
}
