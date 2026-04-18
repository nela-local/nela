import type { RegisteredModel } from "../types";

export const formatModelSizeLabel = (memoryMb: number | null | undefined): string => {
  if (typeof memoryMb !== "number" || !Number.isFinite(memoryMb) || memoryMb <= 0) {
    return "Unknown size";
  }
  const mb = memoryMb;
  if (mb >= 1024) return `${(mb / 1024).toFixed(2)} GB`;
  return `${Math.round(mb)} MB`;
};

export const formatTotalSizeLabel = (totalMb: number): string => {
  if (totalMb >= 1024) return `${(totalMb / 1024).toFixed(2)} GB`;
  return `${Math.round(totalMb)} MB`;
};

export const formatDownloadSpeedLabel = (bytesPerSecond: number): string => {
  if (!Number.isFinite(bytesPerSecond) || bytesPerSecond <= 0) {
    return "0 KB/s";
  }

  if (bytesPerSecond >= 1024 * 1024) {
    return `${(bytesPerSecond / (1024 * 1024)).toFixed(2)} MB/s`;
  }
  return `${(bytesPerSecond / 1024).toFixed(1)} KB/s`;
};

export const normalizeModelRef = (raw: string): string =>
  raw.replace(/\\/g, "/").toLowerCase();

export const modelRefBasename = (raw: string): string => {
  const normalized = normalizeModelRef(raw);
  const parts = normalized.split("/");
  return parts[parts.length - 1] ?? normalized;
};

export const findRegisteredModelByIdentifier = (
  models: RegisteredModel[],
  identifier: string | null | undefined
): RegisteredModel | undefined => {
  if (!identifier) return undefined;

  const exact = models.find((model) => model.id === identifier);
  if (exact) return exact;

  const normalizedIdentifier = normalizeModelRef(identifier);
  const identifierBase = modelRefBasename(identifier);

  return models.find((model) => {
    if (!model.model_file) return false;
    const normalizedFile = normalizeModelRef(model.model_file);
    return (
      normalizedFile === normalizedIdentifier ||
      modelRefBasename(normalizedFile) === identifierBase
    );
  });
};
