import { useState } from 'react';
import { useTranslation } from 'react-i18next';
import { Dialog } from './ui/dialog';
import { Tabs, TabsList, TabsTrigger, TabsContent } from './ui/tabs';
import { Button } from './ui/button';
import { Input } from './ui/input';
import { Label } from './ui/label';
import { open as openFileDialog } from '@tauri-apps/plugin-dialog';
import { tauri } from '@/lib/tauri';
import { useProfiles } from '@/store/profiles';

export function ImportDialog() {
  const { t } = useTranslation();
  const open = useProfiles((s) => s.importOpen);
  const setImportOpen = useProfiles((s) => s.setImportOpen);
  const loadAll = useProfiles((s) => s.loadAll);
  const [tab, setTab] = useState<'file' | 'url'>('file');
  const [url, setUrl] = useState('');
  const [error, setError] = useState<string | null>(null);
  const [busy, setBusy] = useState(false);

  async function importFile() {
    setError(null);
    setBusy(true);
    try {
      const picked = await openFileDialog({
        multiple: false,
        filters: [{ name: 'Profile JSON', extensions: ['json'] }],
      });
      if (typeof picked !== 'string') return;
      await tauri.importFromFile(picked);
      await loadAll();
      setImportOpen(false);
    } catch (e) {
      setError(e instanceof Error ? e.message : String(e));
    } finally {
      setBusy(false);
    }
  }

  async function importUrl() {
    setError(null);
    setBusy(true);
    try {
      await tauri.importFromUrl(url);
      await loadAll();
      setImportOpen(false);
    } catch (e) {
      setError(e instanceof Error ? e.message : String(e));
    } finally {
      setBusy(false);
    }
  }

  return (
    <Dialog open={open} onOpenChange={setImportOpen} title={t('import.title')} size="lg">
      <Tabs value={tab} onValueChange={(v) => setTab(v as 'file' | 'url')}>
        <TabsList>
          <TabsTrigger value="file">{t('import.tabFile')}</TabsTrigger>
          <TabsTrigger value="url">{t('import.tabUrl')}</TabsTrigger>
        </TabsList>
        <TabsContent value="file">
          <div className="flex flex-col gap-2 py-2">
            <p className="text-sm text-muted-foreground">{t('import.filePrompt')}</p>
            <Button onClick={() => void importFile()} disabled={busy}>
              {t('import.chooseFile')}
            </Button>
          </div>
        </TabsContent>
        <TabsContent value="url">
          <div className="flex flex-col gap-2 py-2">
            <Label>{t('import.urlLabel')}</Label>
            <Input
              placeholder="https://example.com/profile.json or https://gist.github.com/..."
              value={url}
              onChange={(e) => setUrl(e.target.value)}
            />
            <Button onClick={() => void importUrl()} disabled={busy || url.trim() === ''}>
              {t('import.fetchAndImport')}
            </Button>
          </div>
        </TabsContent>
      </Tabs>
      {error && <div className="mt-3 text-sm text-destructive">{error}</div>}
    </Dialog>
  );
}
