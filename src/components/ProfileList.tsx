import { useProfiles } from '@/store/profiles';
import { Button } from './ui/button';
import { Plus, Download, Clock } from 'lucide-react';
import { ProfileRow } from './ProfileRow';
import { EmptyState } from './EmptyState';

export function ProfileList() {
  const profiles = useProfiles((s) => s.profiles);
  const activeId = useProfiles((s) => s.activeId);
  const selectedId = useProfiles((s) => s.selectedId);
  const select = useProfiles((s) => s.select);
  const createNew = useProfiles((s) => s.createNew);
  const remove = useProfiles((s) => s.remove);
  const setImportOpen = useProfiles((s) => s.setImportOpen);
  const setView = useProfiles((s) => s.setView);
  const view = useProfiles((s) => s.view);

  return (
    <div className="flex h-full flex-col">
      <div className="flex items-center gap-1 p-2">
        <Button onClick={() => void createNew()} size="sm" className="flex-1">
          <Plus className="h-4 w-4" />
          New
        </Button>
        <Button
          onClick={() => setImportOpen(true)}
          size="icon"
          variant="outline"
          aria-label="Import"
        >
          <Download className="h-4 w-4" />
        </Button>
      </div>
      {profiles.length === 0 ? (
        <EmptyState />
      ) : (
        <div className="flex-1 overflow-y-auto px-2 pb-2">
          {profiles.map((p) => (
            <ProfileRow
              key={p.id}
              profile={p}
              selected={p.id === selectedId && view === 'editor'}
              active={p.id === activeId}
              onSelect={() => select(p.id)}
              onDelete={() => void remove(p.id)}
            />
          ))}
        </div>
      )}
      <div className="border-t border-border p-2">
        <Button
          onClick={() => setView(view === 'history' ? 'editor' : 'history')}
          size="sm"
          variant={view === 'history' ? 'secondary' : 'ghost'}
          className="w-full justify-start"
        >
          <Clock className="h-4 w-4" />
          History
        </Button>
      </div>
    </div>
  );
}
