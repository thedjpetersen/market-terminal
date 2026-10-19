import { useEffect, useState } from "react";
import { ModalProvider, useModal } from "./ui/useModal";
import { Shell, type Destination } from "./app/Shell";
import { Markets, Research } from "./features/research/Workspace";
import { Search as SearchDialog } from "./features/research/Search";
import { Watchlist } from "./features/watchlist/Workspace";
import { Models } from "./features/models/Workspace";
import { Library } from "./features/library/Workspace";
import { publicResearch } from "./infrastructure/research";
import { cloudAnalytics } from "./infrastructure/analytics";
import { useWatchlist } from "./features/watchlist/state";
import { useLibrary } from "./features/library/state";
import { deviceWatchlist, deviceLibrary } from "./infrastructure/deviceStorage";
import { watchlistQuotes } from "./infrastructure/watchlist";
function route() {
  const p = new URLSearchParams(location.search),
    candidate = p.get("view");
  return {
    page: (["markets", "research", "watchlist", "models", "saved"].includes(
      candidate ?? "",
    )
      ? candidate
      : "markets") as Destination,
    symbol: /^[A-Z0-9^][A-Z0-9.^=-]{0,19}$/.test(p.get("symbol") ?? "")
      ? p.get("symbol")!
      : "AAPL",
  };
}
export function Application() {
  return (
    <ModalProvider>
      <ApplicationWorkspace />
    </ModalProvider>
  );
}
function ApplicationWorkspace() {
  const [current, setCurrent] = useState(route),
    [toast, setToast] = useState("");
  const search = useModal();
  const { symbols, toggle } = useWatchlist(deviceWatchlist, setToast);
  const { items: saved, save, remove } = useLibrary(deviceLibrary, setToast);
  useEffect(() => {
    const onPop = () => setCurrent(route());
    window.addEventListener("popstate", onPop);
    return () => window.removeEventListener("popstate", onPop);
  }, []);
  useEffect(() => {
    if (!toast) return;
    const timer = setTimeout(() => setToast(""), 5000);
    return () => clearTimeout(timer);
  }, [toast]);
  function navigate(page: Destination, symbol = current.symbol) {
    history.pushState(
      null,
      "",
      `?view=${page}&symbol=${encodeURIComponent(symbol)}`,
    );
    setCurrent({ page, symbol });
    window.scrollTo(0, 0);
  }
  const select = (symbol: string) => {
    search.close();
    navigate("research", symbol);
  };
  return (
    <>
      <Shell page={current.page} onNavigate={navigate} onSearch={search.show}>
        {current.page === "markets" && (
          <Markets port={publicResearch} onSelect={select} />
        )}{" "}
        {current.page === "research" && (
          <Research
            key={current.symbol}
            port={publicResearch}
            symbol={current.symbol}
            watched={symbols.includes(current.symbol)}
            onWatch={() => toggle(current.symbol)}
            onSave={save}
            onModel={() => navigate("models")}
          />
        )}{" "}
        {current.page === "watchlist" && (
          <Watchlist
            port={watchlistQuotes}
            symbols={symbols}
            onSelect={select}
            onRemove={toggle}
            onSearch={search.show}
          />
        )}{" "}
        {current.page === "models" && (
          <Models port={cloudAnalytics} symbol={current.symbol} onSave={save} />
        )}{" "}
        {current.page === "saved" && (
          <Library items={saved} onRemove={remove} />
        )}
      </Shell>
      {search.open && (
        <SearchDialog
          dialogRef={search.ref}
          port={publicResearch}
          onSelect={select}
          onClose={search.close}
        />
      )}{" "}
      {toast && (
        <div className="toast" role="status">
          {toast}
          <button
            aria-label="Dismiss notification"
            onClick={() => setToast("")}
          >
            ×
          </button>
        </div>
      )}
    </>
  );
}
