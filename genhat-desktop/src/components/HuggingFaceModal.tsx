import React, { useState, useEffect } from "react";
import { listen } from "@tauri-apps/api/event";
import { X, Search, Download, Loader2, CheckCircle, AlertTriangle, XCircle, Ban, HelpCircle, Cpu, HardDrive, MemoryStick, Zap, Info, Lightbulb } from "lucide-react";
import { Api, type HFModel, type HFRepoFile, type DeviceSpecs, type ModelCompatibility, type DocumentedRequirements } from "../api";
import type { ImportModelProfile } from "../types";
import "./HuggingFaceModal.css";

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

/** Rating icon component */
const RatingIcon: React.FC<{ rating: string; size?: number }> = ({ rating, size = 12 }) => {
  switch (rating) {
    case 'efficient':
      return <CheckCircle size={size} />;
    case 'usable':
      return <AlertTriangle size={size} />;
    case 'veryslow':
      return <AlertTriangle size={size} />;
    case 'notrecommended':
      return <XCircle size={size} />;
    case 'wontrun':
      return <Ban size={size} />;
    default:
      return <HelpCircle size={size} />;
  }
};

/** Compatibility badge component - shows rating with icon */
const CompatibilityBadge: React.FC<{ compatibility: ModelCompatibility; onClick?: () => void }> = ({ compatibility, onClick }) => {
  const colors: Record<string, string> = {
    efficient: "bg-green-500/20 text-green-400 border-green-500/30 hover:bg-green-500/30",
    usable: "bg-yellow-500/20 text-yellow-400 border-yellow-500/30 hover:bg-yellow-500/30",
    veryslow: "bg-orange-500/20 text-orange-400 border-orange-500/30 hover:bg-orange-500/30",
    notrecommended: "bg-red-500/20 text-red-400 border-red-500/30 hover:bg-red-500/30",
    wontrun: "bg-red-700/20 text-red-500 border-red-700/30 hover:bg-red-700/30",
    unknown: "bg-gray-500/20 text-gray-400 border-gray-500/30 hover:bg-gray-500/30",
  };
  const labels: Record<string, string> = {
    efficient: "Efficient",
    usable: "Usable",
    veryslow: "Very Slow",
    notrecommended: "Not Recommended",
    wontrun: "Won't Run",
    unknown: "Unknown",
  };
  
  return (
    <button
      onClick={onClick}
      className={`px-2 py-0.5 text-xs font-medium rounded border transition-colors cursor-pointer flex items-center gap-1 ${colors[compatibility.rating] || colors.unknown}`}
      title="Click for detailed compatibility analysis"
    >
      <RatingIcon rating={compatibility.rating} size={11} />
      <span>{labels[compatibility.rating] || labels.unknown}</span>
    </button>
  );
};

