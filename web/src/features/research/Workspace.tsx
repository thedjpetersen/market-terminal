import { useState } from "react";
import { ArrowUpRight, BookmarkPlus, RefreshCw, Star } from "lucide-react";
import type { ResearchPort, Quote } from "./contracts";
import {
  Loading,
  Notice,
  SeriesChart,
  Sparkline,
  useQuery,
} from "../../ui/primitives";
export const money = (n: number, currency = "USD") =>
  new Intl.NumberFormat(undefined, {
    style: "currency",
    currency,
    maximumFractionDigits: 2,
  }).format(n);
export function QuoteTile({
  quote,
  onSelect,
}: {
  quote: Quote;
  onSelect: (symbol: string) => void;
}) {
  return (
    <button className="quote-tile" onClick={() => onSelect(quote.symbol)}>
      <div className="row">
        <strong>{quote.symbol}</strong>
        <ArrowUpRight size={15} />
      </div>
      <span className="truncate muted">{quote.name}</span>
      <div className="quote-price">{money(quote.price, quote.currency)}</div>
      <div className="row">
        <span className={quote.changePercent >= 0 ? "positive" : "negative"}>
          {quote.changePercent >= 0 ? "+" : ""}
          {quote.changePercent.toFixed(2)}%
        </span>
        <Sparkline values={quote.points} positive={quote.changePercent >= 0} />
      </div>
    </button>
  );
}
function Headlines({ port, symbol }: { port: ResearchPort; symbol: string }) {
  const result = useQuery((s) => port.news(symbol, s), `news:${symbol}`);
  return (
    <>
      {result.loading && <Loading label="Loading headlines" />}
      {result.error && <Notice>{result.error}</Notice>}
      {result.data?.length === 0 && (
        <Notice>No headlines are available for this ticker.</Notice>
      )}
      <div className="headlines">
        {result.data?.map((story, i) => (
          <a
            key={story.id}
            href={story.url}
            target="_blank"
            rel="noreferrer"
            className="headline"
          >
            <span className="story-number">
              {String(i + 1).padStart(2, "0")}
            </span>
            <div>
              <div className="eyebrow">
                {story.source}{" "}
                <span>
                  · {new Date(story.publishedAt).toLocaleDateString()}
                </span>
              </div>
              <h3>{story.title}</h3>
            </div>
            <ArrowUpRight size={17} />
          </a>
        ))}
      </div>
    </>
  );
}
export function Markets({
  port,
  onSelect,
}: {
  port: ResearchPort;
  onSelect: (symbol: string) => void;
}) {
  const [refresh, setRefresh] = useState(0),
    [benchmark, setBenchmark] = useState("SPY");
  const quotes = useQuery(
    (s) => port.quotes(["SPY", "QQQ", "DIA", "IWM"], s),
    "benchmarks",
    refresh,
  );
  const leaders = useQuery(
    (s) =>
      port.quotes(
        ["AAPL", "MSFT", "NVDA", "AMZN", "GOOGL", "META", "TSLA", "BTC-USD"],
        s,
      ),
    "leaders",
    refresh,
  );
  const history = useQuery(
    (s) => port.history(benchmark, "1m", s),
    `overview:${benchmark}`,
    refresh,
  );
  return (
    <>
      <div className="page-heading">
        <div>
          <div className="eyebrow accent">THE BIG PICTURE</div>
          <h1>
            Market overview<span className="accent">.</span>
          </h1>
          <p>A clearer view of the markets. A better place to start.</p>
        </div>
        <button className="button" onClick={() => setRefresh((v) => v + 1)}>
          <RefreshCw size={15} />
          Refresh
        </button>
      </div>
      {quotes.error && (
        <Notice retry={() => setRefresh((v) => v + 1)}>{quotes.error}</Notice>
      )}
      {quotes.loading && <Loading />}
      <div className="quote-grid">
        {quotes.data?.quotes.map((q) => (
          <QuoteTile key={q.symbol} quote={q} onSelect={onSelect} />
        ))}
      </div>
      {!!quotes.data?.unavailable.length && (
        <Notice>Unavailable: {quotes.data.unavailable.join(", ")}</Notice>
      )}
      <div className="overview-grid">
        <section className="panel">
          <div className="panel-heading">
            <div>
              <div className="eyebrow">MARKET PULSE</div>
              <h2>One month in perspective</h2>
            </div>
            <select
              aria-label="Benchmark"
              value={benchmark}
              onChange={(e) => setBenchmark(e.target.value)}
            >
              {["SPY", "QQQ", "DIA", "IWM"].map((s) => (
                <option key={s}>{s}</option>
              ))}
            </select>
          </div>
          {history.loading && <Loading />}
          {history.error && <Notice>{history.error}</Notice>}
          {history.data && (
            <SeriesChart
              points={history.data.bars.map((b) => ({
                x: b.timestamp,
                y: b.close,
              }))}
            />
          )}
          <div className="panel-foot">
            Daily closing prices · {benchmark} · Yahoo Finance
          </div>
        </section>
        <section className="panel">
          <div className="panel-heading">
            <div>
              <div className="eyebrow">ON THE RADAR</div>
              <h2>Market bellwethers</h2>
            </div>
            <span className="tag">8 assets</span>
          </div>
          {leaders.loading && <Loading />}
          {leaders.error && <Notice>{leaders.error}</Notice>}
          <div className="quote-list">
            {leaders.data?.quotes.map((q) => (
              <button key={q.symbol} onClick={() => onSelect(q.symbol)}>
                <span className="ticker-avatar">{q.symbol.slice(0, 2)}</span>
                <span className="asset-name">
                  <strong>{q.symbol}</strong>
                  <small>{q.name}</small>
                </span>
                <span className="asset-price">
                  {money(q.price, q.currency)}
                  <small
                    className={q.changePercent >= 0 ? "positive" : "negative"}
                  >
                    {q.changePercent >= 0 ? "+" : ""}
                    {q.changePercent.toFixed(2)}%
                  </small>
                </span>
              </button>
            ))}
          </div>
          {!!leaders.data?.unavailable.length && (
            <Notice>Unavailable: {leaders.data.unavailable.join(", ")}</Notice>
          )}
        </section>
      </div>
      <section className="panel">
        <div className="panel-heading">
          <div>
            <div className="eyebrow">THE LATEST</div>
            <h2>Headlines to watch</h2>
          </div>
          <span className="tag">SPY</span>
        </div>
        <Headlines port={port} symbol="SPY" />
      </section>
      <p className="source-note">
        Public, unofficial Yahoo Finance data. Prices may be delayed. Open an
        asset to see its source timestamp.
      </p>
    </>
  );
}
export function Research({
  port,
  symbol,
  watched,
  onWatch,
  onSave,
  onModel,
}: {
  port: ResearchPort;
  symbol: string;
  watched: boolean;
  onWatch: () => void;
  onSave: (title: string, content: string) => void;
  onModel: () => void;
}) {
  const [range, setRange] = useState("1m"),
    [tab, setTab] = useState("overview"),
    [refresh, setRefresh] = useState(0);
  const quote = useQuery(
    (s) => port.quotes([symbol], s),
    `quote:${symbol}`,
    refresh,
  );
  const history = useQuery(
    (s) => port.history(symbol, range, s),
    `history:${symbol}:${range}`,
    refresh,
  );
  const company = useQuery(
    (s) =>
      tab === "financials" || tab === "filings"
        ? port.company(symbol, s)
        : Promise.resolve(null),
    `company:${symbol}:${tab}`,
    refresh,
  );
  const q = quote.data?.quotes[0];
  return (
    <>
      <div className="page-heading">
        <div>
          <div className="eyebrow accent">SECURITY RESEARCH</div>
          <h1>
            {symbol}
            <span className="company-title">{q?.name}</span>
          </h1>
          <p>
            {q
              ? `${q.exchange} · ${q.currency} · ${q.source}`
              : "Public prices, company filings and market context."}
          </p>
        </div>
        <div className="actions">
          <button
            className={`button ${watched ? "selected" : ""}`}
            onClick={onWatch}
          >
            <Star size={16} fill={watched ? "currentColor" : "none"} />
            {watched ? "Watching" : "Watch"}
          </button>
          <button
            className="button"
            disabled={!q || !history.data}
            onClick={() =>
              onSave(
                `${symbol} research`,
                JSON.stringify({
                  symbol,
                  quote: q,
                  history: history.data,
                  company: company.data,
                  savedAt: new Date().toISOString(),
                }),
              )
            }
          >
            <BookmarkPlus size={16} />
            Save snapshot
          </button>
        </div>
      </div>
      {quote.loading && <Loading />}
      {quote.error && (
        <Notice retry={() => setRefresh((v) => v + 1)}>{quote.error}</Notice>
      )}
      {q && (
        <div className="security-price">
          <strong>{money(q.price, q.currency)}</strong>
          <span className={q.change >= 0 ? "positive" : "negative"}>
            {q.change >= 0 ? "+" : ""}
            {q.change.toFixed(2)} ({q.changePercent.toFixed(2)}%)
          </span>
          <small>As of {new Date(q.asOf).toLocaleString()} · Delayed</small>
        </div>
      )}
      <div className="tabs">
        {["overview", "financials", "filings", "news"].map((t) => (
          <button
            key={t}
            className={tab === t ? "active" : ""}
            onClick={() => setTab(t)}
          >
            {t}
          </button>
        ))}
      </div>
      {tab === "overview" && (
        <>
          <section className="panel">
            <div className="panel-heading">
              <h2>Price history</h2>
              <div className="segments">
                {["1d", "1m", "3m", "6m", "1y", "5y"].map((r) => (
                  <button
                    key={r}
                    className={range === r ? "active" : ""}
                    onClick={() => setRange(r)}
                  >
                    {r.toUpperCase()}
                  </button>
                ))}
              </div>
            </div>
            {history.loading && <Loading />}
            {history.error && <Notice>{history.error}</Notice>}
            {history.data && (
              <SeriesChart
                points={history.data.bars.map((b) => ({
                  x: b.timestamp,
                  y: b.close,
                }))}
              />
            )}
            <div className="panel-foot">
              {history.data?.bars.length ?? 0} observations ·{" "}
              {history.data?.source ?? "Yahoo Finance"}
            </div>
          </section>
          <div className="stat-grid">
            {[
              ["Previous close", q && money(q.previousClose, q.currency)],
              ["Session high", q?.high != null && money(q.high, q.currency)],
              ["Session low", q?.low != null && money(q.low, q.currency)],
              ["Volume", q?.volume?.toLocaleString()],
            ].map(([label, value]) => (
              <div className="stat" key={String(label)}>
                <span>{label}</span>
                <strong>{value || "—"}</strong>
              </div>
            ))}
          </div>
          <div className="callout">
            <div>
              <h2>Turn an idea into a test.</h2>
              <p>
                Explore option scenarios or backtest a moving-average strategy
                with the Rust research engine.
              </p>
            </div>
            <button className="button primary" onClick={onModel}>
              Open models <ArrowUpRight size={16} />
            </button>
          </div>
        </>
      )}
      {tab === "news" && (
        <section className="panel">
          <Headlines port={port} symbol={symbol} />
        </section>
      )}
      {(tab === "financials" || tab === "filings") && (
        <section className="panel">
          {company.loading && <Loading label="Loading SEC research" />}
          {company.error && <Notice>{company.error}</Notice>}
          {company.data && (
            <>
              <div className="panel-heading">
                <div>
                  <h2>{company.data.name}</h2>
                  <p className="muted">{company.data.industry} · SEC EDGAR</p>
                </div>
                <span className="tag">CIK {company.data.cik}</span>
              </div>
              {tab === "financials" ? (
                <div className="table-scroll">
                  <table>
                    <thead>
                      <tr>
                        <th>Fiscal period end</th>
                        <th>Revenue</th>
                        <th>Operating income</th>
                        <th>Net income</th>
                        <th>Diluted EPS</th>
                      </tr>
                    </thead>
                    <tbody>
                      {company.data.periods.map((p) => (
                        <tr key={p.end}>
                          <td>{p.end}</td>
                          <td>{money(p.revenue)}</td>
                          <td>
                            {p.operatingIncome === null
                              ? "—"
                              : money(p.operatingIncome)}
                          </td>
                          <td>
                            {p.netIncome === null ? "—" : money(p.netIncome)}
                          </td>
                          <td>{p.eps === null ? "—" : money(p.eps)}</td>
                        </tr>
                      ))}
                    </tbody>
                  </table>
                  {!company.data.periods.length && (
                    <Notice>
                      No comparable annual financial facts were found.
                    </Notice>
                  )}
                  <p className="source-note">
                    Annual US GAAP facts, latest filed value for each period.
                    Missing facts are shown as a dash.
                  </p>
                </div>
              ) : (
                <div className="headlines">
                  {company.data.filings.map((f) => (
                    <a
                      className="headline"
                      key={f.url}
                      href={f.url}
                      target="_blank"
                      rel="noreferrer"
                    >
                      <span className="tag">{f.form}</span>
                      <div>
                        <h3>{f.title}</h3>
                        <span className="muted">{f.date}</span>
                      </div>
                      <ArrowUpRight size={17} />
                    </a>
                  ))}
                </div>
              )}
            </>
          )}
        </section>
      )}
    </>
  );
}
