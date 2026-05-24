/**
 * Project detail pane — ad-redesign layout.
 *
 * Sections:
 *   1. Header — name / path / status pills
 *   2. 当前生效 — hero card (active profile + layer tag)
 *   3. 层级优先级 — per-layer stack (env › local › shared)
 *   4. 切换配置 — profile cards grid + apply bar
 *   5. 最近应用 — history list
 *   6. 项目配置 — settings.local.json editor (collapsible)
 */

import { useCallback, useEffect, useMemo, useState, type CSSProperties, type ReactNode } from 'react';
import { useTranslation } from 'react-i18next';
import Editor from '@monaco-editor/react';
import { useUiState } from '@/store/ui';
import { useProjects } from '@/store/projects';
import { useProfiles } from '@/store/profiles';
import { tauri } from '@/lib/tauri';
import { useGlobalKeymap } from '@/lib/keymap';
import { InlineConflictResolver } from './InlineConflictResolver';
import { AlertTriangle, CheckCircle2, ChevronDown, ChevronRight, Copy, Pencil, Trash2 } from 'lucide-react';
import type {
  Conflict,
  Project,
  ProjectStatus,
  Resolution,
  ApplyOptions,
  ApplyOutcome,
} from '@/lib/projectTypes';
import type { ProfileFile } from '@/lib/profileSchema';

const LAYER_ORDER = ['env', 'local', 'shared'] as const;
type LayerKey = typeof LAYER_ORDER[number];

export function ProjectDetail() {
  const { t } = useTranslation();
  const activePath = useUiState((s) => s.activeProjectPath);
  const projects = useProjects((s) => s.projects);
  const project = projects.find((p) => p.path === activePath) ?? null;

  if (!project) {
    return (
      <div className="flex h-full items-center justify-center text-sm" style={{ color: 'var(--ds-fg-4)' }}>
        {t('detail.selectPrefix')}
        <KbdChip className="mx-1">⌘1</KbdChip>
        {t('detail.selectSuffix')}
      </div>
    );
  }
  return <Detail project={project} key={project.path} />;
}

