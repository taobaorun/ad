import { catppuccinLatte, catppuccinMocha } from '@catppuccin/codemirror';
import type { Extension } from '@codemirror/state';

export function editorThemeFor(dark: boolean): Extension {
  return dark ? catppuccinMocha : catppuccinLatte;
}
