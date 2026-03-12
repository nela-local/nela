import { useState, useEffect, useRef } from "react";
import { renderAsync } from "docx-preview";
import hljs from "highlight.js";
import JSZip from "jszip";
import ReactMarkdown from "react-markdown";
import remarkGfm from "remark-gfm";
import {
  X,
  ZoomIn,
  ZoomOut,
  ChevronLeft,
  ChevronRight,
  FileText,
  Code2,
  Image as ImageIcon,
  Music,
  Table2,
  FileType,
} from "lucide-react";
import { Api } from "../api";

/* ── Helpers ──────────────────────────────────────────────────────────────── */

const EXT_TO_LANG: Record<string, string> = {
  js: "javascript", jsx: "javascript", ts: "typescript", tsx: "typescript",
  py: "python", rs: "rust", go: "go", rb: "ruby", java: "java",
  c: "c", cpp: "cpp", h: "c", hpp: "cpp", cs: "csharp",
  sh: "bash", bat: "batch", ps1: "powershell",
  html: "html", htm: "html", css: "css", scss: "scss", less: "less",
  json: "json", xml: "xml", yaml: "yaml", yml: "yaml", toml: "toml",
  sql: "sql", md: "markdown", csv: "plaintext", tsv: "plaintext",
  log: "plaintext", ini: "ini", cfg: "ini", txt: "plaintext",
};

type ViewerKind =
  | "pdf" | "docx" | "pptx" | "spreadsheet"
  | "image" | "audio" | "markdown" | "code" | "plaintext" | "unknown";

function classifyFile(ext: string): ViewerKind {
  switch (ext) {
    case "pdf": return "pdf";
    case "docx": return "docx";
    case "pptx": return "pptx";
    case "xlsx": case "xls": case "ods": return "spreadsheet";
    case "png": case "jpg": case "jpeg": case "gif": case "webp": case "bmp": case "svg":
      return "image";
    case "mp3": case "wav": case "ogg": case "m4a": case "flac": case "aac":
      return "audio";
    case "md": return "markdown";
    default:
      if (EXT_TO_LANG[ext]) return "code";
      return "plaintext";
  }
}

function getFileIcon(kind: ViewerKind) {
  switch (kind) {
    case "code": return <Code2 size={16} />;
    case "image": return <ImageIcon size={16} />;
    case "audio": return <Music size={16} />;
    case "spreadsheet": return <Table2 size={16} />;
    default: return <FileText size={16} />;
  }
}

/* ── Props ────────────────────────────────────────────────────────────────── */

interface DocumentViewerProps {
  filePath: string;
  title: string;
  onClose: () => void;
}

/* ── Main Component ───────────────────────────────────────────────────────── */