function Detail({ project }: { project: Project }) {
  const { t } = useTranslation();
  const profiles = useProfiles((s) => s.profiles);
  const reloadProjects = useProjects((s) => s.loadAll);
  const openEditDrawer = useUiState((s) => s.openEditDrawer);
  const removeProject = useProjects((s) => s.removeProject);

  const initialProfileId =
    project.currentProfileId ??
    project.lastApplied?.profileId ??
    profiles[0]?.id ?? '';

  const initialTargetLayer: LayerKey = useMemo(() => {
    const last = project.lastApplied?.layers;
    if (last && last.length > 0) return last[0] as LayerKey;
    return 'local';
  }, [project.lastApplied]);

  const [profileId, setProfileId] = useState<string>(initialProfileId);
  const [targetLayer, setTargetLayer] = useState<LayerKey>(initialTargetLayer);
  const [dirtyAcked, setDirtyAcked] = useState(false);
  const [resolutions, setResolutions] = useState<Record<string, Resolution>>({});
  const [pendingConflicts, setPendingConflicts] = useState<{ layer: string; conflicts: Conflict[] } | null>(null);

  const [status, setStatus] = useState<ProjectStatus | null>(null);
  const [busy, setBusy] = useState(false);
  const [error, setError] = useState<string | null>(null);
  const [appliedFlash, setAppliedFlash] = useState<ApplyOutcome | null>(null);

  useEffect(() => {
    setProfileId(initialProfileId);
    setTargetLayer(initialTargetLayer);
    setDirtyAcked(false);
    setResolutions({});
    setPendingConflicts(null);
    setAppliedFlash(null);
    setError(null);
  }, [project.path, initialProfileId, initialTargetLayer]);

  useEffect(() => {
    void (async () => {
      try { setStatus(await tauri.getProjectStatus(project.path)); }
      catch { setStatus(null); }
    })();
  }, [project.path, project.lastApplied]);

  const sharedAndDirty = targetLayer === 'shared' && status?.gitDirty === true;
  const dirtyBlocked = sharedAndDirty && !dirtyAcked;
  const allConflictsResolved =
    !pendingConflicts ||
    pendingConflicts.conflicts.every((c) => resolutions[c.keyPath] != null);

  const canApply = !!profileId && !dirtyBlocked && allConflictsResolved && !busy;

  async function doApply() {
    if (!canApply) return;
    setBusy(true);
    setError(null);
    setAppliedFlash(null);
    try {
      const opts: ApplyOptions = {
        layers: [targetLayer],
        resolutions,
        overwriteDirtyWarningAcked: dirtyAcked,
      };
      const result = await tauri.applyProfileToProject(profileId, project.path, opts);
      if (result.kind === 'applied') {
        setAppliedFlash(result);
        setPendingConflicts(null);
        setResolutions({});
        await reloadProjects();
        window.setTimeout(() => setAppliedFlash(null), 4000);
      } else if (result.kind === 'needsResolution') {
        setPendingConflicts({ layer: result.layer, conflicts: result.conflicts });
        setResolutions((prev) => {
          const next = { ...prev };
          for (const c of result.conflicts) {
            if (!next[c.keyPath]) next[c.keyPath] = { kind: 'keepExisting' };
          }
          return next;
        });
      } else {
        setError(`Git dirty — ${result.message}`);
      }
    } catch (e) {
      setError(e instanceof Error ? e.message : String(e));
    } finally {
      setBusy(false);
    }
  }

  useGlobalKeymap({
    'mod+enter': () => { void doApply(); },
    'mod+e': () => {
      const id = project.currentProfileId ?? profileId;
      if (id) openEditDrawer(id);
    },
  });

  const currentProfile = profiles.find((p) => p.id === project.currentProfileId);
  const selectedProfile = profiles.find((p) => p.id === profileId);

  // Derive per-layer profile assignments from lastApplied
  const layerProfiles: Record<LayerKey, string | null> = useMemo(() => {
    const r: Record<LayerKey, string | null> = { env: null, local: null, shared: null };
    if (project.lastApplied) {
      for (const l of project.lastApplied.layers) {
        if (l === 'env' || l === 'local' || l === 'shared') {
          r[l] = project.lastApplied.profileId;
        }
      }
    }
    return r;
  }, [project.lastApplied]);

  // Active = first non-null in env › local › shared order
  const activeLayer = LAYER_ORDER.find((l) => layerProfiles[l] != null) ?? null;

  return (
    <div
      className="h-full w-full overflow-y-auto"
      style={{ background: 'hsl(var(--background))', scrollbarGutter: 'stable' }}
    >
      <div style={{ maxWidth: 720, margin: '0 auto', padding: '32px 40px 64px' }}>

        {/* Header */}
        <div style={{ display: 'flex', alignItems: 'flex-start', justifyContent: 'space-between', gap: 16 }}>
          <div style={{ minWidth: 0, flex: 1 }}>
            <h1 style={{ fontSize: 24, fontWeight: 600, letterSpacing: '-0.02em', color: 'hsl(var(--foreground))', margin: 0 }}>
              {project.displayName}
            </h1>
            <div className="font-mono text-[12.5px] mt-1.5" style={{ color: 'var(--ds-fg-3)' }}>
              {project.path}
            </div>
            <div style={{ display: 'flex', flexWrap: 'wrap', gap: 6, marginTop: 14 }}>
              {status && (
                <>
                  <StatusPill ok={!status.gitDirty && status.isGitRepo} warn={status.gitDirty}>
                    {status.isGitRepo
                      ? (status.gitDirty ? t('detail.gitDirty') : t('detail.gitClean'))
                      : t('detail.notARepo')}
                  </StatusPill>
                  {status.claudeDirExists && (
                    <StatusPill ok>{t('detail.claudeDirPresent')}</StatusPill>
                  )}
                  {status.isGitRepo && status.gitignoreExcludesSettingsLocal === false && (
                    <StatusPill warn>{t('detail.settingsLocalNotIgnored')}</StatusPill>
                  )}
                </>
              )}
            </div>
          </div>
          <button
            type="button"
            onClick={() => {
              if (window.confirm(t('detail.removeConfirm', { name: project.displayName }))) {
                void removeProject(project.path);
              }
            }}
            title={t('detail.removeTitle')}
            style={{
              display: 'inline-flex', alignItems: 'center', justifyContent: 'center',
              width: 30, height: 30, borderRadius: 7,
              background: 'transparent', border: 0, color: 'var(--ds-fg-4)', cursor: 'pointer',
            }}
            onMouseEnter={(e) => {
              (e.currentTarget as HTMLElement).style.background = 'var(--ds-danger-soft)';
              (e.currentTarget as HTMLElement).style.color = 'var(--ds-danger)';
            }}
            onMouseLeave={(e) => {
              (e.currentTarget as HTMLElement).style.background = 'transparent';
              (e.currentTarget as HTMLElement).style.color = 'var(--ds-fg-4)';
            }}
          >
            <Trash2 className="h-4 w-4" />
          </button>
        </div>

        {/* CURRENT ACTIVE */}
        <Section title={t('detail.sections.current')}>
          {currentProfile ? (
            <HeroCard
              profile={currentProfile}
              layer={activeLayer}
              onEdit={() => openEditDrawer(currentProfile.id)}
              t={t}
            />
          ) : (
            <div
              style={{
                borderRadius: 10,
                background: 'var(--ds-bg-inset)',
                border: '0.5px solid var(--ds-line)',
                padding: '18px 20px',
                fontSize: 13,
                color: 'var(--ds-fg-4)',
                fontStyle: 'italic',
              }}
            >
              {t('detail.noProfileApplied')}
            </div>
          )}
        </Section>

        {/* LAYER PRECEDENCE */}
        <Section title={t('detail.sections.layerPrecedence')} hint="env › local › shared">
          <LayerStack
            layerProfiles={layerProfiles}
            profiles={profiles}
            activeLayer={activeLayer}
            targetLayer={targetLayer}
            onLayerClick={setTargetLayer}
          />
        </Section>

        {/* SWITCH PROFILE */}
        <Section
          title={t('detail.sections.switchProfile')}
          hint={<>按数字键 <KbdChip>1</KbdChip> <KbdChip>2</KbdChip> <KbdChip>3</KbdChip></>}
        >
          <ProfileCards
            profiles={profiles}
            selectedId={profileId}
            activeId={currentProfile?.id ?? null}
            activeLayer={activeLayer}
            layerProfiles={layerProfiles}
            onSelect={setProfileId}
          />

          <ApplyBar
            selectedProfile={selectedProfile ?? null}
            targetLayer={targetLayer}
            onLayerChange={setTargetLayer}
            onApply={() => void doApply()}
            busy={busy}
            canApply={canApply}
            hasConflicts={!!pendingConflicts}
          />

          {sharedAndDirty && (
            <div
              style={{
                marginTop: 12,
                borderRadius: 8,
                border: '0.5px solid rgba(194,65,12,0.30)',
                background: 'var(--ds-warning-soft)',
                padding: '10px 14px',
                fontSize: 12.5,
                color: 'var(--ds-warning)',
              }}
            >
              <div style={{ display: 'flex', alignItems: 'center', gap: 6, fontWeight: 600, marginBottom: 6 }}>
                <AlertTriangle className="h-3.5 w-3.5" />
                {t('detail.dirtyTitle')}
              </div>
              <p style={{ margin: '0 0 8px', color: 'var(--ds-fg-2)', fontSize: 12 }}>{t('detail.dirtyDesc')}</p>
              <label style={{ display: 'flex', alignItems: 'center', gap: 8, cursor: 'pointer', fontSize: 12 }}>
                <input
                  type="checkbox"
                  checked={dirtyAcked}
                  onChange={(e) => setDirtyAcked(e.target.checked)}
                />
                {t('detail.applyAnyway')}
              </label>
            </div>
          )}

          {pendingConflicts && (
            <div style={{ marginTop: 12 }}>
              <InlineConflictResolver
                conflicts={pendingConflicts.conflicts}
                resolutions={resolutions}
                onChange={setResolutions}
              />
            </div>
          )}

          {error && (
            <div
              style={{
                marginTop: 10,
                borderRadius: 6,
                border: '0.5px solid var(--ds-danger)',
                background: 'var(--ds-danger-soft)',
                padding: '8px 12px',
                fontSize: 12,
                color: 'var(--ds-danger)',
              }}
            >
              {error}
            </div>
          )}

          {appliedFlash?.kind === 'applied' && (
            <div
              style={{
                marginTop: 10,
                display: 'flex',
                alignItems: 'center',
                gap: 6,
                fontSize: 12,
                color: 'var(--ds-ok)',
              }}
            >
              <CheckCircle2 className="h-3.5 w-3.5" />
              {t('detail.appliedFlash', { count: appliedFlash.writtenFiles.length })}
            </div>
          )}

          {appliedFlash?.kind === 'applied' && appliedFlash.envExportSnippet && (
            <div
              style={{
                marginTop: 10,
                borderRadius: 8,
                border: '0.5px solid var(--ds-line)',
                background: 'var(--ds-bg-inset)',
                padding: '10px 14px',
              }}
            >
              <div style={{ display: 'flex', alignItems: 'center', gap: 8, fontSize: 12, color: 'var(--ds-fg-3)', marginBottom: 8 }}>
                <span>{t('detail.envExports')}</span>
                <button
                  type="button"
                  onClick={() => void navigator.clipboard.writeText(appliedFlash.envExportSnippet ?? '')}
                  style={dsBtn}
                >
                  <Copy className="h-3 w-3" />
                  {t('detail.copy')}
                </button>
              </div>
              <pre className="font-mono overflow-auto rounded text-[11px]" style={{ background: 'hsl(var(--background))', padding: 8 }}>
                {appliedFlash.envExportSnippet}
              </pre>
            </div>
          )}
        </Section>

        {/* RECENT APPLIES */}
        <Section title={t('detail.sections.recent')}>
          {project.lastApplied ? (
            <RecentList
              applies={[project.lastApplied]}
              profiles={profiles}
            />
          ) : (
            <div style={{ fontSize: 13, color: 'var(--ds-fg-4)', padding: '10px 0' }}>
              {t('detail.noApplies')}
            </div>
          )}
        </Section>

        {/* PROJECT SETTINGS */}
        <ProjectSettingsSection projectPath={project.path} />
      </div>
    </div>
  );
}

