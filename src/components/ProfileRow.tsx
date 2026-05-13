import { type ProfileFile } from '@/lib/profileSchema';
import { cn } from '@/lib/utils';
import { Trash2 } from 'lucide-react';

interface Props {
  profile: ProfileFile;
  selected: boolean;
  active: boolean;
  onSelect: () => void;
  onDelete: () => void;
}

export function ProfileRow({ profile, selected, active, onSelect, onDelete }: Props) {
  return (
    <div
      role="button"
      tabIndex={0}
      onClick={onSelect}
      onKeyDown={(e) => {
        if (e.key === 'Enter' || e.key === ' ') onSelect();
      }}
      className={cn(
        'group flex cursor-pointer items-center gap-2 rounded-md px-2 py-1.5 text-sm',
        selected ? 'bg-accent text-accent-foreground' : 'hover:bg-muted',
      )}
    >
      <span
        aria-hidden
        className={cn('inline-block h-2.5 w-2.5 shrink-0 rounded-full')}
        style={{ backgroundColor: profile.color }}
      />
      <span className="flex-1 truncate">{profile.displayName}</span>
      {active && (
        <span className="rounded bg-green-500/15 px-1.5 py-0.5 text-[10px] font-medium uppercase tracking-wide text-green-700 dark:text-green-300">
          active
        </span>
      )}
      <button
        type="button"
        aria-label={`Delete ${profile.displayName}`}
        onClick={(e) => {
          e.stopPropagation();
          if (window.confirm(`Delete profile "${profile.displayName}"?`)) onDelete();
        }}
        className="rounded p-1 text-muted-foreground opacity-0 hover:bg-destructive/10 hover:text-destructive group-hover:opacity-100"
      >
        <Trash2 className="h-3.5 w-3.5" />
      </button>
    </div>
  );
}
