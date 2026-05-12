/**
 * PlaygroundFlowNode — custom React Flow node renderer.
 *
 * Renders a node card with its kind icon, label, and connection handles.
 */

import { memo } from "react";
import { Handle, Position } from "@xyflow/react";
import {
  Clock,
  Play,
  MessageSquare,
  FileText,
  Mic,
  Volume2,
  Database,
  FolderOpen,
  Save,
  Mail,
  GitBranch,
  Code2,
  Bell,
  Terminal,
  Globe,
  Rss,
  Braces,
  VariableIcon,
  type LucideIcon,
} from "lucide-react";
import type { PlaygroundNodeData, NodeKind } from "../app/playgroundTypes";

const KIND_ICON: Record<NodeKind, LucideIcon> = {
  Schedule: Clock,
  Manual: Play,
  LlmChat: MessageSquare,
  Summarize: FileText,
  Transcribe: Mic,
  Tts: Volume2,
  RagQuery: Database,
  FileRead: FolderOpen,
  FileWrite: Save,
  EmailFetch: Mail,
  Condition: GitBranch,
  Template: Code2,
  Notification: Bell,
  Script: Terminal,
  HttpRequest: Globe,
  RssReader: Rss,
  JsonPath: Braces,
  SetVariable: VariableIcon,
};

const KIND_COLOR: Record<NodeKind, string> = {
  Schedule: "bg-indigo-500/20 border-indigo-500/40 text-indigo-300",
  Manual: "bg-emerald-500/20 border-emerald-500/40 text-emerald-300",
  LlmChat: "bg-violet-500/20 border-violet-500/40 text-violet-300",
  Summarize: "bg-violet-500/20 border-violet-500/40 text-violet-300",
  Transcribe: "bg-blue-500/20 border-blue-500/40 text-blue-300",
  Tts: "bg-blue-500/20 border-blue-500/40 text-blue-300",
  RagQuery: "bg-cyan-500/20 border-cyan-500/40 text-cyan-300",
  FileRead: "bg-amber-500/20 border-amber-500/40 text-amber-300",
  FileWrite: "bg-amber-500/20 border-amber-500/40 text-amber-300",
  EmailFetch: "bg-orange-500/20 border-orange-500/40 text-orange-300",
  Condition: "bg-rose-500/20 border-rose-500/40 text-rose-300",
  Template: "bg-teal-500/20 border-teal-500/40 text-teal-300",
  Notification: "bg-sky-500/20 border-sky-500/40 text-sky-300",
  Script: "bg-pink-500/20 border-pink-500/40 text-pink-300",
  HttpRequest: "bg-lime-500/20 border-lime-500/40 text-lime-300",
  RssReader: "bg-orange-500/20 border-orange-500/40 text-orange-300",
  JsonPath: "bg-yellow-500/20 border-yellow-500/40 text-yellow-300",
  SetVariable: "bg-teal-500/20 border-teal-500/40 text-teal-300",
};

interface PlaygroundFlowNodeProps {
  data: PlaygroundNodeData;
  selected?: boolean;
}

function PlaygroundFlowNode({ data, selected }: PlaygroundFlowNodeProps) {
  const Icon = KIND_ICON[data.kind] ?? Play;
  const colorClass = KIND_COLOR[data.kind] ?? "bg-white/10 border-white/20 text-white";

  return (
    <div
      className={`
        min-w-[140px] max-w-[200px] rounded-xl border backdrop-blur-sm
        ${colorClass}
        ${selected ? "ring-2 ring-white/40 shadow-lg" : ""}
        transition-shadow
      `}
    >
      {/* target handle (top) */}
      <Handle
        type="target"
        position={Position.Top}
        className="!bg-white/60 !w-2.5 !h-2.5 !border-2 !border-white/30"
      />

      <div className="flex items-center gap-2 px-3 py-2.5">
        <Icon size={15} className="shrink-0 opacity-90" />
        <span className="text-xs font-medium truncate leading-tight">{data.label}</span>
      </div>

      {/* source handle (bottom) */}
      <Handle
        type="source"
        position={Position.Bottom}
        className="!bg-white/60 !w-2.5 !h-2.5 !border-2 !border-white/30"
      />
    </div>
  );
}

export default memo(PlaygroundFlowNode);