export default function DocumentViewer({ filePath, title, onClose }: DocumentViewerProps) {
  const ext = filePath.split(".").pop()?.toLowerCase() || "";
  const kind = classifyFile(ext);

  const [loading, setLoading] = useState(true);
  const [error, setError] = useState<string | null>(null);

  // Text-based content
  const [textContent, setTextContent] = useState<string>("");

  // Binary data URL (for images, audio, docx raw)
  const [dataUrl, setDataUrl] = useState<string>("");

  // Code viewer state
  const [fontSize, setFontSize] = useState(14);

  // PPTX slide state (we render as extracted HTML from backend)
  // For now, PPTX is handled as a rough text extraction
  // Future: integrate PPTXjs for full fidelity

  const docxContainerRef = useRef<HTMLDivElement>(null);
  const viewerRef = useRef<HTMLDivElement>(null);

  /* ── Keyboard shortcuts ── */
  useEffect(() => {
    const handler = (e: KeyboardEvent) => {
      if (e.key === "Escape") onClose();
      if ((e.ctrlKey || e.metaKey) && e.key === "=") { e.preventDefault(); setFontSize(s => Math.min(s + 2, 28)); }
      if ((e.ctrlKey || e.metaKey) && e.key === "-") { e.preventDefault(); setFontSize(s => Math.max(s - 2, 8)); }
    };
    window.addEventListener("keydown", handler);
    return () => window.removeEventListener("keydown", handler);
  }, [onClose]);

  /* ── Load file content ── */
  useEffect(() => {
    let cancelled = false;

    async function loadFile() {
      try {
        setLoading(true);
        setError(null);
        setDataUrl("");
        setTextContent("");

        if (kind === "image" || kind === "audio" || kind === "docx" || kind === "pptx") {
          // Binary files: load as base64 data URL
          const url = await Api.readFileBase64(filePath);
          if (!cancelled) setDataUrl(url);
        } else if (kind === "spreadsheet") {
          // Spreadsheet: load as base64, extract on frontend with simple table
          const url = await Api.readFileBase64(filePath);
          if (!cancelled) setDataUrl(url);
        } else {
          // Text-based files: load as raw text
          const text = await Api.readFileText(filePath);
          if (!cancelled) setTextContent(text);
        }
      } catch (e) {
        if (!cancelled) setError(`Failed to load file: ${e}`);
      } finally {
        if (!cancelled) setLoading(false);
      }
    }

    loadFile();
    return () => { cancelled = true; };
  }, [filePath, kind]);

  /* ── Render DOCX once data is ready ── */
  useEffect(() => {
    if (kind !== "docx" || !dataUrl || !docxContainerRef.current) return;

    let cancelled = false;
    const container = docxContainerRef.current;
    container.innerHTML = ""; // clear previous

    // Extract ArrayBuffer from data URL
    const base64 = dataUrl.split(",")[1];
    const binaryString = atob(base64);
    const bytes = new Uint8Array(binaryString.length);
    for (let i = 0; i < binaryString.length; i++) {
      bytes[i] = binaryString.charCodeAt(i);
    }

    renderAsync(bytes.buffer, container, undefined, {
      className: "docx-preview-wrapper",
      inWrapper: true,
      ignoreWidth: false,
      ignoreHeight: false,
      ignoreFonts: false,
      breakPages: true,
      ignoreLastRenderedPageBreak: true,
      experimental: false,
      trimXmlDeclaration: true,
      useBase64URL: true,
      renderHeaders: true,
      renderFooters: true,
      renderFootnotes: true,
      renderEndnotes: true,
    }).catch((err: unknown) => {
      if (!cancelled) setError(`Failed to render DOCX: ${err}`);
    });

    return () => { cancelled = true; };
  }, [kind, dataUrl, filePath]);

  /* ── Subrenderers ───────────────────────────────────────────────────────── */

  const renderContent = () => {
    if (loading) {
      return (
        <div className="dv-loading">
          <div className="dv-spinner" />
          <span>Loading {title}...</span>
        </div>
      );
    }

    if (error) {
      return (
        <div className="dv-error">
          <FileType size={48} />
          <p>{error}</p>
        </div>
      );
    }

    switch (kind) {
      case "image":
        return <ImageViewer dataUrl={dataUrl} title={title} />;

      case "audio":
        return <AudioViewer dataUrl={dataUrl} title={title} />;

      case "docx":
        return (
          <div className="dv-docx-wrapper">
            <div ref={docxContainerRef} className="dv-docx-container" />
          </div>
        );

      case "markdown":
        return <MarkdownViewer content={textContent} />;

      case "code":
        return <CodeViewer content={textContent} ext={ext} fontSize={fontSize} />;

      case "pptx":
        return <PptxViewer dataUrl={dataUrl} />;

      case "spreadsheet":
        return <SpreadsheetViewer dataUrl={dataUrl} />;

      case "plaintext":
      default:
        return <CodeViewer content={textContent} ext="txt" fontSize={fontSize} />;
    }
  };

  return (
    <div className="dv-overlay" ref={viewerRef}>
      {/* ── Toolbar ── */}
      <div className="dv-toolbar">
        <div className="dv-toolbar-left">
          {getFileIcon(kind)}
          <span className="dv-toolbar-title" title={filePath}>{title}</span>
          <span className="dv-toolbar-ext">.{ext}</span>
        </div>
        <div className="dv-toolbar-right">
          {(kind === "code" || kind === "plaintext") && (
            <>
              <button className="glass-btn dv-btn" onClick={() => setFontSize(s => Math.max(s - 2, 8))} title="Zoom Out (Ctrl+-)">
                <ZoomOut size={16} />
              </button>
              <span className="dv-font-size">{fontSize}px</span>
              <button className="glass-btn dv-btn" onClick={() => setFontSize(s => Math.min(s + 2, 28))} title="Zoom In (Ctrl+=)">
                <ZoomIn size={16} />
              </button>
            </>
          )}
          <button className="glass-btn dv-btn dv-close-btn" onClick={onClose} title="Close (Esc)">
            <X size={18} />
          </button>
        </div>
      </div>

      {/* ── Content ── */}
      <div className="dv-content">
        {renderContent()}
      </div>
    </div>
  );
}