// ---------------------------------------------------------------------------
// HeroCard
// ---------------------------------------------------------------------------

function HeroCard({
  profile,
  layer,
  onEdit,
  t,
}: {
  profile: ProfileFile;
  layer: LayerKey | null;
  onEdit: () => void;
  t: (k: string, o?: Record<string, unknown>) => string;
}) {
  const mark = (profile.displayName ?? '?').charAt(0).toUpperCase();
  const fileName = layer === 'shared' ? 'settings.json'
    : layer === 'local' ? 'settings.local.json'
    : layer === 'env' ? '(env)' : '—';

  return (
    <div
      style={{
        position: 'relative',
        borderRadius: 10,
        background: 'var(--ds-bg-card)',
        border: '0.5px solid var(--ds-line-strong)',
        padding: '18px 18px 18px 20px',
        display: 'grid',
        gridTemplateColumns: 'auto 1fr auto',
        alignItems: 'center',
        gap: 16,
        overflow: 'hidden',
      }}
    >
      {/* Left accent bar */}
      <div style={{ position: 'absolute', left: 0, top: 0, bottom: 0, width: 3, background: profile.color }} />

      {/* Avatar */}
      <div style={{ position: 'relative' }}>
        <div
          style={{
            width: 38, height: 38, borderRadius: 9,
            background: profile.color,
            display: 'flex', alignItems: 'center', justifyContent: 'center',
            fontWeight: 600, fontSize: 14, color: '#fff',
          }}
        >
          {mark}
        </div>
        <div
          style={{
            position: 'absolute', inset: -3, borderRadius: 12,
            border: `1.5px solid ${profile.color}44`,
            pointerEvents: 'none',
          }}
        />
      </div>

      {/* Text */}
      <div>
        <div style={{ display: 'flex', alignItems: 'center', gap: 8, fontSize: 16, fontWeight: 600, color: 'hsl(var(--foreground))' }}>
          {profile.displayName}
          {layer && (
            <span
              className="font-mono"
              style={{
                fontSize: 11.5, fontWeight: 500,
                color: 'var(--ds-accent)',
                background: 'var(--ds-accent-soft)',
                border: '0.5px solid var(--ds-accent-line)',
                padding: '1px 6px', borderRadius: 5,
              }}
            >
              {layer}
            </span>
          )}
        </div>
        <div className="font-mono" style={{ fontSize: 12.5, color: 'var(--ds-fg-3)', marginTop: 4, display: 'flex', alignItems: 'center', gap: 8 }}>
          <span style={{ color: 'var(--ds-ok)' }}>✓</span>
          <span>正在生效 · .claude/{fileName}</span>
        </div>
      </div>

      {/* Actions */}
      <div>
        <button
          type="button"
          onClick={onEdit}
          title={t('detail.editProfileTitle')}
          style={dsBtn}
        >
          <Pencil className="h-3.5 w-3.5" />
          {t('detail.edit')}
        </button>
      </div>
    </div>
  );
}

