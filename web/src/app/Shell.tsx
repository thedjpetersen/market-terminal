import { useModal } from "../ui/useModal";
import { useCallback, useEffect, useState } from "react";
import {
  ArrowUpRight,
  Bookmark,
  ChartNoAxesCombined,
  ChevronRight,
  CircleHelp,
  Globe2,
  Search,
  SlidersHorizontal,
  Star,
} from "lucide-react";
export type Destination =
  "markets" | "research" | "watchlist" | "models" | "saved";
const destinations = [
  { id: "markets", label: "Markets", icon: Globe2 },
  { id: "watchlist", label: "Watchlist", icon: Star },
  { id: "research", label: "Research", icon: ChartNoAxesCombined },
  { id: "models", label: "Models", icon: SlidersHorizontal },
  { id: "saved", label: "Saved research", icon: Bookmark },
] as const;
export function Shell({
  page,
  onNavigate,
  onSearch,
  children,
}: {
  page: Destination;
  onNavigate: (page: Destination) => void;
  onSearch: () => void;
  children: React.ReactNode;
}) {
  const [clock, setClock] = useState(new Date());
  const [help, setHelp] = useState(false);
  const closeHelp = useCallback(() => setHelp(false), []);
  useModal(help, closeHelp);
  useEffect(() => {
    const timer = setInterval(() => setClock(new Date()), 1000);
    return () => clearInterval(timer);
  }, []);
  useEffect(() => {
    const handler = (e: KeyboardEvent) => {
      if ((e.metaKey || e.ctrlKey) && e.key === "k") {
        e.preventDefault();
        onSearch();
      }
    };
    window.addEventListener("keydown", handler);
    return () => window.removeEventListener("keydown", handler);
  }, [onSearch]);
  return (
    <div className="terminal-shell">
      <aside className="sidebar">
        <a
          className="brand"
          href="/"
          onClick={(e) => {
            e.preventDefault();
            onNavigate("markets");
          }}
        >
          <span className="brand-mark">
            M<span>↗</span>
          </span>
          <span>
            MARKET<span className="brand-sub">TERMINAL</span>
          </span>
        </a>
        <div className="nav-caption">WORKSPACE</div>
        <nav aria-label="Main navigation">
          {destinations.map(({ id, label, icon: Icon }) => (
            <button
              key={id}
              className={`nav-item ${page === id ? "active" : ""}`}
              onClick={() => onNavigate(id)}
              aria-current={page === id ? "page" : undefined}
            >
              <Icon size={18} />
              <span>{label}</span>
              {page === id && <span className="nav-dot" />}
            </button>
          ))}
        </nav>
        <div className="sidebar-bottom">
          <div className="connection">
            <span className="status-dot" />
            CLOUDFLARE EDGE
          </div>
          <p>
            Your research.
            <br />
            Anywhere you are.
          </p>
          <button className="quiet" onClick={() => setHelp(true)}>
            <CircleHelp size={16} /> About this workspace{" "}
            <ArrowUpRight size={13} />
          </button>
        </div>
      </aside>
      <div className="main-shell">
        <header className="topbar">
          <div className="breadcrumbs">
            <span className="desktop-only">Workspace</span>
            <ChevronRight size={13} className="desktop-only" />
            <strong>{destinations.find((d) => d.id === page)?.label}</strong>
            <span className="mobile-brand">Market Terminal</span>
          </div>
          <button
            className="search-trigger"
            aria-label="Search a ticker or company"
            onClick={onSearch}
          >
            <Search size={16} />
            <span>Search a ticker or company</span>
            <kbd>⌘ K</kbd>
          </button>
          <time className="host-clock">
            {clock.toISOString().slice(11, 19)} <span>UTC</span>
          </time>
          <span className="avatar" title="Local workspace">
            MT
          </span>
        </header>
        <main id="main-content" key={page}>
          {children}
        </main>
        <footer className="app-footer">
          <span>
            <span className="status-dot" /> Public market research
          </span>
          <span>Source timestamps shown · Quotes may be delayed</span>
        </footer>
      </div>
      <nav className="mobile-nav" aria-label="Mobile navigation">
        {destinations.map(({ id, label, icon: Icon }) => (
          <button
            key={id}
            className={page === id ? "active" : ""}
            onClick={() => onNavigate(id)}
            aria-current={page === id ? "page" : undefined}
          >
            <Icon size={20} />
            <span>{label === "Saved research" ? "Saved" : label}</span>
          </button>
        ))}
      </nav>
      {help && (
        <div className="modal-backdrop" onClick={() => setHelp(false)}>
          <section
            className="dialog"
            role="dialog"
            aria-modal="true"
            aria-label="About Market Terminal"
            onClick={(e) => e.stopPropagation()}
          >
            <h2>Research, wherever you are.</h2>
            <p>
              Explore public market data and SEC filings. Options, bonds and
              backtests use the same Rust engine as the native terminal, running
              on your device.
            </p>
            <p>
              Watchlists and saved research stay in this browser. They are not
              synced between devices. Market quotes may be delayed; analytical
              models use explicit assumptions and are not trading instructions.
            </p>
            <p>
              On mobile, use your browser’s “Add to Home Screen” action for a
              standalone workspace.
            </p>
            <button className="button primary" onClick={() => setHelp(false)}>
              Back to workspace
            </button>
          </section>
        </div>
      )}
    </div>
  );
}
