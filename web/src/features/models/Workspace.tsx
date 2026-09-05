import { useRef, useState } from "react";
import { Play, BookmarkPlus, Download, Cpu } from "lucide-react";
import {
  parseResearchJson,
  stringifyResearchJson,
  type ResearchRequest,
  type ResearchResponse,
} from "../../../../contracts/web/client/index";
import type { AnalyticsPort } from "./contracts";
import { fixed, fixedInput } from "./numbers";
import { Loading, Notice, SeriesChart } from "../../ui/primitives";
type Model = "option" | "bond" | "backtest";
const fields: Record<Model, [string, string, string][]> = {
  option: [
    ["spot", "Underlying price ($)", "200"],
    ["strike", "Strike price ($)", "200"],
    ["days", "Days to expiration", "30"],
    ["volatility", "Volatility (%)", "25"],
    ["rate", "Risk-free rate (%)", "4"],
    ["dividend", "Dividend yield (%)", "0"],
    ["multiplier", "Contract multiplier", "100"],
  ],
  bond: [
    ["face", "Face value ($)", "1000"],
    ["coupon", "Coupon rate (%)", "5"],
    ["yield", "Yield to maturity (%)", "4"],
    ["years", "Years to maturity", "10"],
    ["accrued", "Coupon period elapsed (%)", "0"],
  ],
  backtest: [
    ["fast", "Fast moving average (days)", "20"],
    ["slow", "Slow moving average (days)", "50"],
    ["cash", "Initial cash ($)", "10000"],
    ["cost", "Execution cost (basis points)", "5"],
    ["commission", "Commission per trade ($)", "0"],
  ],
};
export function Models({
  port,
  symbol,
  onSave,
}: {
  port: AnalyticsPort;
  symbol: string;
  onSave: (title: string, content: string) => void;
}) {
  const [model, setModel] = useState<Model>("option"),
    [values, setValues] = useState<Record<string, string>>({}),
    [right, setRight] = useState<"call" | "put">("call"),
    [frequency, setFrequency] = useState<
      "annual" | "semi_annual" | "quarterly"
    >("semi_annual");
  const [result, setResult] = useState<ResearchResponse>(),
    [raw, setRaw] = useState(""),
    [error, setError] = useState(""),
    [busy, setBusy] = useState(false);
  const output = useRef<HTMLElement>(null);
  const value = (key: string) =>
    values[key] ?? fields[model].find((f) => f[0] === key)![2];
  const integer = (key: string) => fixedInput(value(key), 0),
    micros = (key: string) => fixedInput(value(key)),
    bps = (key: string) => fixedInput(value(key), 2);
  async function run(event: React.FormEvent) {
    event.preventDefault();
    setBusy(true);
    setError("");
    setResult(undefined);
    setRaw("");
    try {
      const envelope = {
        schema_version: 1n as const,
        request_id: crypto.randomUUID(),
      };
      let request: ResearchRequest;
      if (model === "option")
        request = {
          ...envelope,
          operation: "price_option",
          input: {
            symbol,
            right,
            spot_micros: micros("spot"),
            strike_micros: micros("strike"),
            days_to_expiry: integer("days"),
            volatility_bps: bps("volatility"),
            risk_free_rate_bps: bps("rate"),
            dividend_yield_bps: bps("dividend"),
            contract_multiplier: integer("multiplier"),
          },
        };
      else if (model === "bond")
        request = {
          ...envelope,
          operation: "analyze_bond",
          input: {
            instrument_id: "HYPOTHETICAL-USD-BOND",
            currency: "USD",
            face_micros: micros("face"),
            coupon_bps: bps("coupon"),
            yield_bps: bps("yield"),
            years_to_maturity: integer("years"),
            frequency,
            accrued_period_bps: bps("accrued"),
          },
        };
      else {
        const bars = await port.bars(symbol);
        request = {
          ...envelope,
          operation: "run_backtest",
          input: {
            config: {
              instrument_id: symbol,
              symbol,
              fast_window: integer("fast"),
              slow_window: integer("slow"),
              execution_cost_bps: integer("cost"),
              commission_micros: micros("commission"),
              initial_cash_micros: micros("cash"),
            },
            bars: bars.map((b) => ({
              timestamp: BigInt(b.timestamp),
              open_micros: fixedInput(b.open.toFixed(6)),
              high_micros: fixedInput(b.high.toFixed(6)),
              low_micros: fixedInput(b.low.toFixed(6)),
              close_micros: fixedInput(b.close.toFixed(6)),
              volume: BigInt(Math.round(b.volume)),
            })),
            source: "Yahoo Finance chart",
            quality:
              "delayed unofficial provider daily OHLC; excludes distributions",
            input_version: "web-yahoo-daily-v1",
          },
        };
      }
      const requestText = stringifyResearchJson(request),
        responseText = await port.execute(requestText);
      const response = parseResearchJson(responseText) as ResearchResponse;
      if (response.status === "error") throw new Error(response.error.message);
      if (
        response.schema_version !== 1n ||
        response.request_id !== request.request_id
      )
        throw new Error("The engine returned an incompatible response.");
      setResult(response);
      if (matchMedia("(max-width: 760px)").matches)
        output.current?.scrollIntoView({ behavior: "smooth", block: "start" });
      setRaw(`{"request":${requestText},"response":${responseText}}`);
    } catch (e) {
      setError(e instanceof Error ? e.message : String(e));
    } finally {
      setBusy(false);
    }
  }
  return (
    <>
      <div className="page-heading">
        <div>
          <div className="eyebrow accent">FROM QUESTION TO CONVICTION</div>
          <h1>
            Research models<span className="accent">.</span>
          </h1>
          <p>Explicit assumptions. Reproducible calculations. Your device.</p>
        </div>
        <span className="engine-badge">
          <Cpu size={15} /> RUST + WEBASSEMBLY
        </span>
      </div>
      <div className="tabs">
        {(["option", "bond", "backtest"] as const).map((m) => (
          <button
            disabled={busy}
            key={m}
            className={model === m ? "active" : ""}
            onClick={() => {
              setModel(m);
              setResult(undefined);
              setError("");
              setRaw("");
            }}
          >
            {m === "option"
              ? "Options pricing"
              : m === "bond"
                ? "Fixed income"
                : "Strategy backtest"}
          </button>
        ))}
      </div>
      <div className="model-grid">
        <form className="panel model-form" onSubmit={run}>
          <div className="panel-heading">
            <h2>Model inputs</h2>
            <span className="tag">{model === "bond" ? "USD" : symbol}</span>
          </div>
          <p className="source-note">
            {model === "option"
              ? "Hypothetical inputs; edit the spot price and assumptions. European option model."
              : model === "bond"
                ? "Hypothetical fixed-rate bond with regular coupon periods."
                : `Moving-average crossover on one year of ${symbol} daily prices. Change the ticker using search.`}
          </p>
          {model === "option" && (
            <label>
              Option right
              <select
                disabled={busy}
                aria-label="Option right"
                value={right}
                onChange={(e) => {
                  setRight(e.target.value as typeof right);
                  setResult(undefined);
                  setRaw("");
                }}
              >
                <option value="call">Call</option>
                <option value="put">Put</option>
              </select>
            </label>
          )}
          {fields[model].map(([key, label]) => (
            <label key={key}>
              {label}
              <input
                disabled={busy}
                inputMode="decimal"
                required
                value={value(key)}
                onChange={(e) => {
                  setValues((v) => ({ ...v, [key]: e.target.value }));
                  setResult(undefined);
                  setRaw("");
                }}
              />
            </label>
          ))}
          {model === "bond" && (
            <label>
              Coupon frequency
              <select
                disabled={busy}
                aria-label="Coupon frequency"
                value={frequency}
                onChange={(e) => {
                  setFrequency(e.target.value as typeof frequency);
                  setResult(undefined);
                  setRaw("");
                }}
              >
                <option value="annual">Annual</option>
                <option value="semi_annual">Semiannual</option>
                <option value="quarterly">Quarterly</option>
              </select>
            </label>
          )}
          <button className="button primary run-button" disabled={busy}>
            <Play size={16} />
            {busy ? "Computing…" : "Run model"}
          </button>
        </form>
        <section ref={output} className="panel model-output">
          <div className="panel-heading">
            <h2>Results</h2>
            {raw && (
              <div className="actions">
                <button
                  className="icon-button"
                  aria-label="Save model result"
                  onClick={() => onSave(`${model} · ${symbol}`, raw)}
                >
                  <BookmarkPlus size={18} />
                </button>
                <button
                  className="icon-button"
                  aria-label="Download exact model evidence"
                  onClick={() => download(raw, `market-${model}.json`)}
                >
                  <Download size={18} />
                </button>
              </div>
            )}
          </div>
          {busy && <Loading label="Running shared Rust engine" />}
          {error && <Notice>{error}</Notice>}
          {!busy && !error && !result && (
            <div className="empty-state">
              <Cpu size={36} />
              <h2>Make your assumptions explicit.</h2>
              <p>
                Choose your inputs and run a model. Results include scenario
                analysis and the engine’s methodology.
              </p>
              <span className="tag">Exact integer evidence · Runs locally</span>
            </div>
          )}
          {result?.status === "ok" && <ModelResult result={result} />}
        </section>
      </div>
    </>
  );
}
export function download(text: string, name: string) {
  const url = URL.createObjectURL(
    new Blob([text], { type: "application/json" }),
  );
  const a = document.createElement("a");
  a.href = url;
  a.download = name;
  a.click();
  setTimeout(() => URL.revokeObjectURL(url), 1000);
}
function ModelResult({
  result,
}: {
  result: Extract<ResearchResponse, { status: "ok" }>;
}) {
  let stats: [string, string][] = [];
  if (result.result_type === "option_analytics") {
    const d = result.data;
    stats = [
      ["Option price", "$" + fixed(d.price_micros)],
      ["Intrinsic value", "$" + fixed(d.intrinsic_micros)],
      ["Time value", "$" + fixed(d.time_value_micros)],
      ["Delta", fixed(d.delta_millionths, 6, 4)],
      ["Gamma", fixed(d.gamma_billionths, 9, 6)],
      ["Vega / vol point", "$" + fixed(d.vega_micros_per_point)],
      ["Theta / day", "$" + fixed(d.theta_micros_per_day)],
      ["Rho / rate point", "$" + fixed(d.rho_micros_per_point)],
    ];
  }
  if (result.result_type === "bond_analytics") {
    const d = result.data;
    stats = [
      ["Clean price", "$" + fixed(d.clean_price_micros)],
      ["Dirty price", "$" + fixed(d.dirty_price_micros)],
      ["Accrued interest", "$" + fixed(d.accrued_interest_micros)],
      ["Current yield", fixed(d.current_yield_bps, 2) + "%"],
      [
        "Modified duration",
        fixed(d.modified_duration_years_millionths) + " yrs",
      ],
      ["DV01", "$" + fixed(d.dv01_micros)],
    ];
  }
  if (result.result_type === "backtest") {
    const d = result.data;
    stats = [
      ["Final equity", "$" + fixed(d.final_equity_micros)],
      ["Total return", fixed(d.total_return_bps, 2) + "%"],
      ["Max drawdown", fixed(d.max_drawdown_bps, 2) + "%"],
      ["Trades", String(d.trades.length)],
    ];
  }
  return (
    <>
      <div className="result-stats">
        {stats.map(([label, value]) => (
          <div className="stat" key={label}>
            <span>{label}</span>
            <strong>{value}</strong>
          </div>
        ))}
      </div>
      {result.result_type === "backtest" && (
        <>
          <SeriesChart
            points={result.data.equity.map((p) => ({
              x: Number(p.timestamp),
              y: Number(p.equity_micros) / 1e6,
            }))}
            label="Backtest equity"
          />
          <p className="source-note">
            Chart values are rounded for display. Downloads preserve exact
            integer evidence.
          </p>
          <div className="table-scroll">
            <table>
              <thead>
                <tr>
                  <th>Executed</th>
                  <th>Side</th>
                  <th>Quantity</th>
                  <th>Price</th>
                </tr>
              </thead>
              <tbody>
                {result.data.trades.map((t, i) => (
                  <tr key={i}>
                    <td>
                      {new Date(
                        Number(t.execution_timestamp) * 1000,
                      ).toLocaleDateString()}
                    </td>
                    <td>{t.side}</td>
                    <td>{String(t.quantity)}</td>
                    <td>${fixed(t.execution_price_micros)}</td>
                  </tr>
                ))}
              </tbody>
            </table>
            {!result.data.trades.length && (
              <Notice>No trades were triggered during this period.</Notice>
            )}
          </div>
        </>
      )}
      {result.result_type === "option_analytics" && (
        <div className="table-scroll">
          <h3>Spot & volatility scenarios</h3>
          <table>
            <thead>
              <tr>
                <th>Spot shock</th>
                <th>Vol shift</th>
                <th>Option price</th>
                <th>Contract value</th>
              </tr>
            </thead>
            <tbody>
              {result.data.scenarios.map((s, i) => (
                <tr key={i}>
                  <td>{fixed(s.spot_shock_bps, 2)}%</td>
                  <td>{fixed(s.volatility_shift_bps, 2)} pts</td>
                  <td>${fixed(s.price_micros)}</td>
                  <td>${fixed(s.contract_value_micros)}</td>
                </tr>
              ))}
            </tbody>
          </table>
        </div>
      )}
      {result.result_type === "bond_analytics" && (
        <div className="table-scroll">
          <h3>Yield scenarios</h3>
          <table>
            <thead>
              <tr>
                <th>Yield shock</th>
                <th>Clean price</th>
                <th>Price change</th>
              </tr>
            </thead>
            <tbody>
              {result.data.scenarios.map((s, i) => (
                <tr key={i}>
                  <td>{String(s.shock_bps)} bps</td>
                  <td>${fixed(s.clean_price_micros)}</td>
                  <td>{fixed(s.clean_change_bps, 2)}%</td>
                </tr>
              ))}
            </tbody>
          </table>
        </div>
      )}
      <details className="methodology">
        <summary>Methodology & disclosures</summary>
        <p>{result.data.methodology}</p>
        {"disclosures" in result.data &&
          result.data.disclosures.map((text) => <p key={text}>{text}</p>)}
        <p className="mono">
          {"input_digest" in result.data
            ? result.data.input_digest
            : "run_digest" in result.data
              ? result.data.run_digest
              : result.data.comparison_digest}
        </p>
      </details>
    </>
  );
}
