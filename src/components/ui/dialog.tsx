import * as React from 'react';
import { useTranslation } from 'react-i18next';
import { cn } from '@/lib/utils';
import { X } from 'lucide-react';

interface DialogProps {
  open: boolean;
  onOpenChange: (open: boolean) => void;
  children: React.ReactNode;
  title?: string;
  description?: string;
  size?: 'sm' | 'md' | 'lg';
  /**
   * Optional sticky footer (typically action buttons). When provided, it
   * renders pinned to the bottom of the dialog with a top divider, while the
   * children area becomes the scrollable body. Without it, the body still
   * scrolls when content exceeds viewport height.
   */
  footer?: React.ReactNode;
}

const sizes = {
  sm: 'max-w-sm',
  md: 'max-w-md',
  lg: 'max-w-2xl',
};

export function Dialog({
  open,
  onOpenChange,
  children,
  title,
  description,
  size = 'md',
  footer,
}: DialogProps) {
  const { t } = useTranslation();

  React.useEffect(() => {
    function onKey(e: KeyboardEvent) {
      if (e.key === 'Escape' && open) onOpenChange(false);
    }
    document.addEventListener('keydown', onKey);
    return () => document.removeEventListener('keydown', onKey);
  }, [open, onOpenChange]);

  if (!open) return null;
  return (
    <div className="fixed inset-0 z-50 flex items-center justify-center bg-overlay/65 p-4">
      <div
        role="dialog"
        aria-modal="true"
        className={cn(
          'relative flex max-h-[calc(100vh-2rem)] w-full flex-col overflow-hidden rounded-lg border border-border bg-card text-card-foreground shadow-lg',
          sizes[size],
        )}
      >
        <button
          type="button"
          onClick={() => onOpenChange(false)}
          className="absolute right-3 top-3 z-10 rounded p-1 text-muted-foreground hover:bg-accent hover:text-foreground focus-visible:outline-none focus-visible:ring-2 focus-visible:ring-ring"
          aria-label={t('common.close')}
        >
          <X className="h-4 w-4" />
        </button>

        {(title || description) && (
          <div className="shrink-0 px-6 pb-4 pt-6 pr-12">
            {title && <h2 className="text-lg font-semibold leading-tight">{title}</h2>}
            {description && (
              <p className="mt-1 text-sm text-muted-foreground">{description}</p>
            )}
          </div>
        )}

        <div
          className={cn(
            'min-h-0 flex-1 overflow-y-auto px-6',
            title || description ? '' : 'pt-6',
            footer ? 'pb-4' : 'pb-6',
          )}
        >
          {children}
        </div>

        {footer && (
          <div className="shrink-0 border-t border-border bg-card px-6 py-3">
            {footer}
          </div>
        )}
      </div>
    </div>
  );
}
