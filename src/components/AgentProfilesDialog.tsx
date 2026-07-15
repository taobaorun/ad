import { useCallback, useEffect, useMemo, useState } from 'react';
import { Play, Plus, RotateCcw, Save, Trash2, UserRoundCog } from 'lucide-react';
import { useTranslation } from 'react-i18next';

import { formatAgentError } from '@/lib/agentErrors';
import type { MutationPlanView, OperationReceipt } from '@/lib/agentTypes';
import { createBlankAgentProfile, profileEditorFor } from '@/lib/profileEditorRegistry';
import type { AgentProfile } from '@/lib/profileSchema';
import { tauri } from '@/lib/tauri';
import { useAgents } from '@/store/agents';
import { useUiSettings } from '@/store/uiSettings';

import { JsonEditor } from './JsonEditor';
import { AgentPlanDialog } from './AgentPlanDialog';
import { Button } from './ui/button';
import { Dialog } from './ui/dialog';
import { Input } from './ui/input';

export function AgentProfilesButton() {
  const { t } = useTranslation();
  const [open, setOpen] = useState(false);

  return (
    <>
      <button
        type="button"
        onClick={() => setOpen(true)}
        className="inline-flex h-7 items-center gap-1.5 rounded-md border border-border px-2 text-xs text-muted-foreground hover:bg-muted hover:text-foreground"
        aria-label={t('agentProfiles.open')}
        title={t('agentProfiles.open')}
      >
        <UserRoundCog className="h-3.5 w-3.5" aria-hidden="true" />
        {t('agentProfiles.shortLabel')}
      </button>
      <AgentProfilesDialog open={open} onOpenChange={setOpen} />
    </>
  );
}