// ---------------------------------------------------------------------------
// LayerStack
// ---------------------------------------------------------------------------

function LayerStack({
  layerProfiles,
  profiles,
  activeLayer,
  targetLayer,
  onLayerClick,
}: {
  layerProfiles: Record<LayerKey, string | null>;
  profiles: ProfileFile[];
  activeLayer: LayerKey | null;
  targetLayer: LayerKey;
  onLayerClick: (l: LayerKey) => void;
}) {
  return (
    <div style={{ borderRadius: 10, background: 'var(--ds-bg-card)', border: '0.5px solid var(--ds-line)', overflow: 'hidden' }}>
      {LAYER_ORDER.map((L, i) => {
        const pid = layerProfiles[L];
        const prof = pid ? profiles.find((p) => p.id === pid) : null;
        const isActive = L === activeLayer;
        const isOverridden = !!prof && !isActive;
        const isEmpty = !prof;
        const isTarget = L === targetLayer;

        return (
          <div
            key={L}
            onClick={() => onLayerClick(L)}
            style={{
              display: 'grid',
              gridTemplateColumns: '80px 1fr auto',
              alignItems: 'center',
              gap: 16,
              padding: '12px 18px',
              borderBottom: i < LAYER_ORDER.length - 1 ? '0.5px solid var(--ds-line)' : undefined,
              position: 'relative',
              cursor: 'pointer',
              background: isActive
                ? 'linear-gradient(90deg, var(--ds-accent-soft), transparent 80%)'
                : isTarget && !isActive
                ? 'var(--ds-bg-inset)'
                : undefined,
            }}
          >
            {/* Active layer accent bar */}
            {isActive && (
              <div style={{ position: 'absolute', left: 0, top: 0, bottom: 0, width: 2, background: 'var(--ds-accent)' }} />
            )}

            {/* Layer name */}
            <div
              className="font-mono"
              style={{
                fontSize: 12.5,
                fontWeight: 500,
                color: isEmpty ? 'var(--ds-fg-4)' : isActive ? 'hsl(var(--foreground))' : 'var(--ds-fg-2)',
                display: 'flex', alignItems: 'center', gap: 7,
              }}
            >
              <PrecedenceMark idx={i} active={isActive} />
              {L}
            </div>

            {/* Content */}
            <div
              style={{
                display: 'flex', alignItems: 'center', gap: 10,
                fontSize: 13,
                color: isEmpty ? 'var(--ds-fg-4)' : 'hsl(var(--foreground))',
                fontStyle: isEmpty ? 'italic' : undefined,
                opacity: isOverridden ? 0.55 : 1,
                textDecoration: isOverridden ? 'line-through' : undefined,
              }}
            >
              <span
                style={{
                  width: 8, height: 8, borderRadius: '50%', flexShrink: 0,
                  background: prof ? prof.color : 'transparent',
                  border: isEmpty ? '1px dashed var(--ds-fg-5)' : undefined,
                }}
              />
              {prof ? prof.displayName : '未设置'}
            </div>

            {/* Status */}
            <div
              className="font-mono"
              style={{
                fontSize: 11,
                color: isActive ? 'var(--ds-accent)' : 'var(--ds-fg-4)',
                fontWeight: isActive ? 500 : undefined,
              }}
            >
              {isActive ? '↑ 生效' : isOverridden ? '被覆盖' : ''}
            </div>
          </div>
        );
      })}
    </div>
  );
}

