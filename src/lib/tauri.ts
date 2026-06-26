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
import type {
  PluginInfo,
  ProjectSkillConfig,
  SkillEntry,
  SkillSource,
  SkillUpdateResult,
} from './skillTypes';

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
  setProjectPinned: (path: string, pinned: boolean) =>
    invoke<Project>('set_project_pinned', { path, pinned }),
  getProjectStatus: (path: string) => invoke<ProjectStatus>('get_project_status', { path }),
  readProjectSettings: (projectPath: string, layer: 'shared' | 'local') =>
    invoke<string>('read_project_settings', { projectPath, layer }),
  writeProjectSettings: (projectPath: string, layer: 'shared' | 'local', content: string) =>
    invoke<void>('write_project_settings', { projectPath, layer, content }),

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

  // External terminal launcher
  listTerminalBackends: () =>
    invoke<{ id: TerminalBackendId; label: string }[]>('list_terminal_backends'),
  openInTerminal: (args: {
    projectPath: string;
    backend: TerminalBackendId;
    claudeBin?: string;
    customTemplate?: string;
  }) => invoke<void>('open_in_terminal', args),

  // Global OS-level keyboard shortcut to toggle the main window.
  // Pass `null` to unregister; pass a Tauri shortcut string
  // (e.g. "Alt+Cmd+KeyA") to register / replace.
  setGlobalShortcut: (binding: string | null) =>
    invoke<void>('set_global_shortcut', { binding }),

  writeThemeHint: (dark: boolean) =>
    invoke<void>('write_theme_hint', { dark }),

  openSettingsWindow: () =>
    invoke<void>('open_settings_window'),

  // Skill management
  listSkillSources: () => invoke<SkillSource[]>('list_skill_sources'),
  addSkillSource: (source: SkillSource) =>
    invoke<SkillSource>('add_skill_source', { source }),
  removeSkillSource: (id: string) => invoke<void>('remove_skill_source', { id }),
  updateSkillSource: (id: string) =>
    invoke<SkillUpdateResult>('update_skill_source', { id }),
  scanSkillLibrary: (projectPath?: string) =>
    invoke<SkillEntry[]>('scan_skill_library', { projectPath: projectPath ?? null }),
  getProjectSkills: (projectPath: string) =>
    invoke<ProjectSkillConfig>('get_project_skills', { projectPath }),
  toggleSkill: (projectPath: string, skillId: string, enabled: boolean) =>
    invoke<ProjectSkillConfig>('toggle_skill', { projectPath, skillId, enabled }),
  setSkillScope: (skillId: string, scope: string) =>
    invoke<void>('set_skill_scope', { skillId, scope }),
  applyProjectSkills: (projectPath: string) =>
    invoke<string[]>('apply_project_skills', { projectPath }),

  listPlugins: (projectPath?: string) =>
    invoke<PluginInfo[]>('list_plugins', { projectPath: projectPath ?? null }),
  togglePlugin: (projectPath: string, pluginId: string, enabled: boolean) =>
    invoke<void>('toggle_plugin', { projectPath, pluginId, enabled }),
};

export type TerminalBackendId = 'ghostty' | 'cmux' | 'apple-terminal' | 'custom';