function AgentProfilesDialog({
  open,
  onOpenChange,
}: {
  open: boolean;
  onOpenChange: (open: boolean) => void;
}) {
  const { t } = useTranslation();
  const activeAgentId = useAgents((state) => state.activeAgentId);
  const activeContext = useAgents((state) => state.activeContext);
  const darkMode = useUiSettings((state) => state.darkMode);
  const [profiles, setProfiles] = useState<AgentProfile[]>([]);
  const [selectedId, setSelectedId] = useState<string | null>(null);
  const [draft, setDraft] = useState<AgentProfile | null>(null);
  const [payloadText, setPayloadText] = useState('');
  const [loading, setLoading] = useState(false);
  const [busy, setBusy] = useState(false);
  const [dirty, setDirty] = useState(false);
  const [error, setError] = useState<string | null>(null);
  const [plan, setPlan] = useState<MutationPlanView | null>(null);
  const [planBusy, setPlanBusy] = useState(false);
  const [planError, setPlanError] = useState<string | null>(null);
  const [lastReceipt, setLastReceipt] = useState<OperationReceipt | null>(null);

  const load = useCallback(async () => {
    setLoading(true);
    setError(null);
    try {
      const next = await tauri.listProfileEnvelopes(activeAgentId);
      setProfiles(next);
      setSelectedId((current) =>
        current && next.some((profile) => profile.key.profileId === current)
          ? current
          : (next[0]?.key.profileId ?? null),
      );
    } catch (caught) {
      setError(formatAgentError(caught));
    } finally {
      setLoading(false);
    }
  }, [activeAgentId]);

  useEffect(() => {
    if (open) void load();
  }, [load, open]);

  const selected = useMemo(
    () => profiles.find((profile) => profile.key.profileId === selectedId) ?? null,
    [profiles, selectedId],
  );

  useEffect(() => {
    setDraft(selected);
    setPayloadText(selected ? profileEditorFor(selected.payloadSchema).serialize(selected) : '');
    setDirty(false);
    setError(null);
    setPlan(null);
    setPlanError(null);
    setLastReceipt(null);
  }, [selected]);

  async function createProfile() {
    const id = nextUntitledId(profiles);
    setBusy(true);
    setError(null);
    try {
      const created = await tauri.saveProfileEnvelope(createBlankAgentProfile(activeAgentId, id));
      setProfiles((current) => [...current, created]);
      setSelectedId(created.key.profileId);
    } catch (caught) {
      setError(formatAgentError(caught));
    } finally {
      setBusy(false);
    }
  }

  async function saveProfile() {
    if (!draft) return;
    setBusy(true);
    setError(null);
    try {
      const editor = profileEditorFor(draft.payloadSchema);
      const withPayload = editor.update(draft, payloadText);
      const saved = await tauri.saveProfileEnvelope({
        ...withPayload,
        metadata: { ...withPayload.metadata, updatedAt: new Date().toISOString() },
      });
      setProfiles((current) =>
        current.map((profile) => (profile.key.profileId === saved.key.profileId ? saved : profile)),
      );
      setDraft(saved);
      setPayloadText(editor.serialize(saved));
      setDirty(false);
    } catch (caught) {
      setError(
        caught instanceof SyntaxError
          ? t('agentProfiles.invalidPayload')
          : formatAgentError(caught),
      );
    } finally {
      setBusy(false);
    }
  }

  async function deleteProfile() {
    if (
      !draft ||
      !window.confirm(t('agentProfiles.deleteConfirm', { name: draft.metadata.displayName }))
    )
      return;
    setBusy(true);
    setError(null);
    try {
      await tauri.deleteProfileEnvelope(activeAgentId, draft.key.profileId);
      const remaining = profiles.filter((profile) => profile.key.profileId !== draft.key.profileId);
      setProfiles(remaining);
      setSelectedId(remaining[0]?.key.profileId ?? null);
    } catch (caught) {
      setError(formatAgentError(caught));
    } finally {
      setBusy(false);
    }
  }

  async function previewApply() {
    if (!draft || !activeContext || dirty) return;
    setBusy(true);
    setError(null);
    try {
      setPlan(await tauri.previewAgentProfileApply(activeContext, draft.key.profileId));
      setPlanError(null);
    } catch (caught) {
      setError(formatAgentError(caught));
    } finally {
      setBusy(false);
    }
  }

  async function applyPlan() {
    if (!plan) return;
    setPlanBusy(true);
    setPlanError(null);
    try {
      const receipt = await tauri.applyAgentPlan(plan.id);
      setLastReceipt(receipt);
      setPlan(null);
    } catch (caught) {
      setPlanError(formatAgentError(caught));
    } finally {
      setPlanBusy(false);
    }
  }

  async function rollbackApply() {
    if (!lastReceipt || !window.confirm(t('agentProfiles.rollbackConfirm'))) return;
    setBusy(true);
    setError(null);
    try {
      await tauri.rollbackAgentReceipt(lastReceipt.id, true);
      setLastReceipt(null);
    } catch (caught) {
      setError(formatAgentError(caught));
    } finally {
      setBusy(false);
    }
  }

  return (
    <>
      <Dialog
        open={open}
        onOpenChange={(nextOpen) => {
          if (dirty && !nextOpen && !window.confirm(t('agentProfiles.discardConfirm'))) return;
          onOpenChange(nextOpen);
        }}
        title={t('agentProfiles.title')}
        description={t('agentProfiles.description', { agent: activeAgentId })}
        size="lg"
        footer={
          <div className="flex items-center justify-between gap-2">
            <Button
              type="button"
              variant="outline"
              onClick={() => void createProfile()}
              disabled={busy}
            >
              <Plus className="h-3.5 w-3.5" />
              {t('agentProfiles.create')}
            </Button>
            <div className="flex gap-2">
              {lastReceipt && (
                <Button
                  type="button"
                  variant="outline"
                  onClick={() => void rollbackApply()}
                  disabled={busy}
                >
                  <RotateCcw className="h-3.5 w-3.5" />
                  {t('agentProfiles.rollback')}
                </Button>
              )}
              {draft && (
                <Button
                  type="button"
                  variant="outline"
                  onClick={() => void deleteProfile()}
                  disabled={busy}
                >
                  <Trash2 className="h-3.5 w-3.5" />
                  {t('agentProfiles.delete')}
                </Button>
              )}
              <Button
                type="button"
                variant="outline"
                onClick={() => void previewApply()}
                disabled={busy || dirty || !draft || !activeContext}
                title={dirty ? t('agentProfiles.applyRequiresSave') : undefined}
              >
                <Play className="h-3.5 w-3.5" />
                {t('agentProfiles.previewApply')}
              </Button>
              <Button
                type="button"
                onClick={() => void saveProfile()}
                disabled={busy || !dirty || !draft}
              >
                <Save className="h-3.5 w-3.5" />
                {t('agentProfiles.save')}
              </Button>
            </div>
          </div>
        }
      >
        {error && (
          <div
            role="alert"
            className="mb-3 rounded-md border border-destructive/40 bg-destructive/10 p-3 text-sm text-destructive"
          >
            {error}
          </div>
        )}
        <div className="grid min-h-[420px] grid-cols-[180px_minmax(0,1fr)] overflow-hidden rounded-lg border border-border">
          <aside
            className="border-r border-border bg-muted/30 p-2"
            aria-label={t('agentProfiles.list')}
          >
            {loading ? (
              <div className="p-2 text-xs text-muted-foreground" aria-busy="true">
                {t('agentProfiles.loading')}
              </div>
            ) : profiles.length === 0 ? (
              <div role="status" className="p-2 text-xs text-muted-foreground">
                {t('agentProfiles.empty')}
              </div>
            ) : (
              <ul className="space-y-1">
                {profiles.map((profile) => (
                  <li key={profile.key.profileId}>
                    <button
                      type="button"
                      onClick={() => {
                        if (dirty && !window.confirm(t('agentProfiles.discardConfirm'))) return;
                        setSelectedId(profile.key.profileId);
                      }}
                      className="w-full rounded-md px-2 py-1.5 text-left text-sm"
                      style={{
                        background:
                          profile.key.profileId === selectedId
                            ? 'var(--ds-bg-hover)'
                            : 'transparent',
                      }}
                      aria-label={profile.metadata.displayName}
                    >
                      <span
                        className="mr-2 inline-block h-2.5 w-2.5 rounded-full"
                        style={{ background: profile.metadata.color }}
                        aria-hidden="true"
                      />
                      {profile.metadata.displayName}
                    </button>
                  </li>
                ))}
              </ul>
            )}
          </aside>

          {draft ? (
            <div className="flex min-w-0 flex-col">
              <div className="grid grid-cols-[44px_minmax(0,1fr)] gap-2 border-b border-border p-3">
                <input
                  type="color"
                  value={draft.metadata.color}
                  onChange={(event) => {
                    setDraft({
                      ...draft,
                      metadata: { ...draft.metadata, color: event.target.value },
                    });
                    setDirty(true);
                  }}
                  aria-label={t('agentProfiles.color')}
                  className="h-9 w-11 rounded border border-input bg-transparent"
                />
                <Input
                  value={draft.metadata.displayName}
                  onChange={(event) => {
                    setDraft({
                      ...draft,
                      metadata: { ...draft.metadata, displayName: event.target.value },
                    });
                    setDirty(true);
                  }}
                  aria-label={t('agentProfiles.name')}
                />
                <div />
                <Input
                  value={draft.metadata.description ?? ''}
                  onChange={(event) => {
                    setDraft({
                      ...draft,
                      metadata: { ...draft.metadata, description: event.target.value },
                    });
                    setDirty(true);
                  }}
                  aria-label={t('agentProfiles.profileDescription')}
                  placeholder={t('agentProfiles.descriptionPlaceholder')}
                />
              </div>
              <div className="min-h-0 flex-1">
                <JsonEditor
                  key={draft.payloadSchema}
                  value={payloadText}
                  onChange={(next) => {
                    setPayloadText(next);
                    setDirty(true);
                  }}
                  dark={darkMode}
                  language={profileEditorFor(draft.payloadSchema).language}
                />
              </div>
            </div>
          ) : (
            <div
              role="status"
              className="flex items-center justify-center text-sm text-muted-foreground"
            >
              {t('agentProfiles.select')}
            </div>
          )}
        </div>
      </Dialog>
      <AgentPlanDialog
        plan={plan}
        busy={planBusy}
        error={planError}
        onCancel={() => {
          setPlan(null);
          setPlanError(null);
        }}
        onConfirm={() => void applyPlan()}
      />
    </>
  );
}

function nextUntitledId(profiles: AgentProfile[]): string {
  const taken = new Set(profiles.map((profile) => profile.key.profileId));
  let index = 1;
  while (taken.has(`untitled-${index}`)) index += 1;
  return `untitled-${index}`;
}
