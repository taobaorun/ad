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

type RetainedDialogView = Pick<
  DialogProps,
  'children' | 'title' | 'description' | 'size' | 'closeDisabled' | 'footer'
>;

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
  const [present, setPresent] = React.useState(open);
  const [visualOpen, setVisualOpen] = React.useState(false);
  const [retainedView, setRetainedView] = React.useState<RetainedDialogView>(() => ({
    children,
    title,
    description,
    size,
    closeDisabled,
    footer,
  }));
  const overlayRef = React.useRef<HTMLDivElement>(null);
  const dialogRef = React.useRef<HTMLDivElement>(null);
  const previousFocusRef = React.useRef<HTMLElement | null>(null);
  const onOpenChangeRef = React.useRef(onOpenChange);
  const closeDisabledRef = React.useRef(closeDisabled);
  const openRef = React.useRef(open);
  const wasOpenRef = React.useRef(false);
  const frameRef = React.useRef<number | null>(null);
  const closeTimerRef = React.useRef<number | null>(null);
  const titleId = React.useId();
  const descriptionId = React.useId();
  onOpenChangeRef.current = onOpenChange;
  closeDisabledRef.current = closeDisabled;
  openRef.current = open;

  React.useLayoutEffect(() => {
    if (!open) return;
    setRetainedView({ children, title, description, size, closeDisabled, footer });
  }, [children, closeDisabled, description, footer, open, size, title]);

  const displayedView: RetainedDialogView = open
    ? { children, title, description, size, closeDisabled, footer }
    : retainedView;

  const finishClose = React.useCallback(() => {
    if (openRef.current) return;
    if (closeTimerRef.current !== null) window.clearTimeout(closeTimerRef.current);
    setPresent(false);
    const activeElement = document.activeElement as HTMLElement | null;
    const shouldRestoreFocus =
      !activeElement ||
      activeElement === document.body ||
      dialogRef.current?.contains(activeElement) === true;
    if (shouldRestoreFocus) previousFocusRef.current?.focus();
    previousFocusRef.current = null;
  }, []);

  React.useEffect(() => {
    if (frameRef.current !== null) cancelAnimationFrame(frameRef.current);
    if (closeTimerRef.current !== null) window.clearTimeout(closeTimerRef.current);
    if (open) {
      if (!wasOpenRef.current && previousFocusRef.current === null) {
        previousFocusRef.current = document.activeElement as HTMLElement | null;
      }
      wasOpenRef.current = true;
      setPresent(true);
      frameRef.current = requestAnimationFrame(() => setVisualOpen(true));
      return;
    }
    if (!wasOpenRef.current) return;
    wasOpenRef.current = false;
    setVisualOpen(false);
    closeTimerRef.current = window.setTimeout(finishClose, 240);
  }, [finishClose, open]);

  React.useEffect(() => {
    if (!open || !present) return;
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
    };
  }, [open, present]);

  React.useLayoutEffect(() => {
    const overlay = overlayRef.current;
    if (!overlay) return;
    if (open) {
      overlay.removeAttribute('inert');
      overlay.removeAttribute('aria-hidden');
      return;
    }

    const activeElement = document.activeElement as HTMLElement | null;
    if (activeElement && dialogRef.current?.contains(activeElement)) {
      activeElement.blur();
    }
    overlay.setAttribute('inert', '');
    overlay.setAttribute('aria-hidden', 'true');
  }, [open, present]);

  React.useEffect(
    () => () => {
      if (frameRef.current !== null) cancelAnimationFrame(frameRef.current);
      if (closeTimerRef.current !== null) window.clearTimeout(closeTimerRef.current);
      const activeElement = document.activeElement as HTMLElement | null;
      const shouldRestoreFocus =
        !activeElement ||
        activeElement === document.body ||
        dialogRef.current?.contains(activeElement) === true;
      if (shouldRestoreFocus) previousFocusRef.current?.focus();
    },
    [],
  );

  if (!present) return null;
  return (
    <div
      ref={overlayRef}
      className="ad-dialog-overlay fixed inset-0 z-50 flex items-center justify-center bg-overlay/65 p-4"
      data-open={visualOpen ? 'true' : 'false'}
    >
      <div
        ref={dialogRef}
        role="dialog"
        aria-modal="true"
        aria-labelledby={displayedView.title ? titleId : undefined}
        aria-describedby={displayedView.description ? descriptionId : undefined}
        tabIndex={-1}
        data-open={visualOpen ? 'true' : 'false'}
        onTransitionEnd={(event) => {
          if (event.currentTarget === event.target && !visualOpen) finishClose();
        }}
        className={cn(
          'ad-dialog-panel relative flex max-h-[calc(100vh-2rem)] w-full flex-col overflow-hidden rounded-lg border border-border bg-card text-card-foreground shadow-lg',
          sizes[displayedView.size ?? 'md'],
        )}
      >
        <button
          type="button"
          onClick={() => onOpenChange(false)}
          disabled={displayedView.closeDisabled}
          className="absolute right-3 top-3 z-10 rounded p-1 text-muted-foreground hover:bg-accent hover:text-foreground focus-visible:outline-none focus-visible:ring-2 focus-visible:ring-ring disabled:cursor-not-allowed disabled:opacity-50"
          aria-label={t('common.close')}
        >
          <X className="h-4 w-4" />
        </button>

        {(displayedView.title || displayedView.description) && (
          <div className="shrink-0 px-6 pb-4 pr-12 pt-6">
            {displayedView.title && (
              <h2 id={titleId} className="text-lg font-semibold leading-tight">
                {displayedView.title}
              </h2>
            )}
            {displayedView.description && (
              <p id={descriptionId} className="mt-1 text-sm text-muted-foreground">
                {displayedView.description}
              </p>
            )}
          </div>
        )}

        <div
          className={cn(
            'min-h-0 flex-1 overflow-y-auto px-6',
            displayedView.title || displayedView.description ? '' : 'pt-6',
            displayedView.footer ? 'pb-4' : 'pb-6',
          )}
        >
          {displayedView.children}
        </div>

        {displayedView.footer && (
          <div className="shrink-0 border-t border-border bg-card px-6 py-3">
            {displayedView.footer}
          </div>
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
