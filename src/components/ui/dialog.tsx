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
  closeDisabled?: boolean;
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
  closeDisabled = false,
  footer,
}: DialogProps) {
  const { t } = useTranslation();
  const dialogRef = React.useRef<HTMLDivElement>(null);
  const previousFocusRef = React.useRef<HTMLElement | null>(null);
  const onOpenChangeRef = React.useRef(onOpenChange);
  const closeDisabledRef = React.useRef(closeDisabled);
  const titleId = React.useId();
  const descriptionId = React.useId();
  onOpenChangeRef.current = onOpenChange;
  closeDisabledRef.current = closeDisabled;

  React.useEffect(() => {
    if (!open) return;
    previousFocusRef.current = document.activeElement as HTMLElement | null;
    const dialog = dialogRef.current;
    const initialFocus =
      dialog?.querySelector<HTMLElement>('[data-dialog-initial-focus]') ??
      focusableElements(dialog)[0] ??
      dialog;
    initialFocus?.focus();

    function onKey(event: KeyboardEvent) {
      if (event.key === 'Escape' && !closeDisabledRef.current) {
        event.preventDefault();
        onOpenChangeRef.current(false);
        return;
      }
      if (event.key !== 'Tab' || !dialog) return;
      const focusable = focusableElements(dialog);
      if (focusable.length === 0) {
        event.preventDefault();
        dialog.focus();
        return;
      }
      const first = focusable[0]!;
      const last = focusable[focusable.length - 1]!;
      if (event.shiftKey && document.activeElement === first) {
        event.preventDefault();
        last.focus();
      } else if (!event.shiftKey && document.activeElement === last) {
        event.preventDefault();
        first.focus();
      }
    }
    document.addEventListener('keydown', onKey);
    return () => {
      document.removeEventListener('keydown', onKey);
      previousFocusRef.current?.focus();
      previousFocusRef.current = null;
    };
  }, [open]);

  if (!open) return null;
  return (
    <div className="fixed inset-0 z-50 flex items-center justify-center bg-overlay/65 p-4">
      <div
        ref={dialogRef}
        role="dialog"
        aria-modal="true"
        aria-labelledby={title ? titleId : undefined}
        aria-describedby={description ? descriptionId : undefined}
        tabIndex={-1}
        className={cn(
          'relative flex max-h-[calc(100vh-2rem)] w-full flex-col overflow-hidden rounded-lg border border-border bg-card text-card-foreground shadow-lg',
          sizes[size],
        )}
      >
        <button
          type="button"
          onClick={() => onOpenChange(false)}
          disabled={closeDisabled}
          className="absolute right-3 top-3 z-10 rounded p-1 text-muted-foreground hover:bg-accent hover:text-foreground focus-visible:outline-none focus-visible:ring-2 focus-visible:ring-ring disabled:cursor-not-allowed disabled:opacity-50"
          aria-label={t('common.close')}
        >
          <X className="h-4 w-4" />
        </button>

        {(title || description) && (
          <div className="shrink-0 px-6 pb-4 pr-12 pt-6">
            {title && (
              <h2 id={titleId} className="text-lg font-semibold leading-tight">
                {title}
              </h2>
            )}
            {description && (
              <p id={descriptionId} className="mt-1 text-sm text-muted-foreground">
                {description}
              </p>
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
          <div className="shrink-0 border-t border-border bg-card px-6 py-3">{footer}</div>
        )}
      </div>
    </div>
  );
}

function focusableElements(root: HTMLElement | null): HTMLElement[] {
  if (!root) return [];
  return Array.from(
    root.querySelectorAll<HTMLElement>(
      'button:not([disabled]), [href], input:not([disabled]), select:not([disabled]), textarea:not([disabled]), [tabindex]:not([tabindex="-1"])',
    ),
  ).filter((element) => !element.hasAttribute('hidden'));
}
