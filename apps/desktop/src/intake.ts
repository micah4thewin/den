import { invoke } from "@tauri-apps/api/core";
import { getCurrentWebview } from "@tauri-apps/api/webview";
import { $, el, toast } from "./dom";
import { renderLibrary } from "./library";
import type { Report } from "./types";

/** The last component of a path, whichever separator wrote it. */
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

export async function chooseAndIntake(): Promise<void> {
  try {
    const folder = await invoke<string | null>("choose_folder");
    if (folder) await intakeFolder(folder);
  } catch (error) {
    toast(String(error));
  }
}

export async function wireDrop(): Promise<void> {
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
