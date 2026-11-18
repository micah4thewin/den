import { invoke } from "@tauri-apps/api/core";
import { getCurrentWebview } from "@tauri-apps/api/webview";
import { $, el, showScreen, toast } from "./dom";
import { expectCovers, renderLibrary } from "./library";
import type { Report } from "./types";

function baseName(path: string): string {
  const cut = Math.max(path.lastIndexOf("/"), path.lastIndexOf("\\"));
  return cut >= 0 ? path.slice(cut + 1) : path;
}

function renderReport(report: Report): void {
  const card = $<HTMLElement>("report-card");
  card.replaceChildren();
  card.classList.remove("hidden");

  card.appendChild(el("h2", "section-label", "Intake report"));

  if (report.entries.length === 0) {
    card.appendChild(el("p", "empty", "Nothing in that folder needed filing."));
    void renderLibrary();
    return;
  }

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

let busy = false;
const waiting: string[][] = [];
let lastPaths: string[] = [];

function needsPassword(report: Report): boolean {
  return report.entries.some(
    (e) => e.outcome.word === "quarantined" && /password/i.test(e.outcome.detail?.reason ?? ""),
  );
}

function offerPassword(): void {
  const card = $<HTMLElement>("report-card");
  const form = el("form", "password-retry");
  const label = el("label", "quiet", "Some archives are locked. Their password:");
  const input = el("input");
  input.type = "password";
  input.autocomplete = "off";
  input.id = "intake-password";
  label.htmlFor = input.id;
  const button = el("button", undefined, "Try again");
  button.type = "submit";
  form.append(label, input, button);
  form.addEventListener("submit", (event) => {
    event.preventDefault();
    if (input.value) void intakePaths(lastPaths, input.value);
  });
  card.insertBefore(form, card.children[1] ?? null);
  input.focus();
}

/** Take in everything that was dropped at once, as one report. A drop that
 * arrives while another is still being filed waits its turn. */
async function intakePaths(paths: string[], password: string | null = null): Promise<void> {
  if (paths.length === 0) return;
  if (busy) {
    waiting.push(paths);
    return;
  }
  busy = true;
  lastPaths = paths;
  showScreen("intake");
  const status = $<HTMLElement>("intake-status");
  const what = paths.length === 1 ? baseName(paths[0]) : `${paths.length} items`;
  status.textContent = `Filing ${what}…`;
  status.classList.add("working");
  try {
    const report = await invoke<Report>("run_intake", { paths, password });
    expectCovers();
    renderReport(report);
    if (needsPassword(report)) offerPassword();
    status.textContent = "Done.";
  } catch (error) {
    toast(String(error));
    status.textContent = "Intake failed.";
  } finally {
    status.classList.remove("working");
    busy = false;
    const next = waiting.shift();
    if (next) void intakePaths(next);
  }
}

export async function chooseAndIntake(): Promise<void> {
  try {
    const folder = await invoke<string | null>("choose_folder");
    if (folder) await intakePaths([folder]);
  } catch (error) {
    toast(String(error));
  }
}

export async function chooseFilesAndIntake(): Promise<void> {
  try {
    const files = await invoke<string[]>("choose_files");
    await intakePaths(files);
  } catch (error) {
    toast(String(error));
  }
}

export async function wireDrop(): Promise<void> {
  const dropzone = $<HTMLElement>("dropzone");

  dropzone.addEventListener("click", () => void chooseAndIntake());
  dropzone.addEventListener("keydown", (event) => {
    if (event.key === "Enter" || event.key === " ") {
      event.preventDefault();
      void chooseAndIntake();
    }
  });

  // The whole window takes a drop, whichever screen is showing: nobody
  // should have to find the drop zone first.
  try {
    await getCurrentWebview().onDragDropEvent((event) => {
      const payload = event.payload;
      if (payload.type === "enter" || payload.type === "over") {
        dropzone.classList.add("drag-over");
        document.body.classList.add("drag-over");
      } else if (payload.type === "leave") {
        dropzone.classList.remove("drag-over");
        document.body.classList.remove("drag-over");
      } else if (payload.type === "drop") {
        dropzone.classList.remove("drag-over");
        document.body.classList.remove("drag-over");
        void intakePaths(payload.paths);
      }
    });
  } catch (error) {
    console.warn("drag and drop is not available here", error);
  }
}
