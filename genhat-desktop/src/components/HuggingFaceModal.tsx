import React, { useState, useEffect } from "react";
import { listen } from "@tauri-apps/api/event";
import { X, Search, Download, Loader2 } from "lucide-react";
import { Api, type HFModel, type HFRepoFile, type DeviceSpecs, type ModelCompatibility, type DocumentedRequirements } from "../api";
import type { ImportModelProfile } from "../types";

interface HuggingFaceModalProps {
  isOpen: boolean;
  onClose: () => void;
  onModelImported?: () => void;
}

const CATEGORIES: { label: string; folder: string }[] = [
  { label: "LLM", folder: "LLM" },
  { label: "Vision", folder: "LiquidAI-VLM" },
  { label: "Embedding", folder: "bge-1.5-embed" },
  { label: "TTS", folder: "kittenTTS" },
  { label: "STT", folder: "parakeet" },
];

/** Detect quantization level from GGUF filename */
function detectQuantization(filename: string): string | null {
  const match = filename.match(/-(q\d+_[km]_[ms]|q\d+_\d+|q\d+_k|f16|f32)\.gguf$/i);
  return match ? match[1].toLowerCase() : null;
}

/** Get quantization-specific RAM multiplier for more accurate estimation */
function getQuantizationInfo(quant: string | null): { multiplier: number; name: string } {
  if (!quant) return { multiplier: 1.30, name: 'Unknown' };
  
  const quantInfo: Record<string, { multiplier: number; name: string }> = {
    'q2_k': { multiplier: 1.15, name: 'Q2_K (Very compressed)' },
    'q3_k_s': { multiplier: 1.20, name: 'Q3_K_S' },
    'q3_k_m': { multiplier: 1.25, name: 'Q3_K_M' },
    'q3_k_l': { multiplier: 1.25, name: 'Q3_K_L' },
    'q4_0': { multiplier: 1.30, name: 'Q4_0' },
    'q4_1': { multiplier: 1.30, name: 'Q4_1' },
    'q4_k_s': { multiplier: 1.28, name: 'Q4_K_S' },
    'q4_k_m': { multiplier: 1.32, name: 'Q4_K_M (Common)' },
    'q5_0': { multiplier: 1.35, name: 'Q5_0' },
    'q5_1': { multiplier: 1.35, name: 'Q5_1' },
    'q5_k_s': { multiplier: 1.38, name: 'Q5_K_S' },
    'q5_k_m': { multiplier: 1.42, name: 'Q5_K_M' },
    'q6_k': { multiplier: 1.48, name: 'Q6_K' },
    'q8_0': { multiplier: 1.55, name: 'Q8_0 (High quality)' },
    'f16': { multiplier: 1.60, name: 'F16 (Half precision)' },
    'f32': { multiplier: 2.00, name: 'F32 (Full precision)' },
  };
  
  return quantInfo[quant] || { multiplier: 1.30, name: quant.toUpperCase() };
}

/** Compatibility badge component */
const CompatibilityBadge: React.FC<{ compatibility: ModelCompatibility; onClick?: () => void }> = ({ compatibility, onClick }) => {
  const colors = {
    efficient: "bg-green-500/20 text-green-400 border-green-500/30 hover:bg-green-500/30",
    satisfies: "bg-yellow-500/20 text-yellow-400 border-yellow-500/30 hover:bg-yellow-500/30",
    notrecommended: "bg-red-500/20 text-red-400 border-red-500/30 hover:bg-red-500/30",
    unknown: "bg-gray-500/20 text-gray-400 border-gray-500/30 hover:bg-gray-500/30",
  };
  const labels = {
    efficient: "✓ Efficient",
    satisfies: "⚠ Satisfies",
    notrecommended: "✗ Not Recommended",
    unknown: "? Unknown",
  };
  
  return (
    <button
      onClick={onClick}
      className={`px-2 py-0.5 text-xs font-medium rounded border transition-colors cursor-pointer ${colors[compatibility.rating]}`}
      title="Click for detailed compatibility analysis"
    >
      {labels[compatibility.rating]}
    </button>
  );
};