function PrecedenceMark({ idx, active }: { idx: number; active: boolean }) {
  return (
    <svg width="14" height="14" viewBox="0 0 14 14" aria-hidden="true">
      <rect x="2" y="2"  width="10" height="2" rx="1" fill="currentColor" opacity={idx === 0 ? (active ? 1 : 0.85) : 0.18}/>
      <rect x="2" y="6"  width="10" height="2" rx="1" fill="currentColor" opacity={idx === 1 ? (active ? 1 : 0.85) : 0.18}/>
      <rect x="2" y="10" width="10" height="2" rx="1" fill="currentColor" opacity={idx === 2 ? (active ? 1 : 0.85) : 0.18}/>
    </svg>
  );
}

// ---------------------------------------------------------------------------
// ProfileCards
// ---------------------------------------------------------------------------

function ProfileCards({
  profiles,
  selectedId,
  activeId,
  activeLayer,
  layerProfiles,
  onSelect,
}: {
  profiles: ProfileFile[];
  selectedId: string;
  activeId: string | null;
  activeLayer: LayerKey | null;
  layerProfiles: Record<LayerKey, string | null>;
  onSelect: (id: string) => void;
}) {
  return (
    <div style={{ display: 'grid', gridTemplateColumns: 'repeat(3, 1fr)', gap: 10 }}>
      {profiles.map((p, i) => {
        const isActive = p.id === activeId;
        const isSelected = p.id === selectedId;
        const layerOfThis = LAYER_ORDER.find((l) => layerProfiles[l] === p.id) ?? null;
        const mark = (p.displayName ?? '?').charAt(0).toUpperCase();

        return (
          <button
            key={p.id}
            type="button"
            onClick={() => onSelect(p.id)}
            style={{
              position: 'relative',
              background: isActive ? 'var(--ds-accent-soft)' : 'var(--ds-bg-card)',
              border: isSelected
                ? `1px solid var(--ds-accent)`
                : '0.5px solid var(--ds-line-strong)',
              borderRadius: 10,
              padding: 16,
              cursor: 'pointer',
              display: 'flex', flexDirection: 'column', gap: 10,
              minHeight: 112,
              textAlign: 'left',
              fontFamily: 'inherit',
              boxShadow: isSelected
                ? '0 0 0 1px var(--ds-accent), 0 4px 14px rgba(91,91,214,0.14)'
                : undefined,
              transition: 'transform 120ms ease, box-shadow 120ms ease',
            }}
          >
            <div style={{ display: 'flex', alignItems: 'flex-start', justifyContent: 'space-between' }}>
              <div
                style={{
                  width: 28, height: 28, borderRadius: 7,
                  background: p.color,
                  display: 'flex', alignItems: 'center', justifyContent: 'center',
                  color: '#fff', fontWeight: 600, fontSize: 12,
                }}
              >
                {mark}
              </div>
              <span
                className="font-mono"
                style={{
                  fontSize: 10.5,
                  color: 'var(--ds-fg-4)',
                  background: 'var(--ds-bg-soft)',
                  border: '0.5px solid var(--ds-line)',
                  padding: '1px 5px', borderRadius: 4,
                }}
              >
                {i + 1}
              </span>
            </div>
            <div>
              <div style={{ fontSize: 14, fontWeight: 600, color: 'hsl(var(--foreground))' }}>{p.displayName}</div>
              {p.description && (
                <div className="font-mono" style={{ fontSize: 11.5, color: 'var(--ds-fg-3)', marginTop: 2 }}>
                  {p.description}
                </div>
              )}
            </div>
            <div
              className="font-mono"
              style={{
                display: 'flex', alignItems: 'center', gap: 6,
                fontSize: 11,
                color: 'var(--ds-fg-4)',
                marginTop: 'auto',
              }}
            >
              {isActive ? (
                <>
                  <svg width="9" height="9" viewBox="0 0 10 10">
                    <circle cx="5" cy="5" r="3" fill="var(--ds-accent)"/>
                  </svg>
                  <span style={{ color: 'var(--ds-accent)', fontWeight: 500 }}>
                    生效中 · {activeLayer}
                  </span>
                </>
              ) : layerOfThis ? (
                <>
                  <span style={{ opacity: 0.6 }}>已设置于</span>
                  <span style={{ color: 'var(--ds-fg-2)' }}>{layerOfThis}</span>
                </>
              ) : (
                <span style={{ opacity: 0.6 }}>未设置</span>
              )}
            </div>
          </button>
        );
      })}
    </div>
  );
}

