import React from "react";
import { MessageSquare, Volume2, Share2 } from "lucide-react";

interface SidebarNavProps {
  selected: "chats" | "audio" | "mindmaps" | null;
  onSelect: (section: "chats" | "audio" | "mindmaps") => void;
}

const SidebarNav: React.FC<SidebarNavProps> = ({ selected, onSelect }) => (
  <nav className="flex flex-col gap-2 py-4 w-14 min-w-[56px] bg-void-800/80 backdrop-blur-xl items-center">
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
  </nav>
);

export default SidebarNav;
