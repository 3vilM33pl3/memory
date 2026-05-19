export function Metric({ label, value }: { label: string; value: string }) {
  return (
    <div className="metric-row">
      <span>{label}</span>
      <strong>{value}</strong>
    </div>
  );
}

export function KeyValueList({ items, empty }: { items: [string, string][]; empty: string }) {
  if (!items.length) return <p className="muted">{empty}</p>;
  return (
    <div className="kv-list">
      {items.map(([key, value]) => (
        <div className="kv-row" key={key}>
          <span>{key}</span>
          <strong>{value}</strong>
        </div>
      ))}
    </div>
  );
}
