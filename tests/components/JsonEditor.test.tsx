import { catppuccinLatte, catppuccinMocha } from '@catppuccin/codemirror';
import { render } from '@testing-library/react';
import { describe, expect, it, vi } from 'vitest';

import { JsonEditor } from '@/components/JsonEditor';
import { editorThemeFor } from '@/lib/editorTheme';

describe('JsonEditor theme lifecycle', () => {
  it('selects the official Catppuccin extension for each mode', () => {
    expect(editorThemeFor(true)).toBe(catppuccinMocha);
    expect(editorThemeFor(false)).toBe(catppuccinLatte);
  });

  it('reconfigures the existing editor when the mode changes', () => {
    const onChange = vi.fn();
    const { container, rerender } = render(
      <JsonEditor value={'{"theme":"latte"}'} onChange={onChange} dark={false} />,
    );
    const editor = container.querySelector('.cm-editor');

    rerender(<JsonEditor value={'{"theme":"mocha"}'} onChange={onChange} dark />);

    expect(container.querySelector('.cm-editor')).toBe(editor);
    expect(container.querySelector('.cm-content')).toHaveTextContent('{"theme":"mocha"}');
    expect(onChange).toHaveBeenCalledWith('{"theme":"mocha"}');
  });
});
