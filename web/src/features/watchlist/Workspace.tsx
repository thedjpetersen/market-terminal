import { Star, Trash2, Plus } from "lucide-react";
import { Loading, Notice, Sparkline, useQuery } from "../../ui/primitives";
import type { WatchlistPort } from "./contracts";
export function Watchlist({
  port,
  symbols,
  onSelect,
  onRemove,
  onSearch,
}: {
  port: WatchlistPort;
  symbols: string[];
  onSelect: (symbol: string) => void;
  onRemove: (symbol: string) => void;
  onSearch: () => void;
}) {
  const data = useQuery(
    (s) =>
      symbols.length
        ? port.quotes(symbols, s)
        : Promise.resolve({ quotes: [], unavailable: [] }),
    symbols.join(","),
  );
  return (
    <>
      <div className="page-heading">
        <div>
          <div className="eyebrow accent">YOUR CORNER OF THE MARKET</div>
          <h1>
            Watchlist<span className="accent">.</span>
          </h1>
          <p>The assets you care about, together in one place.</p>
        </div>
        <button className="button primary" onClick={onSearch}>
          <Plus size={16} />
          Find an asset
        </button>
      </div>
      <section className="panel">
        {data.loading && <Loading />}
        {data.error && <Notice>{data.error}</Notice>}
        {!symbols.length && (
          <div className="empty-state">
            <Star size={36} />
            <h2>Build your market view.</h2>
            <p>
              Find a company or ticker, then tap Watch on its research page.
            </p>
            <button className="button" onClick={onSearch}>
              Search assets
            </button>
          </div>
        )}
        <div className="watch-items">
          {data.data?.quotes.map((q) => (
            <div key={q.symbol} className="watch-item">
              <button className="watch-main" onClick={() => onSelect(q.symbol)}>
                <span className="ticker-avatar">{q.symbol.slice(0, 2)}</span>
                <span className="asset-name">
                  <strong>{q.symbol}</strong>
                  <small>{q.name}</small>
                </span>
                <Sparkline values={q.points} positive={q.changePercent >= 0} />
                <span className="asset-price">
                  {new Intl.NumberFormat(undefined, {
                    style: "currency",
                    currency: q.currency,
                  }).format(q.price)}
                  <small
                    className={q.changePercent >= 0 ? "positive" : "negative"}
                  >
                    {q.changePercent >= 0 ? "+" : ""}
                    {q.changePercent.toFixed(2)}%
                  </small>
                </span>
              </button>
              <button
                className="icon-button"
                aria-label={`Remove ${q.symbol} from watchlist`}
                onClick={() => onRemove(q.symbol)}
              >
                <Trash2 size={16} />
              </button>
            </div>
          ))}
        </div>
        {data.data?.unavailable.map((symbol) => (
          <div className="watch-item" key={symbol}>
            <Notice>{symbol}: data is currently unavailable.</Notice>
            <button
              className="icon-button"
              aria-label={`Remove ${symbol} from watchlist`}
              onClick={() => onRemove(symbol)}
            >
              <Trash2 size={16} />
            </button>
          </div>
        ))}
      </section>
      <p className="source-note">
        Saved in this browser · Up to 32 tickers · No account or cross-device
        sync
      </p>
    </>
  );
}
