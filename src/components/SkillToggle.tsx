export function Toggle({
  on,
  onChange,
  disabled,
  ariaLabel,
}: {
  on: boolean;
  onChange: () => void;
  disabled?: boolean;
  ariaLabel?: string;
}) {
  return (
    <button
      type="button"
      role="switch"
      aria-checked={on}
      aria-label={ariaLabel}
      disabled={disabled}
      onClick={onChange}
      className={`ad-motion-fast-colors relative h-[20px] w-[36px] cursor-pointer rounded-full focus-visible:outline-none focus-visible:ring-2 focus-visible:ring-ring focus-visible:ring-offset-2 focus-visible:ring-offset-background ${
        on ? 'bg-primary' : 'bg-surface-active/55'
      }`}
      style={{
        opacity: disabled ? 0.3 : 1,
        cursor: disabled ? 'not-allowed' : 'pointer',
      }}
    >
      <span
        className={`ad-skill-toggle-thumb absolute left-[3px] top-[3px] h-[14px] w-[14px] rounded-full shadow-sm will-change-transform ${
          on ? 'bg-primary-foreground' : 'bg-foreground'
        }`}
        style={{
          transform: on ? 'translateX(16px)' : 'translateX(0)',
        }}
      />
    </button>
  );
}
