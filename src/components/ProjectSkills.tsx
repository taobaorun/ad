import { useEffect, useMemo, useState } from 'react';
import { useTranslation } from 'react-i18next';
import { useSkills } from '@/store/skills';
import type { SkillEntry } from '@/lib/skillTypes';
import { Search, ChevronDown, ChevronRight } from 'lucide-react';
import { Toggle } from './SkillToggle';

interface Props {
  projectPath: string;
}

export function ProjectSkills({ projectPath }: Props) {
  const entries = useSkills((s) => s.entries);
  const plugins = useSkills((s) => s.plugins);
  const loading = useSkills((s) => s.loading);
  const projectConfig = useSkills((s) => s.projectConfig);
  const loadProjectSkills = useSkills((s) => s.loadProjectSkills);
  const toggleSkill = useSkills((s) => s.toggleSkill);
  const setSkillScope = useSkills((s) => s.setSkillScope);
  const togglePlugin = useSkills((s) => s.togglePlugin);

  const [filter, setFilter] = useState('');
  const [demoting, setDemoting] = useState<string | null>(null);
  const [expanded, setExpanded] = useState<Record<string, boolean>>({});
  const [optimistic, setOptimistic] = useState<Record<string, boolean>>({});

  useEffect(() => {
    void loadProjectSkills(projectPath);
  }, [projectPath, loadProjectSkills]);

  const filtered = useMemo(() => {
    if (!filter) return entries;
    const q = filter.toLowerCase();
    return entries.filter(
      (e) =>
        e.name.toLowerCase().includes(q) ||
        e.description?.toLowerCase().includes(q) ||
        e.sourceId?.toLowerCase().includes(q),
    );
  }, [entries, filter]);

  const globalSkills = filtered.filter((e) => e.scope === 'global');
  const managedSkills = filtered.filter((e) => e.source === 'managed' && e.scope !== 'global');
  const externalSkills = filtered.filter((e) => e.source === 'external');

  const managedBySource = useMemo(() => {
    const groups: Record<string, SkillEntry[]> = {};
    for (const e of managedSkills) {
      const key = e.sourceId ?? 'unknown';
      (groups[key] ??= []).push(e);
    }
    return Object.entries(groups).sort(([a], [b]) => a.localeCompare(b));
  }, [managedSkills]);

  function isEnabled(entry: SkillEntry): boolean {
    if (entry.scope === 'global') return true;
    if (!entry.sourceId) return false;
    const id = `${entry.sourceId}/${entry.name}`;
    if (id in optimistic) return optimistic[id] ?? false;
    if (!projectConfig) return false;
    if (projectConfig.mode === 'blocklist') {
      return !projectConfig.listedSkills.includes(id);
    }
    return projectConfig.listedSkills.includes(id);
  }

  function sourceEnabledCount(skills: SkillEntry[]): number {
    return skills.filter(isEnabled).length;
  }

  async function handleToggle(entry: SkillEntry) {
    if (!entry.sourceId) return;
    const id = `${entry.sourceId}/${entry.name}`;
    const next = !isEnabled(entry);
    setOptimistic((prev) => ({ ...prev, [id]: next }));
    try {
      await toggleSkill(projectPath, id, next);
    } finally {
      setOptimistic((prev) => {
        const copy = { ...prev };
        delete copy[id];
        return copy;
      });
    }
  }

  async function handleToggleAll(skills: SkillEntry[], enable: boolean) {
    const batch: Record<string, boolean> = {};
    for (const entry of skills) {
      if (!entry.sourceId) continue;
      if (isEnabled(entry) === enable) continue;
      batch[`${entry.sourceId}/${entry.name}`] = enable;
    }
    setOptimistic((prev) => ({ ...prev, ...batch }));
    try {
      for (const [id, val] of Object.entries(batch)) {
        await toggleSkill(projectPath, id, val);
      }
    } finally {
      setOptimistic((prev) => {
        const copy = { ...prev };
        for (const id of Object.keys(batch)) delete copy[id];
        return copy;
      });
    }
  }

  async function handleDemote(entry: SkillEntry) {
    if (!entry.sourceId) return;
    const id = `${entry.sourceId}/${entry.name}`;
    await setSkillScope(id, 'none');
    setDemoting(null);
  }

  function toggleExpand(key: string) {
    setExpanded((prev) => ({ ...prev, [key]: !prev[key] }));
  }

  if (loading && entries.length === 0) {
    return (
      <div
        className="flex h-full items-center justify-center text-sm"
        style={{ color: 'var(--ds-fg-4)' }}
      >
        Loading skills...
      </div>
    );
  }

  return (
    <div className="flex h-full flex-col">
      <div
        className="flex items-center gap-2 px-4 py-2.5"
        style={{ borderBottom: '0.5px solid var(--ds-line)' }}
      >
        <Search className="h-3.5 w-3.5" style={{ color: 'var(--ds-fg-4)' }} />
        <input
          type="text"
          value={filter}
          onChange={(e) => setFilter(e.target.value)}
          placeholder="Filter skills..."
          className="flex-1 bg-transparent text-xs outline-none placeholder:text-[var(--ds-fg-4)]"
        />
        {entries.length > 0 && (
          <span className="text-[11px]" style={{ color: 'var(--ds-fg-4)' }}>
            {filtered.length}/{entries.length}
          </span>
        )}
      </div>

      <div className="flex-1 overflow-y-auto px-4 py-3">
        {globalSkills.length > 0 && (
          <SourceGroup
            title="Global"
            subtitle="所有项目可见"
            skills={globalSkills}
            isEnabled={isEnabled}
            onToggle={handleToggle}
            isGlobal
            collapsed={!expanded['__global__']}
            onToggleCollapse={() => toggleExpand('__global__')}
            enabledCount={sourceEnabledCount(globalSkills)}
            onDemote={(e) => setDemoting(e.name)}
          />
        )}

        {managedBySource.map(([sourceId, skills]) => (
          <SourceGroup
            key={sourceId}
            title={sourceId}
            subtitle={`${skills.length} skills`}
            skills={skills}
            isEnabled={isEnabled}
            onToggle={handleToggle}
            collapsed={!expanded[sourceId]}
            onToggleCollapse={() => toggleExpand(sourceId)}
            enabledCount={sourceEnabledCount(skills)}
            onToggleAll={(enable) => void handleToggleAll(skills, enable)}
          />
        ))}

        {externalSkills.length > 0 && (
          <SourceGroup
            title="External"
            subtitle="手动安装，AD 不管理"
            skills={externalSkills}
            isEnabled={() => true}
            onToggle={() => {}}
            disabled
            collapsed={!expanded['__external__']}
            onToggleCollapse={() => toggleExpand('__external__')}
            enabledCount={externalSkills.length}
          />
        )}

        {plugins.length > 0 && (
          <div
            className="mb-3 overflow-hidden rounded-lg"
            style={{ border: '0.5px solid var(--ds-line)' }}
          >
            <button
              type="button"
              onClick={() => toggleExpand('__plugins__')}
              className="flex w-full items-center gap-2 px-3 py-2 text-left transition-colors hover:bg-[var(--ds-bg-hover)]"
              style={{ background: 'var(--ds-bg-inset)' }}
            >
              {expanded['__plugins__'] ? (
                <ChevronDown className="h-3 w-3" style={{ color: 'var(--ds-fg-4)' }} />
              ) : (
                <ChevronRight className="h-3 w-3" style={{ color: 'var(--ds-fg-4)' }} />
              )}
              <span className="text-[12px] font-semibold">Plugins</span>
              <span className="text-[11px]" style={{ color: 'var(--ds-fg-4)' }}>
                {plugins.length} plugins
              </span>
              <span className="ml-auto text-[11px]" style={{ color: 'var(--ds-fg-4)' }}>
                {plugins.filter((p) => p.enabled).length}/{plugins.length}
              </span>
            </button>
            {expanded['__plugins__'] && (
              <div>
                {plugins.map((p) => {
                  const [name, registry] = p.id.split('@');
                  return (
                    <div
                      key={p.id}
                      className="flex items-center gap-3 py-[7px] text-[13px] transition-colors hover:bg-[var(--ds-bg-hover)]"
                      style={{
                        borderTop:
                          '0.5px solid color-mix(in srgb, var(--ds-line) 50%, transparent)',
                        paddingLeft: 28,
                        paddingRight: 12,
                      }}
                    >
                      <div className="min-w-[120px] font-medium">{name}</div>
                      <div
                        className="flex-1 truncate text-[12px]"
                        style={{ color: 'var(--ds-fg-3)' }}
                      >
                        {registry ?? ''}
                      </div>
                      <div className="w-9">
                        <Toggle
                          on={p.enabled}
                          onChange={() => void togglePlugin(projectPath, p.id, !p.enabled)}
                        />
                      </div>
                    </div>
                  );
                })}
              </div>
            )}
          </div>
        )}

        {entries.length === 0 && plugins.length === 0 && (
          <div className="py-8 text-center text-xs" style={{ color: 'var(--ds-fg-4)' }}>
            No skills or plugins found. Go to Settings → Skill Sources to add a skill source.
          </div>
        )}
      </div>

      {demoting && (
        <DemoteDialog
          skillName={demoting}
          onConfirm={() => {
            const entry = entries.find((e) => e.name === demoting);
            if (entry) void handleDemote(entry);
          }}
          onCancel={() => setDemoting(null)}
        />
      )}
    </div>
  );
}