/* ═══════════════════════════════════════════════════════════════════════════
   Sub-Viewers
   ═══════════════════════════════════════════════════════════════════════════ */

/* ── Code Viewer (VS Code-like) ── */

function CodeViewer({ content, ext, fontSize }: { content: string; ext: string; fontSize: number }) {
  const codeRef = useRef<HTMLElement>(null);
  const lang = EXT_TO_LANG[ext] || "plaintext";
  const lines = content.split("\n");

  useEffect(() => {
    if (codeRef.current) {
      // Reset highlight state
      codeRef.current.removeAttribute("data-highlighted");
      try {
        hljs.highlightElement(codeRef.current);
      } catch {
        // Fallback: no highlighting
      }
    }
  }, [content, lang]);

  return (
    <div className="dv-code-container" style={{ fontSize: `${fontSize}px` }}>
      {/* Line numbers gutter */}
      <div className="dv-line-numbers" style={{ fontSize: `${fontSize}px` }}>
        {lines.map((_, i) => (
          <span key={i} className="dv-line-num">{i + 1}</span>
        ))}
      </div>
      {/* Code content */}
      <pre className="dv-code-pre">
        <code ref={codeRef} className={`language-${lang}`}>
          {content}
        </code>
      </pre>
    </div>
  );
}

/* ── Markdown Viewer ── */

function MarkdownViewer({ content }: { content: string }) {
  return (
    <div className="dv-markdown-container">
      <ReactMarkdown remarkPlugins={[remarkGfm]}>
        {content}
      </ReactMarkdown>
    </div>
  );
}

/* ── Image Viewer ── */

function ImageViewer({ dataUrl, title }: { dataUrl: string; title: string }) {
  const [zoom, setZoom] = useState(1);
  return (
    <div className="dv-image-container">
      <div className="dv-image-controls">
        <button className="dv-btn" onClick={() => setZoom(z => Math.max(z - 0.25, 0.25))}>
          <ZoomOut size={16} />
        </button>
        <span className="dv-zoom-label">{Math.round(zoom * 100)}%</span>
        <button className="dv-btn" onClick={() => setZoom(z => Math.min(z + 0.25, 5))}>
          <ZoomIn size={16} />
        </button>
        <button className="dv-btn" onClick={() => setZoom(1)}>Fit</button>
      </div>
      <div className="dv-image-scroll">
        <img
          src={dataUrl}
          alt={title}
          style={{ transform: `scale(${zoom})`, transformOrigin: "top left" }}
          className="dv-image"
        />
      </div>
    </div>
  );
}

/* ── Audio Viewer ── */

function AudioViewer({ dataUrl, title }: { dataUrl: string; title: string }) {
  return (
    <div className="dv-audio-container">
      <Music size={64} strokeWidth={1} className="dv-audio-icon" />
      <h3 className="dv-audio-title">{title}</h3>
      <audio controls src={dataUrl} className="dv-audio-player">
        Your browser does not support the audio element.
      </audio>
    </div>
  );
}