// ---------------------------------------------------------------------------
// ApplyBar
// ---------------------------------------------------------------------------

function ApplyBar({
  selectedProfile,
  targetLayer,
  onLayerChange,
  onApply,
  busy,
  canApply,
  hasConflicts,
}: {
  selectedProfile: ProfileFile | null;
  targetLayer: LayerKey;
  onLayerChange: (l: LayerKey) => void;
  onApply: () => void;
  busy: boolean;
  canApply: boolean;
  hasConflicts: boolean;
}) {
  const fileName = targetLayer === 'shared' ? 'settings.json'
    : targetLayer === 'env' ? '(env, no file)'
    : 'settings.local.json';

  const layerOrder: LayerKey[] = ['shared', 'local', 'env'];

  return (
    <div
      style={{
        marginTop: 16,
        border: '0.5px solid var(--ds-line)',
        borderRadius: 10,
        background: 'var(--ds-bg-inset)',
        padding: '14px 18px',
        display: 'flex', alignItems: 'center', gap: 16,
      }}
    >
      {/* Layer target */}
      <div style={{ display: 'flex', alignItems: 'center', gap: 8, fontSize: 12.5, color: 'var(--ds-fg-3)' }}>
        <span style={{ color: 'var(--ds-fg-4)' }}>写入到</span>
        <div
          style={{
            display: 'inline-flex',
            background: 'var(--ds-bg-card)',
            borderRadius: 7,
            padding: 2,
            border: '0.5px solid var(--ds-line)',
          }}
        >
          {layerOrder.map((L) => (
            <button
              key={L}
              type="button"
              onClick={() => onLayerChange(L)}
              className="font-mono"
              style={{
                fontSize: 11.5, fontWeight: 500,
                background: L === targetLayer ? 'hsl(var(--foreground))' : 'transparent',
                color: L === targetLayer ? 'hsl(var(--background))' : 'var(--ds-fg-3)',
                border: 0,
                padding: '4px 10px',
                borderRadius: 5,
                cursor: 'pointer',
              }}
            >
              {L}
            </button>
          ))}
        </div>
      </div>

      {/* Arrow + file */}
      <div
        className="font-mono"
        style={{ display: 'flex', alignItems: 'center', gap: 10, fontSize: 12, color: 'var(--ds-fg-3)' }}
      >
        <span>→</span>
        <span style={{ color: 'var(--ds-fg-2)' }}>.claude/{fileName}</span>
      </div>

      {/* Apply button */}
      <button
        type="button"
        onClick={onApply}
        disabled={!canApply}
        style={{
          marginLeft: 'auto',
          display: 'inline-flex', alignItems: 'center', gap: 8,
          height: 34,
          padding: '0 14px',
          borderRadius: 7,
          fontFamily: 'inherit',
          fontSize: 13,
          fontWeight: 500,
          background: canApply ? 'var(--ds-accent)' : 'var(--ds-fg-5)',
          color: canApply ? '#fff' : 'var(--ds-fg-3)',
          border: 'none',
          boxShadow: canApply ? '0 1px 2px rgba(91,91,214,0.20), inset 0 1px 0 rgba(255,255,255,0.18)' : undefined,
          cursor: canApply ? 'pointer' : 'not-allowed',
          opacity: canApply ? 1 : 0.45,
        }}
      >
        {busy ? '应用中…'
          : hasConflicts ? '按解决方案应用'
          : selectedProfile ? `应用 ${selectedProfile.displayName}`
          : '选择一个配置'}
        <KbdChip style={{ background: 'rgba(255,255,255,0.16)', border: 'none', color: 'rgba(255,255,255,0.85)' }}>
          ⌘↵
        </KbdChip>
      </button>
    </div>
  );
}

// ---------------------------------------------------------------------------
// RecentList
// ---------------------------------------------------------------------------