function SourceGroup({
  title,
  subtitle,
  skills,
  isEnabled,
  onToggle,
  isGlobal,
  disabled,
  collapsed,
  onToggleCollapse,
  enabledCount,
  onToggleAll,
  onDemote,
}: {
  title: string;
  subtitle: string;
  skills: SkillEntry[];
  isEnabled: (e: SkillEntry) => boolean;
  onToggle: (e: SkillEntry) => void;
  isGlobal?: boolean;
  disabled?: boolean;
  collapsed?: boolean;
  onToggleCollapse: () => void;
  enabledCount: number;
  onToggleAll?: (enable: boolean) => void;
  onDemote?: (e: SkillEntry) => void;
}) {
  const allOn = enabledCount === skills.length;

  return (
    <div
      className="mb-3 overflow-hidden rounded-lg"
      style={{ border: '0.5px solid var(--ds-line)' }}
    >
      <button
        type="button"
        onClick={onToggleCollapse}
        className="flex w-full items-center gap-2 px-3 py-2 text-left transition-colors hover:bg-[var(--ds-bg-hover)]"
        style={{ background: 'var(--ds-bg-inset)' }}
      >
        {collapsed ? (
          <ChevronRight className="h-3 w-3" style={{ color: 'var(--ds-fg-4)' }} />
        ) : (
          <ChevronDown className="h-3 w-3" style={{ color: 'var(--ds-fg-4)' }} />
        )}
        <span className="text-[12px] font-semibold">{title}</span>
        <span className="text-[11px]" style={{ color: 'var(--ds-fg-4)' }}>
          {subtitle}
        </span>
        <span className="ml-auto text-[11px]" style={{ color: 'var(--ds-fg-4)' }}>
          {enabledCount}/{skills.length}
        </span>
        {onToggleAll && !disabled && (
          <div onClick={(e) => e.stopPropagation()}>
            <Toggle on={allOn} onChange={() => onToggleAll(!allOn)} />
          </div>
        )}
      </button>

      {!collapsed && (
        <div>
          {skills.map((entry) => (
            <SkillRow
              key={`${entry.sourceId ?? 'ext'}/${entry.name}`}
              entry={entry}
              enabled={isEnabled(entry)}
              onToggle={() => onToggle(entry)}
              isGlobal={isGlobal}
              disabled={disabled}
              onDemote={onDemote ? () => onDemote(entry) : undefined}
            />
          ))}
        </div>
      )}
    </div>
  );
}