/* ── PPTX Viewer (slide-based text extraction) ── */

function PptxViewer({ dataUrl }: { dataUrl: string }) {
  const [slides, setSlides] = useState<string[]>([]);
  const [currentSlide, setCurrentSlide] = useState(0);
  const [loadError, setLoadError] = useState<string | null>(null);

  useEffect(() => {
    if (!dataUrl) return;

    // Extract slide text from PPTX using JSZip-like approach
    // PPTX is a ZIP with slide XML files
    extractPptxSlides(dataUrl)
      .then(setSlides)
      .catch((e) => setLoadError(`Could not parse PPTX: ${e}`));
  }, [dataUrl]);

  if (loadError) {
    return <div className="dv-error"><p>{loadError}</p></div>;
  }

  if (slides.length === 0) {
    return <div className="dv-loading"><span>Parsing slides...</span></div>;
  }

  return (
    <div className="dv-pptx-container">
      {/* Slide navigation */}
      <div className="dv-pptx-nav">
        <button
          className="dv-btn"
          disabled={currentSlide === 0}
          onClick={() => setCurrentSlide(s => s - 1)}
        >
          <ChevronLeft size={16} />
        </button>
        <span className="dv-pptx-counter">
          Slide {currentSlide + 1} of {slides.length}
        </span>
        <button
          className="dv-btn"
          disabled={currentSlide === slides.length - 1}
          onClick={() => setCurrentSlide(s => s + 1)}
        >
          <ChevronRight size={16} />
        </button>
      </div>

      {/* Current slide */}
      <div className="dv-pptx-slide">
        <div className="dv-pptx-slide-content" dangerouslySetInnerHTML={{ __html: slides[currentSlide] }} />
      </div>

      {/* Slide thumbnails */}
      <div className="dv-pptx-thumbs">
        {slides.map((_, i) => (
          <button
            key={i}
            className={`glass-btn dv-pptx-thumb ${i === currentSlide ? "active" : ""}`}
            onClick={() => setCurrentSlide(i)}
          >
            {i + 1}
          </button>
        ))}
      </div>
    </div>
  );
}

/** Parse PPTX data URL into slide HTML content using JSZip */
async function extractPptxSlides(dataUrl: string): Promise<string[]> {
  // Convert data URL to ArrayBuffer
  const base64 = dataUrl.split(",")[1];
  const binaryString = atob(base64);
  const bytes = new Uint8Array(binaryString.length);
  for (let i = 0; i < binaryString.length; i++) {
    bytes[i] = binaryString.charCodeAt(i);
  }

  const zip = await JSZip.loadAsync(bytes.buffer);

  // Find all slide XML files
  const slideFiles = Object.keys(zip.files)
    .filter(name => /^ppt\/slides\/slide\d+\.xml$/.test(name))
    .sort((a, b) => {
      const numA = parseInt(a.match(/slide(\d+)/)?.[1] || "0");
      const numB = parseInt(b.match(/slide(\d+)/)?.[1] || "0");
      return numA - numB;
    });

  const slides: string[] = [];
  for (const name of slideFiles) {
    const xmlText = await zip.file(name)!.async("string");
    const html = pptxXmlToHtml(xmlText);
    slides.push(html);
  }

  return slides.length > 0 ? slides : ["<p>(No slides found)</p>"];
}

