export interface SkillSource {
  id: string;
  sourceType: 'git' | 'local';
  url: string;
  branch?: string;
  subdirectory?: string;
  autoUpdate: boolean;
  addedAt: string;
}

export interface ProjectSkillConfig {
  projectPath: string;
  listedSkills: string[];
  mode: 'allowlist' | 'blocklist';
}

export interface SkillEntry {
  name: string;
  path: string;
  source: 'managed' | 'external';
  sourceId?: string;
  version?: string;
  description?: string;
  scope: 'global' | 'project' | 'none';
}

export interface SkillUpdateResult {
  sourceId: string;
  updated: boolean;
  beforeVersion: string;
  afterVersion: string;
}

export interface PluginInfo {
  id: string;
  enabled: boolean;
}