function SkillRow({
  entry,
  enabled,
  onToggle,
  isGlobal,
  disabled,
  onDemote,
}: {
  entry: SkillEntry;
  enabled: boolean;
  onToggle: () => void;
  isGlobal?: boolean;
  disabled?: boolean;
  onDemote?: () => void;
}) {
  return (
    <div
      className="flex items-center gap-3 py-[7px] text-[13px] transition-colors hover:bg-[var(--ds-bg-hover)]"
      style={{
        borderTop: '0.5px solid color-mix(in srgb, var(--ds-line) 50%, transparent)',
        paddingLeft: 28,
        paddingRight: 12,
      }}
    >
      <div className="min-w-[120px] font-medium">{entry.name}</div>
      <div className="flex-1 truncate text-[12px]" style={{ color: 'var(--ds-fg-3)' }}>
        {entry.description ?? ''}
      </div>
      <div className="w-[52px] text-center">
        {isGlobal && onDemote ? (
          <button
            type="button"
            onClick={onDemote}
            className="rounded-full px-2 py-0.5 text-[10px] transition-colors"
            style={{
              background: 'rgb(var(--color-action-primary) / 0.15)',
              color: 'rgb(var(--color-action-primary))',
              border: '1px solid rgb(var(--color-action-primary) / 0.3)',
            }}
          >
            全局
          </button>
        ) : null}
      </div>
      <div className="w-9">
        {disabled ? (
          <span className="text-[10px]" style={{ color: 'var(--ds-fg-4)', opacity: 0.5 }}>
            read-only
          </span>
        ) : (
          <Toggle on={enabled} onChange={onToggle} disabled={isGlobal} />
        )}
      </div>
    </div>
  );
}

function DemoteDialog({
  skillName,
  onConfirm,
  onCancel,
}: {
  skillName: string;
  onConfirm: () => void;
  onCancel: () => void;
}) {
  const { t } = useTranslation();

  return (
    <div className="fixed inset-0 z-50 flex items-center justify-center bg-overlay/65">
      <div
        className="w-[400px] rounded-xl p-5"
        style={{ background: 'var(--ds-bg-card)', border: '1px solid var(--ds-line)' }}
      >
        <h3 className="mb-3 text-sm font-semibold">
          {t('projectSkills.demote.title', { skillName })}
        </h3>
        <p className="mb-4 text-xs" style={{ color: 'var(--ds-fg-3)' }}>
          {t('projectSkills.demote.description')}
        </p>
        <div className="flex justify-end gap-2">
          <button
            type="button"
            onClick={onCancel}
            className="rounded-md border px-3 py-1.5 text-xs"
            style={{ borderColor: 'var(--ds-line)' }}
          >
            {t('projectSkills.demote.cancel')}
          </button>
          <button
            type="button"
            onClick={onConfirm}
            className="rounded-md border border-border px-3 py-1.5 text-xs hover:bg-foreground/5"
          >
            {t('projectSkills.demote.confirm')}
          </button>
        </div>
      </div>
    </div>
  );
}