/** Convert PowerPoint slide XML to simplified HTML */
function pptxXmlToHtml(xml: string): string {
  const parser = new DOMParser();
  const doc = parser.parseFromString(xml, "text/xml");

  const textBlocks: string[] = [];

  // Helper: getElementsByTagName with namespace-agnostic fallback
  function getElements(parent: Element | Document, localName: string): Element[] {
    // Try namespace-aware first (OOXML uses specific namespaces)
    let els = parent.getElementsByTagNameNS("*", localName);
    if (els.length === 0) {
      // Fallback: try with common PPTX prefixes
      els = parent.getElementsByTagName(`p:${localName}`);
      if (els.length === 0) els = parent.getElementsByTagName(`a:${localName}`);
      if (els.length === 0) els = parent.getElementsByTagName(localName);
    }
    return Array.from(els);
  }

  // Find the shape tree: <p:spTree> or just spTree
  const spTrees = getElements(doc, "spTree");
  const spTree = spTrees[0];
  if (!spTree) return "<p>(Empty slide)</p>";

  // Find all shape elements
  const shapes = getElements(spTree, "sp");
  for (const shape of shapes) {
    // Get paragraphs within this shape
    const paragraphs = getElements(shape, "p").filter(p => {
      // Only include <a:p> paragraphs, not <p:sp> etc
      const tag = p.tagName.toLowerCase();
      return tag === "a:p" || tag === "p" || p.namespaceURI?.includes("drawingml");
    });
    const shapeTexts: string[] = [];

    for (const para of paragraphs) {
      // Get all text runs <a:r>
      const runs = getElements(para, "r");
      let paraText = "";

      for (const run of runs) {
        const tElements = getElements(run, "t");
        for (const tEl of tElements) {
          const text = tEl.textContent || "";
          // Check for bold / large font
          const rPr = getElements(run, "rPr")[0];
          const isBold = rPr?.getAttribute("b") === "1";
          const fontSize = rPr?.getAttribute("sz");
          const isLarge = fontSize ? parseInt(fontSize) >= 2400 : false;

          if (isBold || isLarge) {
            paraText += `<strong>${escapeHtml(text)}</strong>`;
          } else {
            paraText += escapeHtml(text);
          }
        }
      }

      // Also check for direct <a:t> text not in a run (field codes etc)
      if (!paraText.trim()) {
        const directT = getElements(para, "t");
        for (const tEl of directT) {
          const text = tEl.textContent || "";
          if (text.trim()) paraText += escapeHtml(text);
        }
      }

      if (paraText.trim()) {
        const pPr = getElements(para, "pPr")[0];
        const lvl = pPr?.getAttribute("lvl");
        if (lvl && parseInt(lvl) > 0) {
          shapeTexts.push(`<li>${paraText}</li>`);
        } else {
          shapeTexts.push(`<p>${paraText}</p>`);
        }
      }
    }

    if (shapeTexts.length > 0) {
      const hasListItems = shapeTexts.some(t => t.startsWith("<li>"));
      if (hasListItems) {
        textBlocks.push(`<ul>${shapeTexts.join("")}</ul>`);
      } else {
        textBlocks.push(shapeTexts.join(""));
      }
    }
  }

  return textBlocks.length > 0
    ? textBlocks.join('<hr class="dv-pptx-divider" />')
    : "<p>(Empty slide)</p>";
}

function escapeHtml(text: string): string {
  return text
    .replace(/&/g, "&amp;")
    .replace(/</g, "&lt;")
    .replace(/>/g, "&gt;")
    .replace(/"/g, "&quot;");
}

/* ── Spreadsheet Viewer (CSV-like table from binary) ── */

function SpreadsheetViewer({ dataUrl }: { dataUrl: string }) {
  // For spreadsheets, we read back as text from the backend
  // Since we have base64, and xlsx is binary, we display a message
  // and suggest using the RAG ingestion for full content
  const content = "Spreadsheet preview is available through RAG ingestion. The file has been loaded successfully.";
  const loading = !dataUrl;

  if (loading) return <div className="dv-loading"><span>Loading spreadsheet...</span></div>;

  return (
    <div className="dv-spreadsheet-info">
      <Table2 size={48} strokeWidth={1} />
      <h3>Spreadsheet File</h3>
      <p>{content}</p>
      <p className="dv-hint">Ingest this file into the Knowledge Base for full text extraction and search.</p>
    </div>
  );
}
