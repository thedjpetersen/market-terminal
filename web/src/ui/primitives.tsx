import { useEffect, useState, useId } from "react";
import { AlertCircle, LoaderCircle, RefreshCw } from "lucide-react";

export function useQuery<T>(
  load: (signal: AbortSignal) => Promise<T>,
  key: string,
  refresh = 0,
) {
  const [state, set] = useState<{ data?: T; error?: string; loading: boolean }>(
    { loading: true },
  );
  useEffect(() => {
    const controller = new AbortController();
    set({ loading: true });
    load(controller.signal)
      .then((data) => {
        if (!controller.signal.aborted) set({ data, loading: false });
      })
      .catch((error) => {
        if (!controller.signal.aborted)
          set({
            error: error instanceof Error ? error.message : String(error),
            loading: false,
          });
      });
    return () => controller.abort();
    // Callers identify the complete resource with key; unstable closures aren't dependencies.
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [key, refresh]);
  return state;
}

export function Notice({
  children,
  retry,
}: {
  children: React.ReactNode;
  retry?: () => void;
}) {
  return (
    <div className="notice" role="status">
      <AlertCircle size={18} />
      <span>{children}</span>
      {retry && (
        <button className="quiet" onClick={retry}>
          <RefreshCw size={14} />
          Retry
        </button>
      )}
    </div>
  );
}
export function Loading({ label = "Loading market data" }: { label?: string }) {
  return (
    <div className="loading" role="status">
      <LoaderCircle size={19} className="spin" />
      {label}…
    </div>
  );
}
export function Sparkline({
  values,
  positive = true,
}: {
  values: number[];
  positive?: boolean;
}) {
  if (values.length < 2) return <span className="muted">—</span>;
  const min = Math.min(...values),
    span = Math.max(...values) - min || 1;
  const path = values
    .map(
      (v, i) =>
        `${i ? "L" : "M"}${(i / (values.length - 1)) * 120},${32 - ((v - min) / span) * 28}`,
    )
    .join(" ");
  return (
    <svg
      className={`sparkline ${positive ? "positive" : "negative"}`}
      viewBox="0 0 120 36"
      aria-hidden="true"
    >
      <path
        d={path}
        fill="none"
        stroke="currentColor"
        strokeWidth="1.8"
        vectorEffect="non-scaling-stroke"
      />
    </svg>
  );
}
export function SeriesChart({
  points,
  format = (n) => n.toFixed(2),
  label = "Price history",
}: {
  points: { x: number; y: number }[];
  format?: (value: number) => string;
  label?: string;
}) {
  const id = useId().replaceAll(":", "");
  const [hover, setHover] = useState<number | null>(null);
  if (points.length < 2)
    return (
      <Notice>There are not enough observations to draw this chart.</Notice>
    );
  const min = Math.min(...points.map((p) => p.y)),
    max = Math.max(...points.map((p) => p.y));
  const span = max - min || Math.abs(max) * 0.02 || 1;
  const y = (v: number) => 240 - ((v - min) / span) * 214;
  const x = (i: number) => 12 + (i / (points.length - 1)) * 866;
  const path = points
    .map((p, i) => `${i ? "L" : "M"}${x(i)},${y(p.y)}`)
    .join(" ");
  const index =
    hover === null ? points.length - 1 : Math.min(hover, points.length - 1);
  const selected = points[index];
  const positive = points.at(-1)!.y >= points[0].y;
  const date = (timestamp: number) =>
    new Date(timestamp * 1000).toLocaleDateString(undefined, {
      month: "short",
      day: "numeric",
      year: "numeric",
    });
  return (
    <div className={`series-chart ${positive ? "positive" : "negative"}`}>
      <div className="chart-readout">
        <span>{date(selected.x)}</span>
        <strong>{format(selected.y)}</strong>
      </div>
      <div className="chart-body">
        <svg
          viewBox="0 0 900 270"
          role="img"
          aria-label={label}
          preserveAspectRatio="none"
          onPointerMove={(event) => {
            const rect = event.currentTarget.getBoundingClientRect();
            setHover(
              Math.max(
                0,
                Math.min(
                  points.length - 1,
                  Math.round(
                    ((event.clientX - rect.left) / rect.width) *
                      (points.length - 1),
                  ),
                ),
              ),
            );
          }}
          onPointerLeave={() => setHover(null)}
        >
          <defs>
            <linearGradient id={id} x1="0" y1="0" x2="0" y2="1">
              <stop stopColor="currentColor" stopOpacity=".17" />
              <stop offset="1" stopColor="currentColor" stopOpacity="0" />
            </linearGradient>
          </defs>
          {[26, 79, 133, 186, 240].map((v) => (
            <line
              key={v}
              x1="0"
              x2="900"
              y1={v}
              y2={v}
              stroke="var(--line)"
              strokeDasharray="3 6"
            />
          ))}
          <path d={`${path} L878,262 L12,262 Z`} fill={`url(#${id})`} />
          <path
            d={path}
            stroke="currentColor"
            strokeWidth="2"
            fill="none"
            vectorEffect="non-scaling-stroke"
          />
          {hover !== null && (
            <>
              <line
                x1={x(index)}
                x2={x(index)}
                y1="0"
                y2="260"
                stroke="var(--muted)"
                strokeDasharray="4 4"
              />
              <circle
                cx={x(index)}
                cy={y(selected.y)}
                r="4"
                fill="currentColor"
              />
            </>
          )}
        </svg>
        <div className="chart-scale">
          {[1, 0.75, 0.5, 0.25, 0].map((f) => (
            <span key={f}>{format(min + span * f)}</span>
          ))}
        </div>
      </div>
      <div className="chart-dates">
        <span>{date(points[0].x)}</span>
        <span>{date(points[Math.floor(points.length / 2)].x)}</span>
        <span>{date(points.at(-1)!.x)}</span>
      </div>
      <input
        className="chart-scrubber"
        aria-label={`Inspect ${label.toLowerCase()}`}
        type="range"
        min="0"
        max={points.length - 1}
        value={index}
        onChange={(e) => setHover(Number(e.target.value))}
      />
    </div>
  );
}
