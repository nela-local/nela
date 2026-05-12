import React from "react";
import {
  MessageSquare,
  Volume2,
  Share2,
  Workflow,
  FolderOpen,
  Save,
  Settings,
  HelpCircle,
  Sun,
  Moon,
} from "lucide-react";

interface SidebarNavProps {
  selected: "chats" | "audio" | "mindmaps" | "playground" | null;
  onSelect: (section: "chats" | "audio" | "mindmaps" | "playground") => void;
  onImportProject: () => void;
  onExportProject: () => void;
  onOpenSettings: () => void;
  onOpenTours: () => void;
  onOpenHuggingFaceSearch?: () => void;
  workspaceBusy?: boolean;
  canExport?: boolean;
  theme?: "dark" | "light";
  onToggleTheme?: () => void;
}

const SidebarNav: React.FC<SidebarNavProps> = ({
  selected,
  onSelect,
  onImportProject,
  onExportProject,
  onOpenSettings,
  onOpenTours,
  onOpenHuggingFaceSearch,
  workspaceBusy = false,
  canExport = false,
  theme = "dark",
  onToggleTheme,
}) => {
  return (
    <nav
      className="relative flex flex-col gap-2 py-4 w-14 min-w-14 bg-void-800/80 backdrop-blur-xl items-center"
      data-tour="sidebar-nav"
    >
      <button
        className={`flex flex-col items-center gap-1 px-2 py-2 rounded-lg transition-colors ${selected === "chats" ? "bg-neon-subtle text-neon" : "text-txt-secondary hover:text-neon"}`}
        title="Chats"
        onClick={() => onSelect("chats")}
        data-tour="sidebar-chats"
      >
        <MessageSquare size={30} />
      </button>

      <button
        className={`flex flex-col items-center gap-1 px-2 py-2 rounded-lg transition-colors ${selected === "audio" ? "bg-neon-subtle text-neon" : "text-txt-secondary hover:text-neon"}`}
        title="Audio Files"
        onClick={() => onSelect("audio")}
        data-tour="sidebar-audio"
      >
        <Volume2 size={30} />
      </button>
      <button
        className={`flex flex-col items-center gap-1 px-2 py-2 rounded-lg transition-colors ${selected === "mindmaps" ? "bg-neon-subtle text-neon" : "text-txt-secondary hover:text-neon"}`}
        title="Mindmaps"
        onClick={() => onSelect("mindmaps")}
        data-tour="sidebar-mindmaps"
      >
        <Share2 size={30} />
      </button>
      <button
        className={`flex flex-col items-center gap-1 px-2 py-2 rounded-lg transition-colors ${selected === "playground" ? "bg-neon-subtle text-neon" : "text-txt-secondary hover:text-neon"}`}
        title="Playground"
        onClick={() => onSelect("playground")}
        data-tour="sidebar-playground"
      >
        <Workflow size={30} />
      </button>

      <div className="mt-auto flex flex-col items-center gap-2 pb-1">
        <button
          className="flex flex-col items-center gap-1 px-2 py-2 rounded-lg transition-colors text-txt-secondary hover:text-neon"
          title="Search Hugging Face"
          onClick={onOpenHuggingFaceSearch}
          data-tour="sidebar-hf"
        >
          <span role="img" aria-label="Hugging Face" style={{ fontSize: "22px" }}>🤗</span>
        </button>
        <button
          className="flex flex-col items-center gap-1 px-2 py-2 rounded-lg transition-colors text-txt-secondary hover:text-neon"
          title={theme === "dark" ? "Switch to light mode" : "Switch to dark mode"}
          onClick={onToggleTheme}
        >
          {theme === "dark" ? <Sun size={22} /> : <Moon size={22} />}
        </button>
        <button
          className="flex flex-col items-center gap-1 px-2 py-2 rounded-lg transition-colors text-txt-secondary hover:text-neon"
          title="Help · Tours"
          onClick={onOpenTours}
          data-tour="sidebar-help-tours"
        >
          <HelpCircle size={22} />
        </button>
        <button
          className="flex flex-col items-center gap-1 px-2 py-2 rounded-lg transition-colors text-txt-secondary hover:text-neon"
          title="Settings"
          onClick={onOpenSettings}
          data-tour="sidebar-settings"
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
