import type { ChatSession, IngestionStatus } from "../types";
import type { RuntimeParamsTarget } from "./ActiveModelParamsDock";
import ActiveModelParamsDock from "./ActiveModelParamsDock";
import KnowledgeBaseSidebar from "./KnowledgeBaseSidebar";

interface AppRightSidebarProps {
  showRightSidebar: boolean;
  showParamsDock: boolean;
  docPanelOpen: boolean;
  activeRuntimeParamTarget: RuntimeParamsTarget | null;
  onApplyRuntimeParams: (nextParams: Record<string, string>) => Promise<void>;
  onCloseParamsDock: () => void;
  ragIngesting: boolean;
  enrichmentStatus: string | null;
  ragDocs: IngestionStatus[];
  activeSession: ChatSession | null;
  onCloseDocPanel: () => void;
  onIngestFile: () => void;
  onIngestDir: () => void;
  onOpenDocViewer: (doc: IngestionStatus) => void;
  onDeleteRagDoc: (docId: number) => void;
}

export default function AppRightSidebar({
  showRightSidebar,
  showParamsDock,
  docPanelOpen,
  activeRuntimeParamTarget,
  onApplyRuntimeParams,
  onCloseParamsDock,
  ragIngesting,
  enrichmentStatus,
  ragDocs,
  activeSession,
  onCloseDocPanel,
  onIngestFile,
  onIngestDir,
  onOpenDocViewer,
  onDeleteRagDoc,
}: AppRightSidebarProps) {
  if (!showRightSidebar) return null;

  return (
    <aside
      className={`kb-sidebar overflow-hidden bg-void-800 flex shrink-0 ${
        showParamsDock && docPanelOpen ? "w-160 min-w-160" : "w-[320px] min-w-[320px]"
      } border-l border-glass-border`}
    >
      {showParamsDock && activeRuntimeParamTarget && (
        <div className="w-[320px] min-w-[320px] h-full">
          <ActiveModelParamsDock
            target={activeRuntimeParamTarget}
            onApply={onApplyRuntimeParams}
            onClose={onCloseParamsDock}
          />
        </div>
      )}

      <KnowledgeBaseSidebar
        docPanelOpen={docPanelOpen}
        ragIngesting={ragIngesting}
        enrichmentStatus={enrichmentStatus}
        ragDocs={ragDocs}
        activeSession={activeSession}
        onClosePanel={onCloseDocPanel}
        onIngestFile={onIngestFile}
        onIngestDir={onIngestDir}
        onOpenDocViewer={onOpenDocViewer}
        onDeleteRagDoc={onDeleteRagDoc}
      />
    </aside>
  );
}
