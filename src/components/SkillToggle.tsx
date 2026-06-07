export function Toggle({
  on,
  onChange,
  disabled,
}: {
  on: boolean;
  onChange: () => void;
  disabled?: boolean;
}) {
  return (
    <button
      type="button"
      role="switch"
      aria-checked={on}
      onClick={disabled ? undefined : onChange}
      className="relative h-[20px] w-[36px] cursor-pointer rounded-full"
      style={{
        background: on ? 'rgba(180,180,190,0.7)' : 'rgba(120,120,128,0.32)',
        opacity: disabled ? 0.3 : 1,
        cursor: disabled ? 'not-allowed' : 'pointer',
      }}
    >
      <span
        className="absolute left-[3px] top-[3px] h-[14px] w-[14px] rounded-full will-change-transform"
        style={{
          transform: on ? 'translateX(16px)' : 'translateX(0)',
          background: '#fff',
          boxShadow: '0 1px 2px rgba(0,0,0,0.25)',
          transition: 'transform 0.12s ease-out',
        }}
      />
    </button>
  );
}
