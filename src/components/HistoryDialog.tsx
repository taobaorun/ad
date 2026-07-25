/**
 * Modal wrapper around HistoryPanel (M5). The legacy left-rail History
 * button is gone; users open this via ⌘K → "Open activation history".
 */

import { useTranslation } from 'react-i18next';
import { Dialog } from './ui/dialog';
import { HistoryPanel } from './HistoryPanel';

export function HistoryDialog({ open, onClose }: { open: boolean; onClose: () => void }) {
  const { t } = useTranslation();
  return (
    <Dialog open={open} onOpenChange={(o) => !o && onClose()} title={t('history.title')} size="lg">
      <HistoryPanel />
    </Dialog>
  );
}
