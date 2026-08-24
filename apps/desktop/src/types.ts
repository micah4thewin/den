export interface Game {
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

export interface Save {
  id: number;
  game_id: number;
  kind: string;
  path: string;
  created_at: number;
}

export interface SystemRow {
  name: string;
  count: number;
}

export interface RetroArchStatus {
  available: boolean;
  path: string | null;
  source: string | null;
  chosen: boolean;
  problem: string | null;
  searched: string[];
  runtime_dir: string;
}

export interface LibraryView {
  path: string;
  games: Game[];
  systems: SystemRow[];
  continue_game: Game | null;
  recent: Game[];
  retroarch: RetroArchStatus;
}

export interface CoreStatus {
  name: string;
  installed: boolean | null;
  unsupported: string | null;
}

export interface GameView {
  game: Game;
  saves: Save[];
  retroarch: RetroArchStatus;
  core: CoreStatus;
}

export interface Outcome {
  word: string;
  detail: Record<string, string>;
}

export interface ReportEntry {
  input: string;
  outcome: Outcome;
}

export interface Report {
  started: number;
  finished: number;
  entries: ReportEntry[];
}

export interface ControllerInfo {
  id: string;
  identity: string;
  name: string;
  player: number | null;
  index: number | null;
}

export interface KeyBinding {
  action: string;
  key: string;
}

export interface ControllerView {
  pads: ControllerInfo[];
  keyboard: KeyBinding[];
  players: number;
}
