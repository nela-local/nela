import { Workflow, ArrowRight } from "lucide-react";

interface PlaygroundSidebarProps {
  onOpen: () => void;
}

export default function PlaygroundSidebar({ onOpen }: PlaygroundSidebarProps) {
  return (
    <aside className="w-[280px] min-w-[280px] border-r border-glass-border bg-void-800/80 backdrop-blur-xl flex flex-col">
      <div className="h-10 px-4 flex items-center shrink-0">
        <span className="text-2xl font-semibold mt-2 text-txt">Playground</span>
      </div>

      <div className="flex-1 p-3 flex flex-col gap-3">
        <div className="bg-void-900 border border-glass-border rounded-xl p-4 flex flex-col gap-3 shadow-md">
          <div className="flex items-center gap-2 text-neon">
            <Workflow size={18} />
            <span className="text-sm font-medium">Agentic Pipelines</span>
          </div>
          <p className="text-[0.8rem] text-txt-muted leading-relaxed">
            Build drag-and-drop ETL pipelines from on-device nodes — LLM steps, file I/O, email
            fetch, scripts, conditions and more.
          </p>
          <button
            onClick={onOpen}
            className="mt-1 flex items-center justify-center gap-2 w-full rounded-lg px-3 py-2 text-sm font-medium bg-neon-subtle text-neon border border-neon/20 hover:bg-neon/15 transition-colors"
          >
            Open Playground
            <ArrowRight size={14} />
          </button>
        </div>

        <p className="text-[0.72rem] text-txt-muted px-1 leading-relaxed">
          Pipelines are persisted locally. Use the canvas to wire nodes together, configure each
          step, then run or schedule the pipeline.
        </p>
      </div>
    </aside>
  );
}
