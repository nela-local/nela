/**
 * PlaygroundNodePalette — drag-to-canvas node palette sidebar.
 *
 * Groups nodes by category. Dragging a tile onto the canvas
 * transfers the NodeKind via dataTransfer so the canvas can create the node.
 */

import { useState } from "react";
import { ChevronDown, ChevronRight } from "lucide-react";
import { PALETTE, type PaletteEntry } from "../app/playgroundTypes";

const CATEGORY_ORDER = ["trigger", "ai", "io", "logic", "script"] as const;
type Category = (typeof CATEGORY_ORDER)[number];

const CATEGORY_LABEL: Record<Category, string> = {
  trigger: "Triggers",
  ai: "AI",
  io: "I / O",
  logic: "Logic",
  script: "Scripts",
};

function PaletteItem({ entry }: { entry: PaletteEntry }) {
  const onDragStart = (event: React.DragEvent) => {
    event.dataTransfer.setData("application/playground-node", entry.kind);
    event.dataTransfer.effectAllowed = "move";
  };

  return (
    <div
      draggable
      onDragStart={onDragStart}
      className="
        flex flex-col gap-0.5 px-3 py-2 rounded-lg cursor-grab
        bg-white/5 hover:bg-white/10 border border-white/10
        hover:border-white/20 transition-colors text-xs select-none
      "
      title={entry.description}
    >
      <span className="font-medium text-txt-primary truncate">{entry.label}</span>
      <span className="text-txt-muted text-[10px] leading-tight line-clamp-1">
        {entry.description}
      </span>
    </div>
  );
}

function CategorySection({ category, entries }: { category: Category; entries: PaletteEntry[] }) {
  const [open, setOpen] = useState(true);

  return (
    <div className="flex flex-col">
      <button
        onClick={() => setOpen(o => !o)}
        className="
          flex items-center gap-1 px-2 py-1.5 text-[10px] font-semibold
          text-txt-muted uppercase tracking-widest hover:text-txt-primary
          transition-colors select-none
        "
      >
        {open ? <ChevronDown size={10} /> : <ChevronRight size={10} />}
        {CATEGORY_LABEL[category]}
      </button>
      {open && (
        <div className="flex flex-col gap-1.5 px-1">
          {entries.map(entry => (
            <PaletteItem key={entry.kind} entry={entry} />
          ))}
        </div>
      )}
    </div>
  );
}

export default function PlaygroundNodePalette() {
  const grouped = CATEGORY_ORDER.reduce<Record<Category, PaletteEntry[]>>(
    (acc, cat) => {
      acc[cat] = PALETTE.filter(e => e.category === cat);
      return acc;
    },
    { trigger: [], ai: [], io: [], logic: [], script: [] }
  );

  return (
    <aside
      className="
        w-52 shrink-0 flex flex-col gap-3 overflow-y-auto
        border-r border-white/10 bg-void-950 px-2 py-3
      "
    >
      <p className="px-2 text-[10px] font-semibold text-txt-muted uppercase tracking-widest">
        Nodes
      </p>
      <p className="px-2 text-[10px] text-txt-muted leading-snug">
        Drag a node onto the canvas to add it to your pipeline.
      </p>
      {CATEGORY_ORDER.map(cat =>
        grouped[cat].length > 0 ? (
          <CategorySection key={cat} category={cat} entries={grouped[cat]} />
        ) : null
      )}
    </aside>
  );
}