function RecentList({
  applies,
  profiles,
}: {
  applies: Array<{ profileId: string; layers: string[]; timestamp: string; conflictsResolved: number }>;
  profiles: ProfileFile[];
}) {
  const { t } = useTranslation();
  return (
    <div style={{ borderRadius: 10, border: '0.5px solid var(--ds-line)', background: 'var(--ds-bg-card)', overflow: 'hidden' }}>
      {applies.map((r, i) => {
        const prof = profiles.find((p) => p.id === r.profileId);
        const layerLabel = r.layers[0] ?? '—';
        const timeStr = new Date(r.timestamp).toLocaleString();
        return (
          <div
            key={i}
            style={{
              display: 'grid',
              gridTemplateColumns: 'auto 1fr auto',
              alignItems: 'center',
              gap: 14,
              padding: '11px 18px',
              fontSize: 13,
              borderBottom: i < applies.length - 1 ? '0.5px solid var(--ds-line)' : undefined,
            }}
          >
            <span style={{ width: 7, height: 7, borderRadius: '50%', background: prof?.color ?? 'var(--ds-fg-5)', display: 'block' }} />
            <div style={{ display: 'flex', alignItems: 'center', gap: 8, color: 'hsl(var(--foreground))' }}>
              {prof?.displayName ?? r.profileId}
              <span
                className="font-mono"
                style={{
                  fontSize: 11,
                  color: 'var(--ds-fg-3)',
                  background: 'var(--ds-bg-soft)',
                  padding: '1px 5px',
                  borderRadius: 4,
                  border: '0.5px solid var(--ds-line)',
                }}
              >
                {layerLabel}
              </span>
              {r.conflictsResolved > 0 && (
                <span className="font-mono" style={{ fontSize: 11, color: 'var(--ds-warning)' }}>
                  {t('detail.conflictsResolved', { count: r.conflictsResolved })}
                </span>
              )}
            </div>
            <div className="font-mono" style={{ fontSize: 11.5, color: 'var(--ds-fg-4)' }}>
              {timeStr}
            </div>
          </div>
        );
      })}
    </div>
  );
}

// ---------------------------------------------------------------------------
// ProjectSettingsSection (collapsible)
// ---------------------------------------------------------------------------

const SETTINGS_EDITOR_OPTIONS = {
  fontSize: 12,
  minimap: { enabled: false },
  scrollBeyondLastLine: false,
  automaticLayout: true,
  tabSize: 2,
} as const;

function ProjectSettingsSection({ projectPath }: { projectPath: string }) {
  const { t } = useTranslation();
  const [expanded, setExpanded] = useState(false);
  const [text, setText] = useState('{}');
  const [busy, setBusy] = useState(false);
  const [saveError, setSaveError] = useState<string | null>(null);
  const [saved, setSaved] = useState(false);

  const load = useCallback(async () => {
    try {
      const raw = await tauri.readProjectSettings(projectPath, 'local');
      try { setText(JSON.stringify(JSON.parse(raw), null, 2)); }
      catch { setText(raw); }
    } catch { setText('{}'); }
  }, [projectPath]);

  useEffect(() => { void load(); }, [load]);

  async function handleSave() {
    setBusy(true);
    setSaveError(null);
    setSaved(false);
    try {
      await tauri.writeProjectSettings(projectPath, 'local', text);
      setSaved(true);
      window.setTimeout(() => setSaved(false), 3000);
    } catch (e) {
      setSaveError(e instanceof Error ? e.message : String(e));
    } finally {
      setBusy(false);
    }
  }

  return (
    <section style={{ marginTop: 32 }}>
      <button
        type="button"
        onClick={() => setExpanded((v) => !v)}
        style={{
          marginBottom: 6,
          display: 'flex',
          width: '100%',
          alignItems: 'center',
          gap: 6,
          background: 'none',
          border: 0,
          cursor: 'pointer',
          padding: 0,
        }}
      >
        <span style={{ color: 'var(--ds-fg-4)' }}>
          {expanded ? <ChevronDown className="h-3 w-3" /> : <ChevronRight className="h-3 w-3" />}
        </span>
        <span
          className="font-mono uppercase tracking-wider text-[11px]"
          style={{ color: 'var(--ds-fg-4)', fontWeight: 600, letterSpacing: '0.10em' }}
        >
          {t('projectSettings.sectionTitle')}
        </span>
      </button>

      {expanded && (
        <div
          style={{
            borderRadius: 10,
            border: '0.5px solid var(--ds-line)',
            background: 'var(--ds-bg-card)',
            padding: 14,
          }}
        >
          <p style={{ marginBottom: 10, fontSize: 12, color: 'var(--ds-fg-3)' }}>
            {t('projectSettings.description')}
          </p>
          <div style={{ height: 192, overflow: 'hidden', borderRadius: 6, border: '0.5px solid var(--ds-line)' }}>
            <Editor
              height="100%"
              defaultLanguage="json"
              value={text}
              onChange={(v) => { setText(v ?? '{}'); setSaved(false); }}
              options={SETTINGS_EDITOR_OPTIONS}
            />
          </div>
          {saveError && (
            <div style={{ marginTop: 6, fontSize: 12, color: 'var(--ds-danger)' }}>{saveError}</div>
          )}
          <div style={{ marginTop: 10, display: 'flex', alignItems: 'center', gap: 8 }}>
            <button
              type="button"
              onClick={() => void handleSave()}
              disabled={busy}
              style={{
                ...dsPrimaryBtn,
                height: 28,
                padding: '0 12px',
                fontSize: 12,
              }}
            >
              {busy ? t('projectSettings.saving') : t('projectSettings.save')}
            </button>
            {saved && (
              <span style={{ display: 'flex', alignItems: 'center', gap: 4, fontSize: 12, color: 'var(--ds-ok)' }}>
                <CheckCircle2 className="h-3 w-3" />
                {t('projectSettings.saved')}
              </span>
            )}
            <span className="font-mono" style={{ marginLeft: 'auto', fontSize: 10, color: 'var(--ds-fg-4)' }}>
              .claude/settings.local.json
            </span>
          </div>
        </div>
      )}
    </section>
  );
}

