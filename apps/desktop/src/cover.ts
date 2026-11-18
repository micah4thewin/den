import { el } from "./dom";
import type { Game } from "./types";

// Tauri serves a custom scheme as `scheme://localhost/...` on Linux and
// macOS, and as `http://scheme.localhost/...` on Windows.
const onWindows = navigator.userAgent.includes("Windows");

export function coverUrl(game: Game): string {
  const base = onWindows ? "http://cover.localhost" : "cover://localhost";
  return `${base}/${game.id}?v=${game.updated_at}`;
}

/** The cover if Play has one, and a plain title card if it does not, or if
 * the image will not load. */
export function fillArt(slot: HTMLElement, game: Game): void {
  slot.replaceChildren();
  const fallback = (): void => {
    slot.replaceChildren();
    const card = el("div", "no-art");
    card.appendChild(el("span", "no-art-title", game.title));
    card.appendChild(el("span", "no-art-system", game.system));
    slot.appendChild(card);
  };
  if (!game.art) {
    fallback();
    return;
  }
  const img = el("img");
  img.alt = "";
  img.loading = "lazy";
  img.decoding = "async";
  img.addEventListener("error", fallback, { once: true });
  img.src = coverUrl(game);
  slot.appendChild(img);
}
