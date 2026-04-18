import type { MindMapGraph, MindMapNode } from "../types";

/** Turn raw page_info metadata (e.g. "page:3", "slide:2") into a readable label. */
export function formatPageLabel(meta?: string): string {
  if (!meta) return "";
  if (meta.startsWith("page:")) return `Page ${meta.split(":")[1]}`;
  if (meta.startsWith("slide:")) return `Slide ${meta.split(":")[1]}`;
  if (meta.startsWith("paragraph:")) return `Paragraph ${meta.split(":")[1]}`;
  return meta;
}

export function extractTaskText(response: unknown): string {
  if (typeof response === "string") return response;
  if (response && typeof response === "object") {
    const record = response as Record<string, unknown>;
    if (typeof record.Text === "string") return record.Text;
    if (typeof record.Error === "string") throw new Error(record.Error);
  }
  return JSON.stringify(response ?? "");
}

function extractJsonObject(raw: string): string | null {
  const trimmed = raw.trim();
  if (trimmed.startsWith("{")) return trimmed;

  const fenced = trimmed.match(/```(?:json)?\s*([\s\S]*?)\s*```/i);
  if (fenced?.[1]) return fenced[1].trim();

  const start = trimmed.indexOf("{");
  const end = trimmed.lastIndexOf("}");
  if (start >= 0 && end > start) return trimmed.slice(start, end + 1);

  return null;
}

function normalizeMindMapNode(input: unknown): MindMapNode {
  const node = (input ?? {}) as Record<string, unknown>;
  const label = typeof node.label === "string" && node.label.trim().length > 0
    ? node.label.trim()
    : "Untitled";

  const childrenRaw = Array.isArray(node.children) ? node.children : [];
  return {
    id: crypto.randomUUID(),
    label,
    children: childrenRaw.map((child) => normalizeMindMapNode(child)),
  };
}

export function parseMindMapGraph(
  raw: string,
  query: string,
  generatedFrom: "documents" | "model",
  sourceCount: number
): MindMapGraph {
  const jsonText = extractJsonObject(raw);
  if (!jsonText) {
    throw new Error("Model did not return JSON mindmap output.");
  }

  const parsed = JSON.parse(jsonText) as Record<string, unknown>;
  const title = typeof parsed.title === "string" && parsed.title.trim().length > 0
    ? parsed.title.trim()
    : query;
  const rootRaw = parsed.root as unknown;
  const root = normalizeMindMapNode(rootRaw ?? { label: title, children: [] });

  return {
    id: crypto.randomUUID(),
    title,
    query,
    generatedFrom,
    sourceCount,
    root,
    createdAt: Date.now(),
  };
}

function normalizeMindMapGraph(raw: unknown): MindMapGraph | null {
  if (!raw || typeof raw !== "object") return null;
  const graph = raw as Partial<MindMapGraph>;
  if (!graph.root || typeof graph.root !== "object") return null;

  return {
    id: typeof graph.id === "string" && graph.id ? graph.id : crypto.randomUUID(),
    title: typeof graph.title === "string" && graph.title ? graph.title : "Mindmap",
    query: typeof graph.query === "string" ? graph.query : "",
    generatedFrom: graph.generatedFrom === "documents" ? "documents" : "model",
    sourceCount: typeof graph.sourceCount === "number" ? graph.sourceCount : 0,
    root: normalizeMindMapNode(graph.root),
    createdAt: typeof graph.createdAt === "number" ? graph.createdAt : Date.now(),
  };
}

export function normalizeMindmapsStore(raw: unknown): Record<string, MindMapGraph[]> {
  if (!raw || typeof raw !== "object") return {};
  const store = raw as Record<string, unknown>;
  const normalized: Record<string, MindMapGraph[]> = {};

  Object.entries(store).forEach(([sessionId, value]) => {
    if (Array.isArray(value)) {
      const items = value
        .map((entry) => normalizeMindMapGraph(entry))
        .filter((entry): entry is MindMapGraph => !!entry);
      if (items.length > 0) normalized[sessionId] = items;
      return;
    }

    const single = normalizeMindMapGraph(value);
    if (single) normalized[sessionId] = [single];
  });

  return normalized;
}
