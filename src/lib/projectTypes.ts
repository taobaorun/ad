/**
 * IPC types for the M2/M4 project + apply backend. These mirror the Rust
 * shapes in `src-tauri/src/models.rs` and `src-tauri/src/commands/apply.rs`.
 *
 * All field names are camelCase at the FFI boundary (Rust serde
 * `rename_all = "camelCase"`).
 */

export interface Project {
  path: string;
  displayName: string;
  addedAt: string;
  currentProfileId?: string | null;
  lastApplied?: LastApplied | null;
  /** Pinned projects sort to the top of the sidebar. */
  pinned?: boolean;
  /** Whether the derived Project Codex Home inherits the Base user config. */
  inheritBaseConfig: boolean;
}

export interface LastApplied {
  profileId: string;
  timestamp: string;
  layers: string[];
  backupPaths: string[];
  conflictsResolved: number;
}

export interface ProjectStatus {
  exists: boolean;
  isGitRepo: boolean;
  gitDirty: boolean;
  claudeDirExists: boolean;
  hasSettingsJson: boolean;
  hasSettingsLocalJson: boolean;
  /** null when not a git repo or no .gitignore present. */
  gitignoreExcludesSettingsLocal: boolean | null;
}

export type ScanRootKind = 'cc_projects_meta' | 'generic';

export interface ScanRoot {
  path: string;
  kind: ScanRootKind;
  builtin: boolean;
  enabled: boolean;
}

export interface DetectedProject {
  path: string;
  sourceRoot: string;
  sourceKind: ScanRootKind;
  signals: string[];
  alreadyAdded: boolean;
}

/** A single field-level disagreement between an existing settings file and the incoming layer. */
export interface Conflict {
  keyPath: string;
  existing: unknown;
  incoming: unknown;
}

export type Resolution =
  | { kind: 'keepExisting' }
  | { kind: 'useIncoming' }
  | { kind: 'custom'; value: unknown };

export interface ApplyOptions {
  layers: ('shared' | 'local' | 'env')[];
  /** Map from `Conflict.keyPath` to `Resolution`. */
  resolutions: Record<string, Resolution>;
  /** Required true when applying `shared` layer to a dirty git repo. */
  overwriteDirtyWarningAcked: boolean;
}

export interface ApplyResult {
  profileId: string;
  projectPath: string;
  writtenFiles: string[];
  backupPaths: string[];
  warnings: string[];
  conflictsResolved: number;
}

export type ApplyOutcome =
  | ({ kind: 'applied' } & ApplyResult)
  | { kind: 'needsResolution'; layer: string; conflicts: Conflict[] }
  | { kind: 'gitDirtyBlocked'; message: string };
