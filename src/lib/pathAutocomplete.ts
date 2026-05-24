import { useEffect, useRef, useState } from 'react';

import { tauri } from './tauri';

const DEBOUNCE_MS = 120;

export interface PathAutocompleteState {
  candidates: string[];
  loading: boolean;
  /**
   * Longest path prefix shared by all candidates. When strictly longer than
   * the user's input, callers can replace the input with this on Tab.
   * Empty when no candidates.
   */
  completion: string;
}

export function usePathAutocomplete(input: string): PathAutocompleteState {
  const [candidates, setCandidates] = useState<string[]>([]);
  const [loading, setLoading] = useState(false);
  const seq = useRef(0);

  useEffect(() => {
    const trimmed = input.trim();
    if (trimmed === '') {
      setCandidates([]);
      setLoading(false);
      return;
    }
    setLoading(true);
    const mySeq = ++seq.current;
    const handle = window.setTimeout(() => {
      tauri
        .completePathPrefix(trimmed)
        .then((r) => {
          if (mySeq !== seq.current) return;
          setCandidates(r);
          setLoading(false);
        })
        .catch(() => {
          if (mySeq !== seq.current) return;
          setCandidates([]);
          setLoading(false);
        });
    }, DEBOUNCE_MS);
    return () => window.clearTimeout(handle);
  }, [input]);

  return { candidates, loading, completion: longestCommonPrefix(candidates) };
}

function longestCommonPrefix(strs: string[]): string {
  const first = strs[0];
  if (first === undefined) return '';
  let prefix = first;
  for (let i = 1; i < strs.length; i++) {
    const s = strs[i] ?? '';
    while (!s.startsWith(prefix)) {
      prefix = prefix.slice(0, -1);
      if (prefix === '') return '';
    }
  }
  return prefix;
}
