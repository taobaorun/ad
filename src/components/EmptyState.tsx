import { useProfiles } from '@/store/profiles';
import { Button } from './ui/button';
import { Plus } from 'lucide-react';

export function EmptyState() {
  const createNew = useProfiles((s) => s.createNew);
  return (
    <div className="flex h-full flex-col items-center justify-center gap-4 p-6 text-center text-muted-foreground">
      <p className="max-w-xs text-sm">No profiles yet. Create one or import an existing JSON.</p>
      <Button onClick={() => void createNew()} size="sm">
        <Plus className="h-4 w-4" />
        New profile
      </Button>
    </div>
  );
}
