import React from "react";
import {
  MessageSquare,
  Volume2,
  Share2,
  FolderOpen,
  Save,
  Settings,
} from "lucide-react";

interface SidebarNavProps {
  selected: "chats" | "audio" | "mindmaps" | null;
  onSelect: (section: "chats" | "audio" | "mindmaps") => void;
  onImportProject: () => void;
  onExportProject: () => void;
  onOpenSettings: () => void;
  onOpenHuggingFaceSearch?: () => void;
  workspaceBusy?: boolean;
  canExport?: boolean;
}

const SidebarNav: React.FC<SidebarNavProps> = ({
  selected,
  onSelect,
  onImportProject,
  onExportProject,
  onOpenSettings,
  onOpenHuggingFaceSearch,
  workspaceBusy = false,
  canExport = false,
}) => {
  return (
    <nav className="relative flex flex-col gap-2 py-4 w-14 min-w-14 bg-void-800/80 backdrop-blur-xl items-center">
      <button
        className={`flex flex-col items-center gap-1 px-2 py-2 rounded-lg transition-colors ${selected === "chats" ? "bg-neon-subtle text-neon" : "text-txt-secondary hover:text-neon"}`}
        title="Chats"
        onClick={() => onSelect("chats")}
      >
        <MessageSquare size={30} />
      </button>

      <button
        className={`flex flex-col items-center gap-1 px-2 py-2 rounded-lg transition-colors ${selected === "audio" ? "bg-neon-subtle text-neon" : "text-txt-secondary hover:text-neon"}`}
        title="Audio Files"
        onClick={() => onSelect("audio")}
      >
        <Volume2 size={30} />
      </button>
      <button
        className={`flex flex-col items-center gap-1 px-2 py-2 rounded-lg transition-colors ${selected === "mindmaps" ? "bg-neon-subtle text-neon" : "text-txt-secondary hover:text-neon"}`}
        title="Mindmaps"
        onClick={() => onSelect("mindmaps")}
      >
        <Share2 size={30} />
      </button>

      <div className="mt-auto flex flex-col items-center gap-2 pb-1">
        <button
          className="flex flex-col items-center gap-1 px-2 py-2 rounded-lg transition-colors text-txt-secondary hover:text-neon"
          title="Search Hugging Face"
          onClick={onOpenHuggingFaceSearch}
        >
          <span role="img" aria-label="Hugging Face" style={{ fontSize: '22px' }}>🤗</span>
        </button>
        <button
          className="flex flex-col items-center gap-1 px-2 py-2 rounded-lg transition-colors text-txt-secondary hover:text-neon"
          title="Settings"
          onClick={onOpenSettings}
        >
          <Settings size={22} />
        </button>
        <button
          className="flex flex-col items-center gap-1 px-2 py-2 rounded-lg transition-colors text-txt-secondary hover:text-neon disabled:opacity-45 disabled:cursor-not-allowed"
          title="Import project (.nela)"
          onClick={() => onImportProject()}
          disabled={workspaceBusy}
        >
          <FolderOpen size={22} />
        </button>
        <button
          className="flex flex-col items-center gap-1 px-2 py-2 rounded-lg transition-colors text-txt-secondary hover:text-neon disabled:opacity-45 disabled:cursor-not-allowed"
          title="Export project (.nela)"
          onClick={() => onExportProject()}
          disabled={workspaceBusy || !canExport}
        >
          <Save size={22} />
        </button>
      </div>
    </nav>
  );
};

export default SidebarNav;
