import React, { useState, useEffect } from "react";
import { listen } from "@tauri-apps/api/event";
import { X, Search, Download, Loader2 } from "lucide-react";
import { Api, type HFModel, type HFRepoFile } from "../api";
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
    try {
      const files = await Api.getHuggingFaceRepoFiles(repoId);
      const ggufs = files.filter(f => f.path.endsWith(".gguf"));
      setRepoFiles(ggufs);
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
          <button onClick={onClose} className="text-txt-secondary hover:text-txt transition-colors">
            <X size={24} />
          </button>
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
                        
                        return (
                          <div key={file.oid} className="bg-void border border-glass-border rounded-lg p-3 flex flex-col gap-2">
                            <div className="flex justify-between items-start gap-2">
                              <span className="text-sm font-medium text-txt break-all">{filename}</span>
                              <span className="text-xs text-txt-secondary whitespace-nowrap">{(file.size / 1024 / 1024 / 1024).toFixed(2)} GB</span>
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
    </div>
  );
}