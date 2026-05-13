import { invoke } from '@tauri-apps/api/core';
import type { ProfileFile, ClaudeSettings } from './profileSchema';

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
};
