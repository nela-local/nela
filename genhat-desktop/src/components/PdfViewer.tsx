import { useState, useEffect, useRef, useCallback } from "react";
import * as pdfjsLib from "pdfjs-dist";
import {
  ZoomIn, ZoomOut, ChevronLeft, ChevronRight, X, Minimize2,
} from "lucide-react";

// Set up the worker
pdfjsLib.GlobalWorkerOptions.workerSrc = new URL(
  "pdfjs-dist/build/pdf.worker.min.mjs",
  import.meta.url,
).toString();

interface PdfViewerProps {
  /** Base64 data URL of the PDF */
  pdfData: string;
  /** Document title for the header */
  title: string;
  /** Called when the user closes the viewer */
  onClose: () => void;
}

export default function PdfViewer({ pdfData, title, onClose }: PdfViewerProps) {
  const containerRef = useRef<HTMLDivElement>(null);
  const [pdfDoc, setPdfDoc] = useState<pdfjsLib.PDFDocumentProxy | null>(null);
  const [currentPage, setCurrentPage] = useState(1);
  const [totalPages, setTotalPages] = useState(0);
  const [scale, setScale] = useState(1.2);
  const [loading, setLoading] = useState(true);
  const [error, setError] = useState<string | null>(null);
  const [renderedPages, setRenderedPages] = useState<Set<number>>(new Set());
  const canvasRefs = useRef<Map<number, HTMLCanvasElement>>(new Map());
  const renderTasksRef = useRef<Map<number, pdfjsLib.RenderTask>>(new Map());

  // Load the PDF document
  useEffect(() => {
    let cancelled = false;

    async function loadPdf() {
      try {
        setLoading(true);
        setError(null);

        // Extract base64 content from data URL
        const base64 = pdfData.split(",")[1];
        const binaryString = atob(base64);
        const bytes = new Uint8Array(binaryString.length);
        for (let i = 0; i < binaryString.length; i++) {
          bytes[i] = binaryString.charCodeAt(i);
        }

        const doc = await pdfjsLib.getDocument({ data: bytes }).promise;
        if (!cancelled) {
          setPdfDoc(doc);
          setTotalPages(doc.numPages);
          setCurrentPage(1);
          setRenderedPages(new Set());
          setLoading(false);
        }
      } catch (e) {
        if (!cancelled) {
          setError(e instanceof Error ? e.message : "Failed to load PDF");
          setLoading(false);
        }
      }
    }

    loadPdf();
    return () => {
      cancelled = true;
    };
  }, [pdfData]);

  // Render a single page to its canvas
  const renderPage = useCallback(
    async (pageNum: number) => {
      if (!pdfDoc) return;

      const canvas = canvasRefs.current.get(pageNum);
      if (!canvas) return;

      // Cancel any in-progress render for this page
      const existingTask = renderTasksRef.current.get(pageNum);
      if (existingTask) {
        existingTask.cancel();
        renderTasksRef.current.delete(pageNum);
      }

      try {
        const page = await pdfDoc.getPage(pageNum);
        const dpr = window.devicePixelRatio || 1;
        const viewport = page.getViewport({ scale: scale * dpr });

        // Set canvas buffer size to full resolution
        canvas.width = Math.floor(viewport.width);
        canvas.height = Math.floor(viewport.height);

        // Set CSS display size to logical size
        canvas.style.width = `${Math.floor(viewport.width / dpr)}px`;
        canvas.style.height = `${Math.floor(viewport.height / dpr)}px`;

        const renderTask = page.render({
          canvas,
          viewport,
        });
        renderTasksRef.current.set(pageNum, renderTask);

        await renderTask.promise;
        renderTasksRef.current.delete(pageNum);
        setRenderedPages((prev) => new Set(prev).add(pageNum));
      } catch (e: unknown) {
        if (e && typeof e === "object" && "name" in e && (e as { name: string }).name === "RenderingCancelledException") {
          // Expected when re-rendering, ignore
        } else {
          console.error(`Error rendering page ${pageNum}:`, e);
        }
      }
    },
    [pdfDoc, scale],
  );

  // Render all pages when doc/scale changes
  useEffect(() => {
    if (!pdfDoc) return;

    // eslint-disable-next-line react-hooks/set-state-in-effect -- intentional: reset when doc/scale changes
    setRenderedPages(new Set());

    // Render all pages
    for (let i = 1; i <= pdfDoc.numPages; i++) {
      renderPage(i);
    }

    const tasks = renderTasksRef.current;
    return () => {
      // Cancel all in-progress renders
      tasks.forEach((task) => task.cancel());
      tasks.clear();
    };
  }, [pdfDoc, scale, renderPage]);

  // Track current page from scroll position
  const handleScroll = useCallback(() => {
    const container = containerRef.current;
    if (!container || totalPages === 0) return;

    const scrollTop = container.scrollTop;
    const containerHeight = container.clientHeight;
    const scrollCenter = scrollTop + containerHeight / 2;

    // Find which page canvas is at the center of the viewport
    let closestPage = 1;
    let closestDist = Infinity;

    canvasRefs.current.forEach((canvas, pageNum) => {
      const rect = canvas.getBoundingClientRect();
      const containerRect = container.getBoundingClientRect();
      const canvasCenter =
        rect.top - containerRect.top + container.scrollTop + rect.height / 2;
      const dist = Math.abs(canvasCenter - scrollCenter);
      if (dist < closestDist) {
        closestDist = dist;
        closestPage = pageNum;
      }
    });

    setCurrentPage(closestPage);
  }, [totalPages]);

  // Zoom handlers
  const zoomIn = () => setScale((s) => Math.min(s + 0.25, 4.0));
  const zoomOut = () => setScale((s) => Math.max(s - 0.25, 0.5));
  const zoomFit = () => {
    if (!containerRef.current || !pdfDoc) return;
    // Reset to default fit width
    setScale(1.2);
  };

  // Page navigation
  const goToPage = useCallback((page: number) => {
    const clamped = Math.max(1, Math.min(page, totalPages));
    setCurrentPage(clamped);
    const canvas = canvasRefs.current.get(clamped);
    if (canvas) {
      canvas.scrollIntoView({ behavior: "smooth", block: "start" });
    }
  }, [totalPages]);

  // Keyboard shortcuts
  useEffect(() => {
    const handleKey = (e: KeyboardEvent) => {
      if (e.key === "Escape") onClose();
      else if (e.key === "+" || e.key === "=") { e.preventDefault(); zoomIn(); }
      else if (e.key === "-") { e.preventDefault(); zoomOut(); }
      else if (e.key === "ArrowLeft" || e.key === "PageUp") goToPage(currentPage - 1);
      else if (e.key === "ArrowRight" || e.key === "PageDown") goToPage(currentPage + 1);
      else if (e.key === "Home") goToPage(1);
      else if (e.key === "End") goToPage(totalPages);
    };
    window.addEventListener("keydown", handleKey);
    return () => window.removeEventListener("keydown", handleKey);
  }, [currentPage, totalPages, onClose, goToPage]);

  return (
    <div className="pdf-viewer">
      {/* Toolbar */}
      <div className="pdf-toolbar">
        <div className="pdf-toolbar-left">
          <span className="pdf-title" title={title}>
            {title}
          </span>
        </div>

        <div className="pdf-toolbar-center">
          {/* Page navigation */}
          <button
            className="glass-btn pdf-tool-btn"
            onClick={() => goToPage(currentPage - 1)}
            disabled={currentPage <= 1}
            title="Previous page"
          >
            <ChevronLeft size={16} />
          </button>

          <span className="pdf-page-info">
            <input
              type="number"
              className="pdf-page-input"
              value={currentPage}
              min={1}
              max={totalPages}
              onChange={(e) => {
                const val = parseInt(e.target.value, 10);
                if (!isNaN(val)) goToPage(val);
              }}
            />
            <span className="pdf-page-sep">/</span>
            <span className="pdf-page-total">{totalPages}</span>
          </span>

          <button
            className="glass-btn pdf-tool-btn"
            onClick={() => goToPage(currentPage + 1)}
            disabled={currentPage >= totalPages}
            title="Next page"
          >
            <ChevronRight size={16} />
          </button>

          <div className="pdf-toolbar-divider" />

          {/* Zoom controls */}
          <button className="pdf-tool-btn" onClick={zoomOut} title="Zoom out (-)">
            <ZoomOut size={16} />
          </button>

          <span className="pdf-zoom-level">{Math.round(scale * 100)}%</span>

          <button className="pdf-tool-btn" onClick={zoomIn} title="Zoom in (+)">
            <ZoomIn size={16} />
          </button>

          <button className="pdf-tool-btn" onClick={zoomFit} title="Reset zoom">
            <Minimize2 size={14} />
          </button>
        </div>

        <div className="pdf-toolbar-right">
          <button className="pdf-tool-btn pdf-close-btn" onClick={onClose} title="Close (Esc)">
            <X size={16} />
          </button>
        </div>
      </div>

      {/* Content area */}
      <div
        className="pdf-content"
        ref={containerRef}
        onScroll={handleScroll}
      >
        {loading && (
          <div className="pdf-loading">
            <div className="pdf-spinner" />
            <span>Loading PDF...</span>
          </div>
        )}

        {error && (
          <div className="pdf-error">
            <span>Failed to load PDF: {error}</span>
          </div>
        )}

        {!loading && !error && pdfDoc && (
          <div className="pdf-pages">
            {Array.from({ length: totalPages }, (_, i) => i + 1).map(
              (pageNum) => (
                <div key={pageNum} className="pdf-page-wrapper">
                  <canvas
                    ref={(el) => {
                      if (el) canvasRefs.current.set(pageNum, el);
                      else canvasRefs.current.delete(pageNum);
                    }}
                    className="pdf-page-canvas"
                  />
                  {!renderedPages.has(pageNum) && (
                    <div className="pdf-page-loading">Rendering page {pageNum}...</div>
                  )}
                </div>
              ),
            )}
          </div>
        )}
      </div>
    </div>
  );
}
