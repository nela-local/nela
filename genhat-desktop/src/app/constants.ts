import type { ElementType } from "react";
import {
  MessageSquare,
  Eye,
  Volume2,
  Mic,
  Share2,
} from "lucide-react";
import type { ChatMode } from "../types";

export const SESSION_STORAGE_PREFIX = "genhat:sessions:v1:";
export const STARTUP_OPTIONAL_DOWNLOAD_KEY = "genhat:download-optional-on-start";

export const STARTUP_MODEL_SELECTOR = {
  tasks: new Set(["embed", "grade", "classify", "tts", "transcribe", "stt"]),
  ids: new Set([
    "kitten-tts",
    "parakeet-tdt",
    "qwen3.5-0_8b",
    "mmproj-LFM2.5-VL-450m-F16",
    "LFM2.5-VL-450M-F32",
  ]),
};

/** Extensions the DocumentViewer can render (non-PDF). */
export const VIEWABLE_EXTS = new Set([
  "docx", "pptx", "xlsx", "xls", "ods",
  "txt", "md", "csv", "tsv", "json", "xml", "html", "htm",
  "rs", "py", "js", "ts", "jsx", "tsx", "java", "c", "cpp",
  "h", "go", "rb", "sh", "css", "scss", "sql", "log", "ini", "cfg",
  "toml", "yaml", "yml",
  "png", "jpg", "jpeg", "gif", "webp", "bmp", "svg",
  "mp3", "wav", "ogg", "m4a", "flac",
]);

export interface ModeConfigItem {
  mode: ChatMode;
  label: string;
  icon: ElementType;
  desc: string;
}

export const MODE_CONFIG: ModeConfigItem[] = [
  { mode: "text", label: "Chat", icon: MessageSquare, desc: "Text conversation" },
  { mode: "vision", label: "Vision", icon: Eye, desc: "Image analysis" },
  { mode: "audio", label: "Audio", icon: Volume2, desc: "Text to speech" },
  { mode: "podcast", label: "Podcast", icon: Mic, desc: "AI podcast generation" },
  { mode: "mindmap", label: "Mindmap", icon: Share2, desc: "Visual idea map" },
];
