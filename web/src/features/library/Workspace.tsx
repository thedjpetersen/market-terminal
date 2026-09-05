import { useModal } from "../../ui/useModal";
import { Bookmark, Download, Trash2, X } from "lucide-react";
import { useCallback, useState } from "react";
import type { SavedItem } from "./contracts";
export function Library({
  items,
  onRemove,
}: {
  items: SavedItem[];
  onRemove: (id: string) => void;
}) {
  const [selected, setSelected] = useState<SavedItem>();
  const close = useCallback(() => setSelected(undefined), []);
  useModal(!!selected, close);
  function download(item: SavedItem) {
    const url = URL.createObjectURL(
      new Blob([item.content], { type: "application/json" }),
    );
    const link = document.createElement("a");
    link.href = url;
    link.download = `market-research-${item.id}.json`;
    link.click();
    setTimeout(() => URL.revokeObjectURL(url), 1000);
  }
  return (
    <>
      <div className="page-heading">
        <div>
          <div className="eyebrow accent">KEEP YOUR WORK CLOSE</div>
          <h1>
            Saved research<span className="accent">.</span>
          </h1>
          <p>A record of your inputs, evidence and ideas.</p>
        </div>
        <span className="tag">{items.length} / 30 snapshots</span>
      </div>
      <section className="panel">
        {!items.length && (
          <div className="empty-state">
            <Bookmark size={36} />
            <h2>Good research deserves a record.</h2>
            <p>
              Save a security snapshot or model result. Your original data and
              exact model evidence will be available here.
            </p>
          </div>
        )}
        {items.map((item) => (
          <div className="saved-item" key={item.id}>
            <Bookmark size={19} />
            <button className="saved-title" onClick={() => setSelected(item)}>
              <strong>{item.title}</strong>
              <small>{new Date(item.createdAt).toLocaleString()}</small>
            </button>
            <button
              className="icon-button"
              aria-label={`Download ${item.title}`}
              onClick={() => download(item)}
            >
              <Download size={17} />
            </button>
            <button
              className="icon-button"
              aria-label={`Delete ${item.title}`}
              onClick={() => onRemove(item.id)}
            >
              <Trash2 size={17} />
            </button>
          </div>
        ))}
      </section>
      <p className="source-note">
        Stored only on this device. Download snapshots to keep a backup.
        Clearing browser data removes this library.
      </p>
      {selected && (
        <div className="modal-backdrop" onClick={() => setSelected(undefined)}>
          <section
            className="dialog evidence-dialog"
            role="dialog"
            aria-modal="true"
            aria-label={selected.title}
            onClick={(e) => e.stopPropagation()}
          >
            <div className="row">
              <h2>{selected.title}</h2>
              <button
                className="icon-button"
                aria-label="Close evidence"
                onClick={() => setSelected(undefined)}
              >
                <X size={20} />
              </button>
            </div>
            <p className="muted">
              Original JSON evidence · Download preserves all integer digits.
            </p>
            <pre>{selected.content}</pre>
            <button
              className="button primary"
              onClick={() => download(selected)}
            >
              <Download size={16} />
              Download evidence
            </button>
          </section>
        </div>
      )}
    </>
  );
}