// ---------------------------------------------------------------------------
// Shared helpers + primitives
// ---------------------------------------------------------------------------

function Section({
  title,
  hint,
  children,
}: {
  title: string;
  hint?: ReactNode;
  children: ReactNode;
}) {
  return (
    <div style={{ marginTop: 32 }}>
      <div style={{ display: 'flex', alignItems: 'baseline', justifyContent: 'space-between', marginBottom: 12 }}>
        <span
          className="font-mono uppercase"
          style={{ fontSize: 11, fontWeight: 600, letterSpacing: '0.10em', color: 'var(--ds-fg-4)' }}
        >
          {title}
        </span>
        {hint && (
          <span
            style={{ fontSize: 11.5, color: 'var(--ds-fg-4)', display: 'inline-flex', gap: 6, alignItems: 'center' }}
          >
            {hint}
          </span>
        )}
      </div>
      {children}
    </div>
  );
}

function StatusPill({ ok, warn, children }: { ok?: boolean; warn?: boolean; children: ReactNode }) {
  let color = 'var(--ds-fg-2)';
  let bg = 'var(--ds-bg-soft)';
  let borderColor = 'var(--ds-line)';
  if (ok) { color = 'var(--ds-ok)'; bg = 'rgba(21,128,61,0.06)'; borderColor = 'rgba(21,128,61,0.18)'; }
  if (warn) { color = 'var(--ds-warning)'; bg = 'var(--ds-warning-soft)'; borderColor = 'rgba(194,65,12,0.18)'; }

  return (
    <span
      className="font-mono"
      style={{
        display: 'inline-flex', alignItems: 'center', gap: 5,
        fontSize: 11.5,
        padding: '3px 8px 3px 7px',
        borderRadius: 5,
        background: bg,
        border: `0.5px solid ${borderColor}`,
        color,
        whiteSpace: 'nowrap',
      }}
    >
      <span style={{ width: 5, height: 5, borderRadius: '50%', background: 'currentColor', opacity: 0.85, flexShrink: 0 }} />
      {children}
    </span>
  );
}

function KbdChip({ children, className = '', style }: { children: ReactNode; className?: string; style?: CSSProperties }) {
  return (
    <span
      className={`inline-flex items-center justify-center font-mono ${className}`}
      style={{
        height: 18,
        minWidth: 18,
        padding: '0 5px',
        borderRadius: 5,
        background: 'var(--ds-bg-soft)',
        border: '0.5px solid var(--ds-line)',
        color: 'var(--ds-fg-3)',
        boxShadow: 'inset 0 -1px 0 rgba(0,0,0,0.06)',
        fontSize: 10.5,
        whiteSpace: 'nowrap',
        flexShrink: 0,
        ...style,
      }}
    >
      {children}
    </span>
  );
}

// Shared button style objects
const dsBtn: CSSProperties = {
  display: 'inline-flex', alignItems: 'center', gap: 7,
  height: 30,
  padding: '0 11px',
  borderRadius: 7,
  fontFamily: 'inherit',
  fontSize: 12.5,
  fontWeight: 500,
  border: '0.5px solid var(--ds-line-strong)',
  background: 'var(--ds-bg-card)',
  color: 'var(--ds-fg-2)',
  cursor: 'pointer',
};

const dsPrimaryBtn: CSSProperties = {
  display: 'inline-flex', alignItems: 'center', gap: 7,
  height: 34,
  padding: '0 14px',
  borderRadius: 7,
  fontFamily: 'inherit',
  fontSize: 13,
  fontWeight: 500,
  background: 'var(--ds-accent)',
  color: '#fff',
  border: 'none',
  cursor: 'pointer',
};
