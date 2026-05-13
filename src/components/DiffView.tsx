import { useEffect, useState } from 'react';
import { DiffEditor } from '@monaco-editor/react';
import { tauri } from '@/lib/tauri';

export function DiffView({ candidateText }: { candidateText: string }) {
  const [originalText, setOriginalText] = useState<string>('');
  const [error, setError] = useState<string | null>(null);

  useEffect(() => {
    let cancelled = false;
    void (async () => {
      try {
        const current = await tauri.readCurrentSettings();
        if (cancelled) return;
        setOriginalText(current ? JSON.stringify(current, null, 2) : '{}');
      } catch (e) {
        if (cancelled) return;
        setError(e instanceof Error ? e.message : String(e));
      }
    })();
    return () => {
      cancelled = true;
    };
  }, [candidateText]);

  if (error) {
    return (
      <div className="p-3 text-sm text-destructive">Could not read current settings: {error}</div>
    );
  }

  return (
    <DiffEditor
      height="100%"
      original={originalText}
      modified={candidateText}
      language="json"
      options={{
        readOnly: true,
        renderSideBySide: true,
        fontSize: 13,
        minimap: { enabled: false },
        automaticLayout: true,
      }}
    />
  );
}
