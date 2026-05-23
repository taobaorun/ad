import { useEffect, useRef, useState, type KeyboardEvent } from 'react';
import { tauri } from '@/lib/tauri';
import { useProjects } from '@/store/projects';
import { Input } from './ui/input';
import { Button } from './ui/button';
import { Plus } from 'lucide-react';

/**
 * Path input with terminal-like Tab-completion (D12 third path).
 *
 * - Live dropdown of matching directories as you type.
 * - Tab → complete to longest common prefix; if a single match, complete fully
 *   and append `/` so the user can keep typing.
 * - Enter → add the current value as a project.
 * - Esc → close dropdown.
 */
export function AddProjectInput() {
  const [value, setValue] = useState('');
  const [completions, setCompletions] = useState<string[]>([]);
  const [open, setOpen] = useState(false);
  const [busy, setBusy] = useState(false);
  const [error, setError] = useState<string | null>(null);
  const inputRef = useRef<HTMLInputElement>(null);
  const debounceRef = useRef<number | null>(null);

  const addProject = useProjects((s) => s.addProject);

  useEffect(() => {
    if (!value) {
      setCompletions([]);
      return;
    }
    if (debounceRef.current !== null) window.clearTimeout(debounceRef.current);
    debounceRef.current = window.setTimeout(() => {
      void (async () => {
        try {
          const r = await tauri.completePathPrefix(value);
          setCompletions(r);
        } catch {
          setCompletions([]);
        }
      })();
    }, 80);
    return () => {
      if (debounceRef.current !== null) window.clearTimeout(debounceRef.current);
    };
  }, [value]);

  function longestCommonPrefix(strings: string[]): string {
    if (strings.length === 0) return '';
    let prefix = strings[0] ?? '';
    for (const s of strings.slice(1)) {
      let i = 0;
      while (i < prefix.length && i < s.length && prefix[i] === s[i]) i += 1;
      prefix = prefix.slice(0, i);
      if (prefix === '') return '';
    }
    return prefix;
  }

  async function onSubmit() {
    const v = value.trim();
    if (v === '') return;
    setBusy(true);
    setError(null);
    try {
      await addProject(v);
      setValue('');
      setCompletions([]);
      setOpen(false);
    } catch (e) {
      setError(e instanceof Error ? e.message : String(e));
    } finally {
      setBusy(false);
    }
  }

  function onKeyDown(e: KeyboardEvent<HTMLInputElement>) {
    if (e.key === 'Tab') {
      if (completions.length === 0) return;
      e.preventDefault();
      if (completions.length === 1) {
        setValue(completions[0] ?? '');
        return;
      }
      const lcp = longestCommonPrefix(completions);
      // Only completes if the LCP is strictly longer than the current value.
      if (lcp.length > value.length) {
        setValue(lcp);
      }
      // Otherwise the dropdown shows the candidates; user keeps typing.
      setOpen(true);
    } else if (e.key === 'Enter') {
      e.preventDefault();
      if (open && completions.length === 1) {
        setValue(completions[0] ?? '');
        setOpen(false);
      } else {
        void onSubmit();
      }
    } else if (e.key === 'Escape') {
      setOpen(false);
    }
  }

  return (
    <div className="relative flex flex-1 items-center gap-2">
      <div className="relative flex-1">
        <Input
          ref={inputRef}
          value={value}
          onChange={(e) => {
            setValue(e.target.value);
            setError(null);
            setOpen(true);
          }}
          onKeyDown={onKeyDown}
          onFocus={() => setOpen(true)}
          placeholder="Type a path, then Tab to complete · ~/ allowed · Enter to add"
          className="font-mono text-xs"
          aria-label="Add project by path"
        />
        {open && completions.length > 0 && (
          <div
            role="listbox"
            className="absolute left-0 right-0 top-full z-50 mt-1 max-h-60 overflow-auto rounded-md border border-border bg-background shadow-lg ring-1 ring-black/5"
          >
            {completions.map((c) => (
              <button
                key={c}
                type="button"
                onClick={() => {
                  setValue(c);
                  inputRef.current?.focus();
                }}
                className="block w-full truncate px-3 py-1.5 text-left font-mono text-xs hover:bg-muted"
              >
                {c}
              </button>
            ))}
          </div>
        )}
      </div>
      <Button onClick={() => void onSubmit()} disabled={busy || value.trim() === ''} size="sm">
        <Plus className="h-4 w-4" />
        Add
      </Button>
      {error && <span className="text-xs text-destructive">{error}</span>}
    </div>
  );
}