/** Detailed compatibility modal */
const CompatibilityDetailModal: React.FC<{
  isOpen: boolean;
  onClose: () => void;
  compatibility: ModelCompatibility;
  modelName: string;
  deviceSpecs: DeviceSpecs;
  actualFileSizeBytes: number;
  documentedReqs?: DocumentedRequirements | null;
}> = ({ isOpen, onClose, compatibility, modelName, deviceSpecs, actualFileSizeBytes, documentedReqs }) => {
  if (!isOpen) return null;

  const ratingColors = {
    efficient: "text-green-400",
    satisfies: "text-yellow-400",
    notrecommended: "text-red-400",
    unknown: "text-gray-400",
  };

  const ratingLabels = {
    efficient: "Efficient",
    satisfies: "Satisfies",
    notrecommended: "Not Recommended",
    unknown: "Unknown",
  };

  const actualFileSizeGB = actualFileSizeBytes / (1024 * 1024 * 1024);
  const quant = detectQuantization(modelName);
  const quantInfo = getQuantizationInfo(quant);

  return (
    <div className="fixed inset-0 z-[200] bg-black/70 backdrop-blur-sm flex items-center justify-center p-4" onClick={onClose}>
      <div className="bg-void-900 border border-glass-border rounded-xl w-full max-w-2xl shadow-2xl overflow-hidden" onClick={e => e.stopPropagation()}>
        {/* Header */}
        <div className="flex items-center justify-between p-4 border-b border-glass-border bg-void-800">
          <h3 className="text-lg font-semibold text-txt">Compatibility Analysis</h3>
          <button onClick={onClose} className="text-txt-secondary hover:text-txt transition-colors">
            <X size={20} />
          </button>
        </div>

        {/* Content */}
        <div className="p-6 space-y-6 max-h-[70vh] overflow-y-auto">
          {/* Model Info */}
          <div>
            <h4 className="text-sm font-medium text-txt-secondary mb-2">Model</h4>
            <p className="text-txt font-medium break-all">{modelName}</p>
            <div className="flex flex-col gap-1 mt-2 text-xs">
              <div className="flex justify-between">
                <span className="text-txt-muted">File Size:</span>
                <span className="text-txt">{actualFileSizeGB.toFixed(2)} GB</span>
              </div>
              {quant && (
                <div className="flex justify-between">
                  <span className="text-txt-muted">Quantization:</span>
                  <span className="text-txt">{quantInfo.name}</span>
                </div>
              )}
              <div className="flex justify-between">
                <span className="text-txt-muted">RAM Estimate Method:</span>
                <span className="text-txt">{documentedReqs?.source === 'documented' ? '📄 Documented' : (quant ? 'Quant-aware' : 'File-size based')}</span>
              </div>
            </div>
            <div className="mt-2 p-2 bg-void-800/50 border border-glass-border rounded text-xs text-txt-secondary">
              {documentedReqs?.source === 'documented' ? (
                <>
                  <span className="text-green-400">✓</span> RAM requirements found in model documentation. 
                  {documentedReqs.minRAM && ` Minimum: ${documentedReqs.minRAM}GB`}
                  {documentedReqs.recommendedRAM && `, Recommended: ${documentedReqs.recommendedRAM}GB`}
                </>
              ) : (
                <>
                  <span className="text-yellow-400">ℹ</span> RAM requirements are estimated {quant ? `using ${quantInfo.name} characteristics` : 'from file size'}. 
                  Actual usage may vary ±20% based on context length and batch size.
                </>
              )}
            </div>
          </div>

          {/* Overall Rating */}
          <div>
            <h4 className="text-sm font-medium text-txt-secondary mb-2">Overall Rating</h4>
            <div className="flex items-center gap-3">
              <span className={`text-2xl font-bold ${ratingColors[compatibility.rating]}`}>
                {ratingLabels[compatibility.rating]}
              </span>
              <span className="text-sm text-txt-secondary">{compatibility.reason}</span>
            </div>
          </div>

          {/* System Requirements Comparison */}
          <div>
            <h4 className="text-sm font-medium text-txt-secondary mb-3">System Requirements vs Your System</h4>
            <div className="space-y-3">
              {/* RAM */}
              <div className="bg-void border border-glass-border rounded-lg p-4">
                <div className="flex items-start justify-between mb-2">
                  <span className="text-sm font-medium text-txt">Memory (RAM)</span>
                  <span className="text-xs text-txt-secondary">
                    {compatibility.ram_usage_percent.toFixed(1)}% usage
                  </span>
                </div>
                <p className="text-sm text-txt-secondary mb-2">{compatibility.details.ram_check}</p>
                <div className="space-y-1 text-xs">
                  <div className="flex justify-between">
                    <span className="text-txt-muted">Required:</span>
                    <span className="text-txt">{(compatibility.estimated_memory_mb / 1024).toFixed(1)} GB</span>
                  </div>
                  <div className="flex justify-between">
                    <span className="text-txt-muted">Available:</span>
                    <span className="text-txt">{deviceSpecs.available_ram_gb.toFixed(1)} GB</span>
                  </div>
                  <div className="flex justify-between">
                    <span className="text-txt-muted">Total:</span>
                    <span className="text-txt">{deviceSpecs.total_ram_gb.toFixed(1)} GB</span>
                  </div>
                </div>
                {/* Progress bar */}
                <div className="mt-3 h-2 w-full bg-void-900 rounded-full overflow-hidden">
                  <div 
                    className={`h-full transition-all ${
                      compatibility.ram_usage_percent < 60 ? 'bg-green-500' :
                      compatibility.ram_usage_percent < 80 ? 'bg-yellow-500' : 'bg-red-500'
                    }`}
                    style={{ width: `${Math.min(compatibility.ram_usage_percent, 100)}%` }}
                  />
                </div>
              </div>

              {/* Disk Space */}
              <div className="bg-void border border-glass-border rounded-lg p-4">
                <div className="flex items-start justify-between mb-2">
                  <span className="text-sm font-medium text-txt">Disk Space</span>
                  <span className="text-xs text-txt-secondary">
                    {compatibility.disk_usage_percent.toFixed(1)}% usage
                  </span>
                </div>
                <p className="text-sm text-txt-secondary mb-2">{compatibility.details.disk_check}</p>
                <div className="space-y-1 text-xs">
                  <div className="flex justify-between">
                    <span className="text-txt-muted">Model File Size:</span>
                    <span className="text-txt">{actualFileSizeGB.toFixed(2)} GB</span>
                  </div>
                  <div className="flex justify-between">
                    <span className="text-txt-muted">Required (2× file):</span>
                    <span className="text-txt">{compatibility.required_disk_gb.toFixed(2)} GB</span>
                  </div>
                  <div className="flex justify-between">
                    <span className="text-txt-muted">Available:</span>
                    <span className="text-txt">{compatibility.available_disk_gb.toFixed(1)} GB</span>
                  </div>
                  <div className="flex justify-between">
                    <span className="text-txt-muted">Total Disk:</span>
                    <span className="text-txt">{deviceSpecs.total_disk_gb.toFixed(1)} GB</span>
                  </div>
                </div>
                {/* Progress bar */}
                <div className="mt-3 h-2 w-full bg-void-900 rounded-full overflow-hidden">
                  <div 
                    className={`h-full transition-all ${
                      compatibility.disk_space_sufficient ? 'bg-green-500' : 'bg-red-500'
                    }`}
                    style={{ width: `${Math.min(compatibility.disk_usage_percent, 100)}%` }}
                  />
                </div>
              </div>

              {/* CPU */}
              <div className="bg-void border border-glass-border rounded-lg p-4">
                <span className="text-sm font-medium text-txt block mb-2">Processor (CPU)</span>
                <p className="text-sm text-txt-secondary mb-2">{compatibility.details.cpu_check}</p>
                <div className="space-y-1 text-xs">
                  <div className="flex justify-between">
                    <span className="text-txt-muted">Cores:</span>
                    <span className="text-txt">{deviceSpecs.cpu_cores}</span>
                  </div>
                  <div className="flex justify-between">
                    <span className="text-txt-muted">Model:</span>
                    <span className="text-txt truncate max-w-[200px]" title={deviceSpecs.cpu_model}>
                      {deviceSpecs.cpu_model}
                    </span>
                  </div>
                </div>
              </div>
            </div>
          </div>

          {/* Performance Notes */}
          {compatibility.details.performance_notes.length > 0 && (
            <div>
              <h4 className="text-sm font-medium text-txt-secondary mb-2">Performance Notes</h4>
              <ul className="space-y-2">
                {compatibility.details.performance_notes.map((note, idx) => (
                  <li key={idx} className="flex items-start gap-2 text-sm text-txt-secondary">
                    <span className="text-yellow-400 mt-0.5">•</span>
                    <span>{note}</span>
                  </li>
                ))}
              </ul>
            </div>
          )}

          {/* Recommendation */}
          <div className="bg-void-800 border border-glass-border rounded-lg p-4">
            <h4 className="text-sm font-medium text-txt mb-2">Recommendation</h4>
            <p className="text-sm text-txt-secondary">
              {compatibility.rating === "efficient" && 
                "Your system is well-suited for this model. You can expect smooth performance and quick response times."}
              {compatibility.rating === "satisfies" && 
                "Your system meets the minimum requirements. The model should work, but you may experience slower performance during inference."}
              {compatibility.rating === "notrecommended" && 
                "Your system may struggle with this model. Consider choosing a smaller model or upgrading your hardware for better performance."}
              {compatibility.rating === "unknown" && 
                "Unable to determine compatibility. Please verify your system specifications."}
            </p>
          </div>
        </div>

        {/* Footer */}
        <div className="flex justify-end gap-3 p-4 border-t border-glass-border bg-void-800">
          <button
            onClick={onClose}
            className="px-4 py-2 bg-void-900 hover:bg-void text-txt border border-glass-border rounded-lg transition-colors"
          >
            Close
          </button>
        </div>
      </div>
    </div>
  );
};

