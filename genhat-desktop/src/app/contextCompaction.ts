import type {
  ChatContextCompactionResult,
  ChatContextMessage,
  ChatMessage,
  MediaAsset,
} from "../types";

export const CONTEXT_COMPACTION_THRESHOLD = 0.9;
export const CONTEXT_COMPACTION_KEEP_RECENT = 8;

export function toContextMessages(messages: ChatMessage[]): ChatContextMessage[] {
  return messages.map(({ role, content }) => ({ role, content }));
}

export function resolveReservedOutputTokens(maxTokens: number | undefined): number {
  const fallback = 2048;
  const safe = Number.isFinite(maxTokens) ? Math.round(maxTokens as number) : fallback;
  return Math.max(128, Math.min(8192, safe));
}

export function applyCompactionResultToSession(
  originalMessages: ChatMessage[],
  originalMediaAssets: Record<number, MediaAsset[]>,
  result: ChatContextCompactionResult
): { messages: ChatMessage[]; mediaAssets: Record<number, MediaAsset[]> } {
  const keptIndices = result.keptIndices
    .filter((idx) => Number.isInteger(idx) && idx >= 0 && idx < originalMessages.length)
    .sort((a, b) => a - b);

  const rebuiltMessages = keptIndices.map((idx) => originalMessages[idx]);
  const rebuiltMediaAssets: Record<number, MediaAsset[]> = {};

  keptIndices.forEach((originalIdx, nextIdx) => {
    if (originalMediaAssets[originalIdx]) {
      rebuiltMediaAssets[nextIdx] = originalMediaAssets[originalIdx];
    }
  });

  if (typeof result.summaryInsertIndex === "number") {
    const insertAt = Math.max(0, Math.min(result.summaryInsertIndex, rebuiltMessages.length));
    const summaryPayload = result.messages[insertAt] ?? {
      role: "system" as const,
      content: "Conversation summary (auto-compacted):\nPrevious context was compacted.",
    };

    rebuiltMessages.splice(insertAt, 0, {
      role: summaryPayload.role,
      content: summaryPayload.content,
    });

    const shifted: Record<number, MediaAsset[]> = {};
    Object.entries(rebuiltMediaAssets).forEach(([idxStr, assets]) => {
      const idx = Number(idxStr);
      shifted[idx >= insertAt ? idx + 1 : idx] = assets;
    });

    return {
      messages: rebuiltMessages,
      mediaAssets: shifted,
    };
  }

  return {
    messages: rebuiltMessages,
    mediaAssets: rebuiltMediaAssets,
  };
}