/** Detailed compatibility modal - shows full calculation breakdown */
const CompatibilityDetailModal: React.FC<{
  isOpen: boolean;
  onClose: () => void;
  compatibility: ModelCompatibility;
  modelName: string;
  deviceSpecs: DeviceSpecs;
  actualFileSizeBytes: number;
  documentedReqs?: DocumentedRequirements | null;
}> = ({ isOpen, onClose, compatibility, modelName, deviceSpecs, actualFileSizeBytes, documentedReqs: _documentedReqs }) => {
  if (!isOpen) return null;

  const ratingColors: Record<string, string> = {
    efficient: "text-green-400",
    usable: "text-yellow-400",
    veryslow: "text-orange-400",
    notrecommended: "text-red-400",
    wontrun: "text-red-500",
    unknown: "text-gray-400",
  };

  const ratingLabels: Record<string, string> = {
    efficient: "Efficient",
    usable: "Usable",
    veryslow: "Very Slow",
    notrecommended: "Not Recommended",
    wontrun: "Won't Run",
    unknown: "Unknown",
  };

  const actualFileSizeGB = actualFileSizeBytes / (1024 * 1024 * 1024);
  const calc = compatibility.calculation;

  return (
    <div className="fixed inset-0 z-[200] bg-black/70 backdrop-blur-sm flex items-center justify-center p-4" onClick={onClose}>
      <div className="bg-void-900 border border-glass-border rounded-xl w-full max-w-2xl shadow-2xl overflow-hidden" onClick={e => e.stopPropagation()}>
        {/* Header */}
        <div className="flex items-center justify-between p-4 border-b border-glass-border bg-void-800">
          <h3 className="text-lg font-semibold text-txt flex items-center gap-2">
            <Info size={18} />
            Compatibility Analysis
          </h3>
          <button onClick={onClose} className="text-txt-secondary hover:text-txt transition-colors">
            <X size={20} />
          </button>
        </div>

        {/* Content */}
        <div className="hf-modal-scrollable compat-detail-scrollable p-6 space-y-6 max-h-[70vh] overflow-y-auto">
          {/* Model Info */}
          <div>
            <h4 className="text-sm font-medium text-txt-secondary mb-2">Model</h4>
            <p className="text-txt font-medium break-all">{modelName}</p>
            <div className="flex flex-col gap-1 mt-2 text-xs">
              <div className="flex justify-between">
                <span className="text-txt-muted">Actual File Size:</span>
                <span className="text-txt font-medium">{actualFileSizeGB.toFixed(2)} GB</span>
              </div>
              <div className="flex justify-between">
                <span className="text-txt-muted">Detected Parameters:</span>
                <span className="text-txt">{calc?.model_params ?? "Unknown"}</span>
              </div>
              <div className="flex justify-between">
                <span className="text-txt-muted">Quantization:</span>
                <span className="text-txt">{calc?.quant_level ?? "Unknown"}</span>
              </div>
            </div>
          </div>

          {/* Overall Rating */}
          <div>
            <h4 className="text-sm font-medium text-txt-secondary mb-2">Overall Rating</h4>
            <div className="flex items-center gap-3">
              <RatingIcon rating={compatibility.rating} size={24} />
              <span className={`text-2xl font-bold ${ratingColors[compatibility.rating] || ratingColors.unknown}`}>
                {ratingLabels[compatibility.rating] || ratingLabels.unknown}
              </span>
            </div>
            <p className="text-sm text-txt-secondary mt-2">{compatibility.reason}</p>
          </div>

          {/* Calculation Breakdown */}
          {calc && (
          <div>
            <h4 className="text-sm font-medium text-txt-secondary mb-3 flex items-center gap-2">
              <Zap size={14} />
              How We Calculated This
            </h4>
            
            {/* File Size Estimation */}
            <div className="bg-void border border-glass-border rounded-lg p-4 mb-3">
              <div className="flex items-center gap-2 mb-2">
                <HardDrive size={14} className="text-txt-secondary" />
                <span className="text-sm font-medium text-txt">1. File Size Estimation</span>
              </div>
              <div className="space-y-1 text-xs font-mono bg-void-900 p-3 rounded">
                <div className="text-txt-muted">Base FP16 size ({calc.model_params}): <span className="text-neon">{calc.base_fp16_size_gb.toFixed(1)} GB</span></div>
                <div className="text-txt-muted">Quant multiplier ({calc.quant_level}): <span className="text-neon">x{calc.quant_multiplier.toFixed(2)}</span></div>
                <div className="border-t border-glass-border my-2"></div>
                <div className="text-txt-muted">Estimated size: <span className="text-txt">{calc.estimated_file_size_gb.toFixed(2)} GB</span> <span className="text-txt-muted">(= {calc.base_fp16_size_gb.toFixed(1)} x {calc.quant_multiplier.toFixed(2)})</span></div>
                <div className="text-txt-muted">Actual size: <span className="text-txt font-bold">{calc.actual_file_size_gb.toFixed(2)} GB</span></div>
                {Math.abs(calc.estimated_file_size_gb - calc.actual_file_size_gb) > 0.5 && (
                  <div className="text-txt-muted mt-2 text-[10px] italic">
                    Note: Difference due to model architecture variations. Actual size used for calculations.
                  </div>
                )}
              </div>
              
              {/* Disk Space Info */}
              <div className="mt-3 text-xs bg-void-800/50 p-3 rounded border border-glass-border">
                <div className="flex items-center gap-2 text-txt-secondary mb-2">
                  <HardDrive size={12} />
                  <span className="font-medium">Download Location</span>
                </div>
                <div className="space-y-1 font-mono">
                  <div className="text-txt-muted">Models directory: <span className="text-txt break-all">{deviceSpecs.models_dir ?? "N/A"}</span></div>
                  <div className="text-txt-muted">Drive: <span className="text-txt">{deviceSpecs.models_dir ? deviceSpecs.models_dir.slice(0, 2) : "N/A"}</span></div>
                  <div className="text-txt-muted">Required space: <span className="text-yellow-400">{calc.estimated_file_size_gb.toFixed(2)} GB</span> <span className="text-txt-muted">(estimated file size)</span></div>
                  <div className="text-txt-muted">Available space: <span className="text-txt">{deviceSpecs.available_disk_gb.toFixed(1)} GB</span></div>
                  <div className={`font-bold flex items-center gap-1 ${compatibility.disk_space_sufficient ? 'text-green-400' : 'text-red-400'}`}>
                    {compatibility.disk_space_sufficient ? <CheckCircle size={12} /> : <XCircle size={12} />}
                    <span>Status: {compatibility.disk_space_sufficient ? 'Sufficient' : 'Insufficient'}</span>
                  </div>
                </div>
              </div>
            </div>

            {/* RAM Estimation */}
            <div className="bg-void border border-glass-border rounded-lg p-4 mb-3">
              <div className="flex items-center gap-2 mb-2">
                <MemoryStick size={14} className="text-txt-secondary" />
                <span className="text-sm font-medium text-txt">2. RAM Requirement</span>
              </div>
              <div className="space-y-1 text-xs font-mono bg-void-900 p-3 rounded">
                <div className="text-txt-muted">Actual file size: <span className="text-txt">{calc.actual_file_size_gb.toFixed(2)} GB</span></div>
                <div className="text-txt-muted">RAM multiplier: <span className="text-neon">x{calc.ram_multiplier.toFixed(1)}</span> {calc.assumed_context >= 8192 ? '(large context)' : '(standard context)'}</div>
                <div className="text-txt-muted">Context assumed: <span className="text-txt">{calc.assumed_context.toLocaleString()} tokens</span></div>
                <div className="border-t border-glass-border my-2"></div>
                <div className="text-txt-muted">Required RAM: <span className="text-yellow-400 font-bold">{calc.required_ram_gb.toFixed(1)} GB</span> <span className="text-txt-muted">(= {calc.actual_file_size_gb.toFixed(2)} x {calc.ram_multiplier.toFixed(1)})</span></div>
                <div className="text-txt-muted">Your total RAM: <span className="text-txt">{calc.total_ram_gb.toFixed(1)} GB</span></div>
                <div className="text-txt-muted">Your available RAM: <span className="text-txt">{calc.available_ram_gb.toFixed(1)} GB</span></div>
                <div className="border-t border-glass-border my-2"></div>
                <div className={`font-bold flex items-center gap-1 ${calc.ram_decision === 'OK' ? 'text-green-400' : calc.ram_decision === 'NOT_RECOMMENDED' ? 'text-yellow-400' : 'text-red-400'}`}>
                  {calc.ram_decision === 'OK' ? <CheckCircle size={12} /> : calc.ram_decision === 'NOT_RECOMMENDED' ? <AlertTriangle size={12} /> : <XCircle size={12} />}
                  <span>Decision: {calc.ram_decision === 'OK' ? 'OK' : calc.ram_decision === 'NOT_RECOMMENDED' ? 'Not Recommended' : 'Do Not Download'}</span>
                </div>
              </div>
            </div>

            {/* CPU Performance */}
            <div className="bg-void border border-glass-border rounded-lg p-4 mb-3">
              <div className="flex items-center gap-2 mb-2">
                <Cpu size={14} className="text-txt-secondary" />
                <span className="text-sm font-medium text-txt">3. CPU Performance Score</span>
              </div>
              <div className="space-y-1 text-xs font-mono bg-void-900 p-3 rounded">
                <div className="text-txt-muted">CPU cores: <span className="text-txt">{calc.cpu_cores}</span></div>
                <div className="text-txt-muted flex items-center gap-1">
                  AVX2 support: 
                  <span className={`flex items-center gap-1 ${calc.cpu_has_avx2 ? 'text-green-400' : 'text-yellow-400'}`}>
                    {calc.cpu_has_avx2 ? <CheckCircle size={10} /> : <XCircle size={10} />}
                    {calc.cpu_has_avx2 ? 'Yes (x1.0)' : 'No (x0.5)'}
                  </span>
                </div>
                <div className="text-txt-muted">CPU score: <span className="text-txt">{calc.cpu_score.toFixed(1)}</span> <span className="text-txt-muted">(= {calc.cpu_cores} x {calc.cpu_has_avx2 ? '1.0' : '0.5'})</span></div>
                <div className="border-t border-glass-border my-2"></div>
                <div className="text-txt-muted">Model factor ({calc.model_params}): <span className="text-txt">{calc.model_factor.toFixed(1)}</span></div>
                <div className="text-txt-muted">Quant boost ({calc.quant_level}): <span className="text-neon">x{calc.quant_boost.toFixed(2)}</span></div>
                <div className="border-t border-glass-border my-2"></div>
                <div className="text-txt-muted">
                  Performance score: <span className="text-yellow-400 font-bold">{calc.perf_score.toFixed(2)}</span>
                  <span className="text-txt-muted ml-2">(= ({calc.cpu_score.toFixed(1)} / {calc.model_factor.toFixed(1)}) x {calc.quant_boost.toFixed(2)})</span>
                </div>
                <div className={`font-bold flex items-center gap-1 ${calc.perf_score >= 2 ? 'text-green-400' : calc.perf_score >= 1 ? 'text-yellow-400' : calc.perf_score >= 0.5 ? 'text-orange-400' : 'text-red-400'}`}>
                  <Zap size={12} />
                  <span>Classification: {calc.perf_classification}</span>
                </div>
              </div>
            </div>

            {/* Scoring Guide */}
            <div className="text-xs text-txt-muted bg-void-800/50 p-3 rounded border border-glass-border">
              <div className="font-medium text-txt-secondary mb-1">Performance Score Guide:</div>
              <div className="grid grid-cols-2 gap-1">
                <span className="flex items-center gap-1"><CheckCircle size={10} className="text-green-400" /> &gt;= 2.0: Efficient (Fast)</span>
                <span className="flex items-center gap-1"><AlertTriangle size={10} className="text-yellow-400" /> &gt;= 1.0: Usable</span>
                <span className="flex items-center gap-1"><AlertTriangle size={10} className="text-orange-400" /> &gt;= 0.5: Very Slow</span>
                <span className="flex items-center gap-1"><XCircle size={10} className="text-red-400" /> &lt; 0.5: Not Recommended</span>
              </div>
            </div>
          </div>
          )}

          {/* Performance Notes */}
          {compatibility.details.performance_notes.length > 0 && (
            <div>
              <h4 className="text-sm font-medium text-txt-secondary mb-2 flex items-center gap-2">
                <Info size={14} />
                Performance Notes
              </h4>
              <ul className="space-y-2">
                {compatibility.details.performance_notes.map((note, idx) => (
                  <li key={idx} className="flex items-start gap-2 text-sm text-txt-secondary">
                    <AlertTriangle size={12} className="text-yellow-400 mt-0.5 shrink-0" />
                    <span>{note}</span>
                  </li>
                ))}
              </ul>
            </div>
          )}

          {/* Alternative Recommendation */}
          {compatibility.alternative && (
            <div className="bg-neon/10 border border-neon/30 rounded-lg p-4">
              <h4 className="text-sm font-medium text-neon mb-2 flex items-center gap-2">
                <Lightbulb size={14} />
                Recommended Alternative
              </h4>
              <p className="text-sm text-txt">
                <span className="font-medium">{compatibility.alternative.suggestion}</span>
              </p>
              <p className="text-xs text-txt-secondary mt-1">{compatibility.alternative.reason}</p>
            </div>
          )}

          {/* Recommendation */}
          <div className="bg-void-800 border border-glass-border rounded-lg p-4">
            <h4 className="text-sm font-medium text-txt mb-2">Summary</h4>
            <p className="text-sm text-txt-secondary">
              {compatibility.rating === "efficient" && 
                "Your system is well-suited for this model. You can expect smooth performance and quick response times."}
              {(compatibility.rating === "usable" || compatibility.rating === "satisfies") && 
                "This model will work on your system with acceptable performance. You may notice some delays during inference."}
              {compatibility.rating === "veryslow" && 
                "This model will run very slowly on your system. Consider a smaller model or lower quantization for better experience."}
              {compatibility.rating === "notrecommended" && 
                "This model is not recommended for your system. Performance will be poor and may cause system instability."}
              {compatibility.rating === "wontrun" && 
                "This model cannot run on your system due to insufficient resources. Please choose a smaller model."}
              {compatibility.rating === "unknown" && 
                "Unable to determine compatibility. Please verify your system specifications."}
            </p>
          </div>
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
        
        // Prefer documented requirements over estimation
        let estimatedRAMMB: number | undefined;
        let contextLength: number | undefined;
        
        if (documentedRequirements?.source === 'documented' && documentedRequirements.minRAM) {
          // Use documented RAM requirement
          estimatedRAMMB = Math.round(documentedRequirements.minRAM * 1024);
        }
        
        if (documentedRequirements?.contextLength) {
          contextLength = documentedRequirements.contextLength;
        }
        
        try {
          // Pass filename to backend for better model detection
          const compat = await Api.checkCompatibility(
            fileSizeMb, 
            estimatedRAMMB, 
            quant || undefined, 
            filename,
            contextLength
          );
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
                <div className="text-txt-secondary bg-void-900 px-3 py-1.5 rounded-lg border border-glass-border flex items-center gap-1.5">
                  <MemoryStick size={12} className="text-txt-muted" />
                  <span className="text-txt">{deviceSpecs.available_ram_gb.toFixed(1)}</span>
                  <span className="text-txt-muted">/{deviceSpecs.total_ram_gb.toFixed(1)} GB</span>
                </div>
                <div className="text-txt-secondary bg-void-900 px-3 py-1.5 rounded-lg border border-glass-border flex items-center gap-1.5" title={`Models dir: ${deviceSpecs.models_dir ?? "N/A"}`}>
                  <HardDrive size={12} className="text-txt-muted" />
                  <span className="text-txt-muted">{deviceSpecs.models_dir ? deviceSpecs.models_dir.slice(0, 2) : "N/A"}</span>
                  <span className="text-txt">{deviceSpecs.available_disk_gb.toFixed(1)}</span>
                  <span className="text-txt-muted">/{deviceSpecs.total_disk_gb.toFixed(1)} GB</span>
                </div>
                <div className="text-txt-secondary bg-void-900 px-3 py-1.5 rounded-lg border border-glass-border flex items-center gap-1.5">
                  <Cpu size={12} className="text-txt-muted" />
                  <span className="text-txt">{deviceSpecs.cpu_cores} cores</span>
                  <span className={`flex items-center gap-0.5 ${deviceSpecs.cpu_has_avx2 ? 'text-green-400' : 'text-yellow-400'}`}>
                    {deviceSpecs.cpu_has_avx2 ? <CheckCircle size={10} /> : <AlertTriangle size={10} />}
                    {deviceSpecs.cpu_has_avx2 ? 'AVX2' : 'No AVX2'}
                  </span>
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
          <div className="hf-modal-scrollable w-1/2 border-r border-glass-border overflow-y-auto p-4 flex flex-col gap-2">
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
          <div className="hf-modal-scrollable w-1/2 overflow-y-auto p-4 bg-void-800/30">
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