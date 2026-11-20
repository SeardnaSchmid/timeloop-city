// A decision-rule formula, with its label.
export function Formula({ label, src }: { label: string; src: string }) {
  return (
    <div style={{ display: 'flex', gap: 8, margin: '3px 0' }}>
      <span className="muted" style={{ minWidth: 54, fontSize: 12 }}>{label}</span>
      <code style={{ whiteSpace: 'pre-wrap' }}>{src}</code>
    </div>
  );
}