export default function HuggingFaceModal({ isOpen, onClose, onModelImported }: HuggingFaceModalProps) {
  const [query, setQuery] = useState("");
  const [isSearching, setIsSearching] = useState(false);
  const [results, setResults] = useState<HFModel[]>([]);
  
  const [selectedRepo, setSelectedRepo] = useState<string | null>(null);
  const [isFetchingFiles, setIsFetchingFiles] = useState(false);
  const [repoFiles, setRepoFiles] = useState<HFRepoFile[]>([]);
  
  const [selectedFolder, setSelectedFolder] = useState<string>("LLM");
  const [importProfile, setImportProfile] = useState<"none" | ImportModelProfile>("llm");
  const [mmprojFile, setMmprojFile] = useState("");
  const [actionError, setActionError] = useState<string | null>(null);
  
  const [downloads, setDownloads] = useState<Record<string, { progress: number; status: string }>>({});
  const [completedDownloads, setCompletedDownloads] = useState<string[]>([]);
  const [currentPage, setCurrentPage] = useState(1);
  const itemsPerPage = 5;

  // Device specs and compatibility
  const [deviceSpecs, setDeviceSpecs] = useState<DeviceSpecs | null>(null);
  const [fileCompatibility, setFileCompatibility] = useState<Record<string, ModelCompatibility>>({});
  const [detailModalOpen, setDetailModalOpen] = useState(false);
  const [selectedCompatibility, setSelectedCompatibility] = useState<{
    compatibility: ModelCompatibility;
    modelName: string;
    fileSizeBytes: number;
  } | null>(null);
  const [documentedRequirements, setDocumentedRequirements] = useState<DocumentedRequirements | null>(null);

  // Fetch device specs on mount
  useEffect(() => {
    if (isOpen && !deviceSpecs) {
      Api.getSystemSpecs().then(setDeviceSpecs).catch(console.error);
    }
  }, [isOpen, deviceSpecs]);

  // Check compatibility for each file when repo files change
  useEffect(() => {
    if (repoFiles.length === 0 || !deviceSpecs) return;
    
    let isMounted = true;
    const checkCompatibility = async () => {
      const results: Record<string, ModelCompatibility> = {};
      for (const file of repoFiles) {
        const filename = file.file_name || file.path.split('/').pop() || file.path;
        const fileSizeMb = Math.round(file.size / (1024 * 1024));
        
        // Detect quantization for more accurate estimation
        const quant = detectQuantization(filename);
        const quantInfo = getQuantizationInfo(quant);
        
        // Prefer documented requirements over estimation
        let estimatedRAMMB: number;
        
        if (documentedRequirements?.source === 'documented' && documentedRequirements.minRAM) {
          // Use documented RAM requirement
          estimatedRAMMB = Math.round(documentedRequirements.minRAM * 1024);
        } else {
          // Calculate quant-aware RAM estimate
          const fileSizeGB = file.size / (1024 * 1024 * 1024);
          const estimatedRAMGB = (fileSizeGB * quantInfo.multiplier) + 0.5;
          estimatedRAMMB = Math.round(estimatedRAMGB * 1024);
        }
        
        try {
          const compat = await Api.checkCompatibility(fileSizeMb, estimatedRAMMB, quant || undefined);
          if (isMounted) {
            results[file.oid] = compat;
          }
        } catch (e) {
          console.error("Compatibility check failed:", e);
        }
      }
      if (isMounted) {
        setFileCompatibility(results);
      }
    };
    
    checkCompatibility();
    return () => { isMounted = false; };
  }, [repoFiles, deviceSpecs, documentedRequirements]);

  useEffect(() => {
    let unlisten: (() => void) | null = null;
    if (isOpen) {
      listen<{ model_id: string; progress: number; status: string }>(
        "model-download-progress",
        (e) => {
          setDownloads((prev) => ({
            ...prev,
            [e.payload.model_id]: { progress: e.payload.progress, status: e.payload.status },
          }));
          if (e.payload.progress >= 100 && e.payload.status === "Complete") {
            setCompletedDownloads(prev => [...prev, e.payload.model_id]);
            setTimeout(() => {
              setDownloads((prev) => {
                const newD = { ...prev };
                delete newD[e.payload.model_id];
                return newD;
              });
            }, 3000);
          }
        }
      ).then((fn) => {
        unlisten = fn;
      });
    }
    return () => {
      unlisten?.();
    };
  }, [isOpen]);

  const handleSearch = async (e: React.FormEvent) => {
    e.preventDefault();
    if (!query.trim()) return;
    
    setIsSearching(true);
    setResults([]);
    setSelectedRepo(null);
    setRepoFiles([]);
    setCurrentPage(1);
    try {
      const models = await Api.searchHuggingFace(query.trim());
      setResults(models);
    } catch (err) {
      console.error(err);
    } finally {
      setIsSearching(false);
    }
  };

  const handleSelectRepo = async (repoId: string) => {
    setSelectedRepo(repoId);
    setIsFetchingFiles(true);
    setRepoFiles([]);
    setCurrentPage(1);
    setDocumentedRequirements(null);
    
    try {
      // Fetch files and documentation in parallel
      const [files, docReqs] = await Promise.all([
        Api.getHuggingFaceRepoFiles(repoId),
        Api.fetchModelDocumentation(repoId)
      ]);
      
      const ggufs = files.filter(f => f.path.endsWith(".gguf"));
      setRepoFiles(ggufs);
      setDocumentedRequirements(docReqs);
      
      const mmproj = ggufs
        .map((f) => f.file_name || f.path.split("/").pop() || f.path)
        .find((name) => name.toLowerCase().includes("mmproj"));
      setMmprojFile(mmproj || "");
    } catch (err) {
      console.error(err);
    } finally {
      setIsFetchingFiles(false);
    }
  };

  // Re-check completed downloads whenever repo files or the target folder changes
  useEffect(() => {
    if (repoFiles.length === 0) return;
    
    let isMounted = true;
    const checkExistingFiles = async () => {
      const checks = await Promise.all(
        repoFiles.map(async (file) => {
          const filename = file.file_name || file.path.split('/').pop() || file.path;
          try {
            const exists = await Api.checkCustomFileExists(selectedFolder, filename);
            return exists ? `${selectedFolder}/${filename}` : null;
          } catch (e) {
            return null;
          }
        })
      );
      
      if (!isMounted) return;
      
      const existingPaths = checks.filter(Boolean) as string[];
      setCompletedDownloads((prev) => {
        // Keep previously completed tracked (from active session), add newly discovered
        const combined = new Set([...prev, ...existingPaths]);
        return Array.from(combined);
      });
    };
    
    checkExistingFiles();
    
    return () => { isMounted = false; };
  }, [repoFiles, selectedFolder]);

  const handleDownload = async (file: HFRepoFile) => {
    const url = `https://huggingface.co/${selectedRepo}/resolve/main/${file.path}`;
    setActionError(null);
    try {
      const filename = file.file_name || file.path.split('/').pop() || file.path;
      await Api.downloadCustomFile(url, selectedFolder, filename);

      if (importProfile === "none") {
        return;
      }

      if (importProfile === "vlm") {
        const companion = mmprojFile.trim();
        if (!companion) {
          throw new Error("VLM import requires an mmproj companion file name.");
        }

        if (companion === filename) {
          throw new Error("mmproj companion file must be different from the model file.");
        }

        const companionExists = await Api.checkCustomFileExists(selectedFolder, companion);
        if (!companionExists) {
          const companionInRepo = repoFiles.find((repoFile) => {
            const candidate = repoFile.file_name || repoFile.path.split("/").pop() || repoFile.path;
            return candidate === companion;
          });

          if (!companionInRepo) {
            throw new Error(`mmproj companion '${companion}' was not found locally or in this repo.`);
          }

          const companionUrl = `https://huggingface.co/${selectedRepo}/resolve/main/${companionInRepo.path}`;
          await Api.downloadCustomFile(companionUrl, selectedFolder, companion);
        }
      }

      if (importProfile === "llm" || importProfile === "vlm") {
        await Api.importDownloadedModel({
          folder: selectedFolder,
          filename,
          profile: importProfile,
          mmproj_file:
            importProfile === "vlm" && mmprojFile.trim()
              ? `${selectedFolder}/${mmprojFile.trim()}`
              : undefined,
          engine_adapter: "llama_cpp",
        });
        onModelImported?.();
      }
    } catch(err) {
      console.error(err);
      setActionError(err instanceof Error ? err.message : String(err));
    }
  };

  if (!isOpen) return null;

  return (
    <div className="fixed inset-0 z-100 bg-black/60 backdrop-blur-sm flex items-center justify-center p-4" onClick={onClose}>
      <div className="bg-void-900 border border-glass-border rounded-xl w-full max-w-4xl h-[80vh] flex flex-col shadow-2xl overflow-hidden" onClick={e => e.stopPropagation()}>
        <div className="flex items-center justify-between p-4 border-b border-glass-border bg-void-800">
          <h2 className="text-xl font-semibold text-txt flex items-center gap-2">
            <span role="img" aria-label="Hugging Face">🤗</span> Hugging Face Hub Search
          </h2>
          <div className="flex items-center gap-4">
            {deviceSpecs && (
              <div className="flex gap-3 text-xs">
                <div className="text-txt-secondary bg-void-900 px-3 py-1.5 rounded-lg border border-glass-border">
                  <span className="text-txt-muted">RAM:</span>{" "}
                  <span className="text-txt">{deviceSpecs.available_ram_gb.toFixed(1)}</span>
                  <span className="text-txt-muted">/{deviceSpecs.total_ram_gb.toFixed(1)} GB</span>
                </div>
                <div className="text-txt-secondary bg-void-900 px-3 py-1.5 rounded-lg border border-glass-border">
                  <span className="text-txt-muted">Disk:</span>{" "}
                  <span className="text-txt">{deviceSpecs.available_disk_gb.toFixed(1)}</span>
                  <span className="text-txt-muted">/{deviceSpecs.total_disk_gb.toFixed(1)} GB</span>
                </div>
                <div className="text-txt-secondary bg-void-900 px-3 py-1.5 rounded-lg border border-glass-border">
                  <span className="text-txt-muted">CPU:</span>{" "}
                  <span className="text-txt">{deviceSpecs.cpu_cores} cores</span>
                </div>
              </div>
            )}
            <button onClick={onClose} className="text-txt-secondary hover:text-txt transition-colors">
              <X size={24} />
            </button>
          </div>
        </div>
        
        <div className="p-4 border-b border-glass-border flex gap-4 bg-void-800">
          <form onSubmit={handleSearch} className="flex-1 flex gap-2">
            <input 
              type="text"
              className="flex-1 bg-void-900 border border-glass-border rounded-lg px-4 py-2 text-txt focus:outline-none focus:border-neon focus:ring-1 focus:ring-neon transition-all"
              placeholder="Search models (e.g. Llama-3-8B-Instruct-GGUF)"
              value={query}
              onChange={e => setQuery(e.target.value)}
            />
            <button 
              type="submit"
              disabled={isSearching}
              className="bg-neon text-void-900 px-4 py-2 rounded-lg font-medium flex items-center gap-2 hover:bg-neon-hover disabled:opacity-50 transition-colors"
            >
              {isSearching ? <Loader2 size={18} className="animate-spin" /> : <Search size={18} />}
              Search
            </button>
          </form>
        </div>

        <div className="flex-1 flex overflow-hidden">
          {/* Left panel: Search results */}
          <div className="w-1/2 border-r border-glass-border overflow-y-auto p-4 flex flex-col gap-2">
            {isSearching ? (
              <div className="flex justify-center py-8 text-txt-secondary"><Loader2 className="animate-spin" size={24}/></div>
            ) : results.length > 0 ? (
              results.map((m) => (
                <button 
                  key={m.id}
                  onClick={() => handleSelectRepo(m.id)}
                  className={`text-left p-3 rounded-lg border transition-all ${selectedRepo === m.id ? 'bg-void-800 border-neon text-neon' : 'bg-void border-glass-border text-txt-secondary hover:border-txt-secondary'}`}
                >
                  <div className="font-medium truncate">{m.id}</div>
                  <div className="text-xs opacity-70 mt-1 flex gap-3">
                    <span>❤️ {m.likes || 0}</span>
                    <span>⬇️ {m.downloads || 0}</span>
                  </div>
                </button>
              ))
            ) : (
              <div className="text-center text-txt-secondary mt-8 text-sm">No results to show.</div>
            )}
          </div>

          {/* Right panel: Repo files */}
          <div className="w-1/2 overflow-y-auto p-4 bg-void-800/30">
            {selectedRepo ? (
              <div className="flex flex-col gap-4">
                <h3 className="font-semibold text-txt wrap-break-word text-lg">{selectedRepo}</h3>
                
                <div className="flex flex-col gap-2">
                  <label className="text-sm font-medium text-txt-secondary">Category (Folder to save into):</label>
                  <select 
                    value={selectedFolder}
                    onChange={(e) => setSelectedFolder(e.target.value)}
                    className="bg-void-900 border border-glass-border rounded-lg px-3 py-2 text-txt focus:outline-none focus:border-neon"
                  >
                    {CATEGORIES.map(c => (
                      <option key={c.folder} value={c.folder}>{c.label} ({c.folder})</option>
                    ))}
                  </select>
                </div>

                <div className="flex flex-col gap-2">
                  <label className="text-sm font-medium text-txt-secondary">Import profile (required for model usage):</label>
                  <select
                    value={importProfile}
                    onChange={(e) => setImportProfile(e.target.value as "none" | ImportModelProfile)}
                    className="bg-void-900 border border-glass-border rounded-lg px-3 py-2 text-txt focus:outline-none focus:border-neon"
                  >
                    <option value="llm">LLM</option>
                    <option value="vlm">VLM</option>
                    <option value="none">Download only (do not import)</option>
                  </select>
                </div>

                {importProfile === "vlm" && (
                  <div className="flex flex-col gap-2">
                    <label className="text-sm font-medium text-txt-secondary">VLM mmproj file name:</label>
                    <input
                      value={mmprojFile}
                      onChange={(e) => setMmprojFile(e.target.value)}
                      placeholder="mmproj-....gguf"
                      className="bg-void-900 border border-glass-border rounded-lg px-3 py-2 text-txt focus:outline-none focus:border-neon"
                    />
                  </div>
                )}

                {actionError && (
                  <div className="text-sm text-red-400 bg-red-500/10 border border-red-500/30 rounded-lg px-3 py-2">
                    {actionError}
                  </div>
                )}

                <div className="mt-2">
                  <div className="text-txt-secondary flex items-center justify-between mb-2">
                    <h4 className="text-sm font-medium">Available .gguf Files</h4>
                  </div>
                  {isFetchingFiles ? (
                    <div className="flex justify-center py-4 text-txt-secondary"><Loader2 className="animate-spin" size={20}/></div>
                  ) : repoFiles.length > 0 ? (
                    <div className="flex flex-col gap-2">
                      {repoFiles.slice((currentPage - 1) * itemsPerPage, currentPage * itemsPerPage).map((file) => {
                        const filename = file.file_name || file.path.split('/').pop() || file.path;
                        const dlKey = `${selectedFolder}/${filename}`;
                        const dlState = downloads[dlKey] || downloads[filename];
                        const isDownloading = dlState !== undefined;
                        const isCompleted = completedDownloads.includes(dlKey) || completedDownloads.includes(filename);
                        const compat = fileCompatibility[file.oid];
                        
                        return (
                          <div key={file.oid} className="bg-void border border-glass-border rounded-lg p-3 flex flex-col gap-2">
                            <div className="flex justify-between items-start gap-2">
                              <span className="text-sm font-medium text-txt break-all">{filename}</span>
                              <div className="flex items-center gap-2 shrink-0">
                                {compat && deviceSpecs && (
                                  <CompatibilityBadge 
                                    compatibility={compat} 
                                    onClick={() => {
                                      setSelectedCompatibility({ 
                                        compatibility: compat, 
                                        modelName: filename,
                                        fileSizeBytes: file.size 
                                      });
                                      setDetailModalOpen(true);
                                    }}
                                  />
                                )}
                                <span className="text-xs text-txt-secondary whitespace-nowrap">{(file.size / 1024 / 1024 / 1024).toFixed(2)} GB</span>
                              </div>
                            </div>
                            
                            {isDownloading ? (
                              <div className="flex flex-col gap-1 mt-1">
                                <div className="flex justify-between text-xs text-txt-secondary">
                                  <span>{dlState.status}</span>
                                  <span>{Math.round(dlState.progress)}%</span>
                                </div>
                                <div className="h-1.5 w-full bg-void-900 rounded-full overflow-hidden">
                                  <div 
                                    className="h-full bg-neon transition-all duration-300"
                                    style={{ width: `${dlState.progress}%` }}
                                  />
                                </div>
                              </div>
                            ) : isCompleted ? (
                              <button
                                disabled
                                className="mt-1 w-full bg-void-800 text-neon border border-neon/50 opacity-70 py-1.5 rounded text-sm font-medium flex items-center justify-center gap-2 cursor-not-allowed"
                              >
                                <Download size={16} />
                                Downloaded
                              </button>
                            ) : (
                              <button
                                onClick={() => handleDownload(file)}
                                className="mt-1 w-full bg-void-800 hover:bg-neon hover:text-void-900 text-neon border border-neon transition-colors py-1.5 rounded text-sm font-medium flex items-center justify-center gap-2"
                              >
                                <Download size={16} />
                                Download
                              </button>
                            )}
                          </div>
                        );
                      })}
                      {repoFiles.length > itemsPerPage && (
                        <div className="flex justify-between items-center mt-2 p-2 bg-void border border-glass-border rounded-lg">
                          <button
                            disabled={currentPage === 1}
                            onClick={() => setCurrentPage(p => p - 1)}
                            className="px-3 py-1 bg-void-800 text-txt-secondary hover:text-txt rounded text-sm transition-colors disabled:opacity-50"
                          >
                            Previous
                          </button>
                          <span className="text-xs text-txt-secondary">
                            Page {currentPage} of {Math.ceil(repoFiles.length / itemsPerPage)}
                          </span>
                          <button
                            disabled={currentPage === Math.ceil(repoFiles.length / itemsPerPage)}
                            onClick={() => setCurrentPage(p => p + 1)}
                            className="px-3 py-1 bg-void-800 text-txt-secondary hover:text-txt rounded text-sm transition-colors disabled:opacity-50"
                          >
                            Next
                          </button>
                        </div>
                      )}
                    </div>
                  ) : (
                    <div className="text-sm text-txt-secondary p-4 bg-void border border-glass-border rounded-lg text-center">
                      No .gguf files found in this repository.
                    </div>
                  )}
                </div>
              </div>
            ) : (
              <div className="h-full flex items-center justify-center text-txt-secondary text-sm">
                Select a model to view files
              </div>
            )}
          </div>
        </div>
      </div>

      {/* Compatibility Detail Modal */}
      {selectedCompatibility && deviceSpecs && (
        <CompatibilityDetailModal
          isOpen={detailModalOpen}
          onClose={() => setDetailModalOpen(false)}
          compatibility={selectedCompatibility.compatibility}
          modelName={selectedCompatibility.modelName}
          deviceSpecs={deviceSpecs}
          actualFileSizeBytes={selectedCompatibility.fileSizeBytes}
          documentedReqs={documentedRequirements}
        />
      )}
    </div>
  );
}