export const $ = <T extends HTMLElement>(id: string): T => {
  const node = document.getElementById(id);
  if (!node) throw new Error(`missing element #${id}`);
  return node as T;
};

export const el = <K extends keyof HTMLElementTagNameMap>(
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
export function toast(message: string): void {
  const node = $<HTMLDivElement>("toast");
  node.textContent = message;
  node.classList.remove("hidden");
  window.clearTimeout(toastTimer);
  toastTimer = window.setTimeout(() => node.classList.add("hidden"), 3600);
}

export function showScreen(name: string): void {
  for (const screen of document.querySelectorAll<HTMLElement>(".screen")) {
    screen.classList.toggle("active", screen.dataset.screen === name);
  }
  for (const nav of document.querySelectorAll<HTMLButtonElement>(".nav button")) {
    if (nav.dataset.screen === name) nav.setAttribute("aria-current", "page");
    else nav.removeAttribute("aria-current");
  }
}
