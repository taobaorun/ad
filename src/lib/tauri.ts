import { invoke } from '@tauri-apps/api/core';
import type { ProfileFile, ClaudeSettings } from './profileSchema';
import type {
  ApplyOptions,
  ApplyOutcome,
  DetectedProject,
  Project,
  ProjectStatus,
  ScanRoot,
} from './projectTypes';

export interface ClaudeProcess {
  pid: number;
  cmd: string;
}

export interface ActivationResult {
  activatedId: string;
  backupPath: string | null;
  detectedPids: ClaudeProcess[];
}

export interface ActivationLogEntry {
  ts: string;
  from: string | null;
  to: string;
  backupPath?: string | null;
}

export const tauri = {
  listProfiles: () => invoke<ProfileFile[]>('list_profiles'),
  getProfile: (id: string) => invoke<ProfileFile>('get_profile', { id }),
  saveProfile: (profile: ProfileFile) => invoke<ProfileFile>('save_profile', { profile }),
  deleteProfile: (id: string) => invoke<void>('delete_profile', { id }),
  getActiveProfileId: () => invoke<string | null>('get_active_profile_id'),

  readCurrentSettings: () => invoke<ClaudeSettings | null>('read_current_settings'),

  activateProfile: (id: string) => invoke<ActivationResult>('activate_profile', { id }),
  detectClaudeProcesses: () => invoke<ClaudeProcess[]>('detect_claude_processes'),

  readHistory: (limit = 50) => invoke<ActivationLogEntry[]>('read_history', { limit }),
  restoreBackup: (backupPath: string) => invoke<void>('restore_backup', { backupPath }),

  importFromFile: (path: string) => invoke<ProfileFile>('import_from_file', { path }),
  importFromUrl: (url: string) => invoke<ProfileFile>('import_from_url', { url }),

  // M2/M4: project registry + auto-detect + tab-complete + layered apply
  listProjects: () => invoke<Project[]>('list_projects'),
  addProject: (path: string) => invoke<Project>('add_project', { path }),
  removeProject: (path: string) => invoke<void>('remove_project', { path }),
  renameProject: (path: string, displayName: string) =>
    invoke<Project>('rename_project', { path, displayName }),
  getProjectStatus: (path: string) => invoke<ProjectStatus>('get_project_status', { path }),

  applyProfileToProject: (
    profileId: string,
    projectPath: string,
    options: ApplyOptions,
  ) =>
    invoke<ApplyOutcome>('apply_profile_to_project', {
      profileId,
      projectPath,
      options,
    }),

  listScanRoots: () => invoke<ScanRoot[]>('list_scan_roots'),
  addScanRoot: (path: string) => invoke<ScanRoot[]>('add_scan_root', { path }),
  removeScanRoot: (path: string) => invoke<ScanRoot[]>('remove_scan_root', { path }),
  setScanRootEnabled: (path: string, enabled: boolean) =>
    invoke<ScanRoot[]>('set_scan_root_enabled', { path, enabled }),

  scanForProjects: () => invoke<DetectedProject[]>('scan_for_projects'),
  completePathPrefix: (prefix: string) =>
    invoke<string[]>('complete_path_prefix', { prefix }),
};
