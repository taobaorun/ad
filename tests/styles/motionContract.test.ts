import { readFileSync } from 'node:fs';
import { describe, expect, it } from 'vitest';

const read = (path: string) => readFileSync(path, 'utf8');

describe('motion contract', () => {
  it('defines one shared easing and duration vocabulary', () => {
    const css = read('src/styles/globals.css');
    expect(css).toContain('--motion-ease-out: cubic-bezier(0.23, 1, 0.32, 1)');
    expect(css).toContain('--motion-ease-in-out: cubic-bezier(0.77, 0, 0.175, 1)');
    expect(css).toContain('--motion-ease-drawer: cubic-bezier(0.32, 0.72, 0, 1)');
    expect(css).toContain('--motion-duration-press: 160ms');
    expect(css).toContain('@media (prefers-reduced-motion: reduce)');
  });

  it('keeps frequent controls precise and progress compositor-only', () => {
    expect(read('src/components/ui/button.tsx')).toContain('ad-motion-press');
    expect(read('src/components/ui/tabs.tsx')).not.toContain('transition-all');
    expect(read('src/components/SkillToggle.tsx')).toContain('ad-skill-toggle-thumb');
    for (const file of [
      'src/components/ResourceRemovalDialog.tsx',
      'src/components/SourceRemovalDialog.tsx',
    ]) {
      const source = read(file);
      expect(source).toContain('ad-motion-progress');
      expect(source).toContain('scaleX(');
      expect(source).not.toContain('transition-[width]');
    }
  });

  it('gives every persistent busy animation a reduced-motion fallback', () => {
    for (const file of [
      'src/components/AgentConversionProgress.tsx',
      'src/components/LegacySkillMigrationCard.tsx',
      'src/components/ProjectDetailSupport.tsx',
      'src/components/ResourceCenter.tsx',
      'src/components/SkillSources.tsx',
    ]) {
      expect(read(file)).toContain('motion-reduce:animate-none');
    }
  });
});
