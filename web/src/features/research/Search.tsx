import { useEffect, useRef, useState } from "react";
import { Search as SearchIcon, ArrowUpRight, X } from "lucide-react";
import type { Instrument, ResearchPort } from "./contracts";
import { Loading, Notice } from "../../ui/primitives";
import { useModal } from "../../ui/useModal";
export function Search({
  port,
  onSelect,
  onClose,
}: {
  port: ResearchPort;
  onSelect: (symbol: string) => void;
  onClose: () => void;
}) {
  const [query, setQuery] = useState("");
  const [results, setResults] = useState<Instrument[]>([]);
  const [loading, setLoading] = useState(false);
  const [error, setError] = useState("");
  const [selected, select] = useState(0);
  const input = useRef<HTMLInputElement>(null);
  useModal(true, onClose);
  useEffect(() => {
    input.current?.focus();
    const handler = (e: KeyboardEvent) => {
      if (e.key === "Escape") onClose();
    };
    window.addEventListener("keydown", handler);
    return () => window.removeEventListener("keydown", handler);
  }, [onClose]);
  useEffect(() => {
    const controller = new AbortController();
    const timer = setTimeout(() => {
      setLoading(true);
      setError("");
      port
        .search(query.trim() || "AAPL", controller.signal)
        .then((result) => {
          if (!controller.signal.aborted) {
            setResults(result);
            select(0);
          }
        })
        .catch((e) => {
          if (!controller.signal.aborted) setError(String(e.message));
        })
        .finally(() => {
          if (!controller.signal.aborted) setLoading(false);
        });
    }, 200);
    return () => {
      clearTimeout(timer);
      controller.abort();
    };
  }, [query, port]);
  return (
    <div className="modal-backdrop search-backdrop" onClick={onClose}>
      <section
        className="search-dialog"
        role="dialog"
        aria-modal="true"
        aria-label="Search instruments"
        onClick={(e) => e.stopPropagation()}
      >
        <div className="search-field">
          <SearchIcon size={21} />
          <input
            ref={input}
            aria-label="Ticker or company"
            placeholder="Search a ticker or company…"
            value={query}
            onChange={(e) => setQuery(e.target.value)}
            onKeyDown={(e) => {
              if (e.key === "ArrowDown") {
                e.preventDefault();
                select(Math.min(results.length - 1, selected + 1));
              }
              if (e.key === "ArrowUp") {
                e.preventDefault();
                select(Math.max(0, selected - 1));
              }
              if (e.key === "Enter" && results[selected])
                onSelect(results[selected].symbol);
            }}
          />
          <button
            className="icon-button"
            aria-label="Close search"
            onClick={onClose}
          >
            <X size={20} />
          </button>
        </div>
        <div className="search-label">
          {query ? "INSTRUMENTS" : "START EXPLORING"}
        </div>
        {loading ? (
          <Loading label="Searching instruments" />
        ) : error ? (
          <Notice>{error}</Notice>
        ) : !results.length ? (
          <Notice>No matching instruments.</Notice>
        ) : (
          <div className="search-results">
            {results.map((item, index) => (
              <button
                key={item.symbol}
                className={selected === index ? "selected" : ""}
                onClick={() => onSelect(item.symbol)}
              >
                <span className="ticker-icon">{item.symbol.slice(0, 2)}</span>
                <span>
                  <strong>{item.symbol}</strong>
                  <small>{item.name}</small>
                </span>
                <span className="search-exchange">
                  {item.exchange || item.kind}
                </span>
                <ArrowUpRight size={16} />
              </button>
            ))}
          </div>
        )}
        <div className="search-footer">
          ↑ ↓ to navigate <span>↵ to open</span>
          <span>esc to close</span>
        </div>
      </section>
    </div>
  );
}
