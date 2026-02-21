import { useState, useEffect, useRef } from "react";
import { invoke, convertFileSrc } from "@tauri-apps/api/core";
import { listen } from "@tauri-apps/api/event";
import { open } from "@tauri-apps/plugin-dialog";

type ChatMode = "text" | "vision" | "audio" | "rag";

interface ModelFile {
  name: string;
  path: string;
}

interface RegisteredModel {
  id: string;
  name: string;
  tasks: string[];
}

interface IngestionStatus {
  doc_id: number;
  title: string;
  total_chunks: number;
  embedded_chunks: number;
  enriched_chunks: number;
  phase: string;
}

interface SourceChunk {
  chunk_id: number;
  doc_title: string;
  text: string;
  score: number;
}

interface RagResult {
  answer: string;
  sources: SourceChunk[];
}

function App() {
  const [models, setModels] = useState<ModelFile[]>([]);
  const [selectedModel, setSelectedModel] = useState("");
  
  const [audioModels, setAudioModels] = useState<ModelFile[]>([]);
  const [selectedAudioModel, setSelectedAudioModel] = useState("None");
  const [audioOutput, setAudioOutput] = useState("");

  const [visionModels, setVisionModels] = useState<RegisteredModel[]>([]);
  const [selectedVisionModel, setSelectedVisionModel] = useState("");

  const [chatMode, setChatMode] = useState<ChatMode>("text");
  const [imagePath, setImagePath] = useState<string | null>(null);
  const [imagePreview, setImagePreview] = useState<string | null>(null);

  const [prompt, setPrompt] = useState("");
  const [response, setResponse] = useState("");
  const [loading, setLoading] = useState(false);
  const visionUnlistenRef = useRef<(() => void) | null>(null);

  // RAG state
  const [ragDocs, setRagDocs] = useState<IngestionStatus[]>([]);
  const [ragResult, setRagResult] = useState<RagResult | null>(null);
  const [ragIngesting, setRagIngesting] = useState(false);
  const [enrichmentStatus, setEnrichmentStatus] = useState<string | null>(null);

  // Clean up vision stream listener on unmount
  useEffect(() => {
    return () => {
      visionUnlistenRef.current?.();
      visionUnlistenRef.current = null;
    };
  }, []);

  useEffect(() => {
    invoke<ModelFile[]>("list_models")
      .then((list) => {
        setModels(list);
        if (list.length > 0) {
          setSelectedModel(list[0].path);
        }
      })
      .catch(console.error);

    invoke<ModelFile[]>("list_audio_models")
      .then((list) => {
        setAudioModels(list);
      })
      .catch(console.error);

    invoke<RegisteredModel[]>("list_registered_models")
      .then((list) => {
        const vision = list.filter((m) => m.tasks.includes("vision_chat"));
        setVisionModels(vision);
        if (vision.length > 0) {
          setSelectedVisionModel(vision[0].id);
        }
      })
      .catch(console.error);
  }, []);

  const handleModelChange = async (path: string) => {
    try {
      setSelectedModel(path);
      await invoke("switch_model", { modelPath: path });
      setResponse(""); 
      alert(`Switched to model: ${path}`);
    } catch (err) {
      console.error(err);
      alert("Failed to switch model");
    }
  };

  const selectImage = async () => {
    try {
      const selected = await open({
        multiple: false,
        filters: [{ name: "Images", extensions: ["jpg", "jpeg", "png", "webp", "gif", "bmp"] }],
      });
      if (selected && typeof selected === "string") {
        setImagePath(selected);
        // Load image as base64 data URL for preview
        const dataUrl = await invoke<string>("read_image_base64", { path: selected });
        setImagePreview(dataUrl);
      }
    } catch (err) {
      console.error("Failed to select image:", err);
    }
  };

  const clearImage = () => {
    setImagePath(null);
    setImagePreview(null);
  };

  // RAG helpers
  const loadRagDocs = async () => {
    try {
      const docs = await invoke<IngestionStatus[]>("list_rag_documents");
      setRagDocs(docs);
    } catch (e) {
      console.error("Failed to load RAG docs:", e);
    }
  };

  useEffect(() => {
    if (chatMode === "rag") loadRagDocs();
  }, [chatMode]);

  // Listen for enrichment progress events
  useEffect(() => {
    let unlisten: (() => void) | null = null;
    listen<{ enriched_this_round: number; status: string }>("rag:enrichment_progress", (event) => {
      if (event.payload.status === "in_progress") {
        setEnrichmentStatus(`Enriched ${event.payload.enriched_this_round} chunks`);
        // Refresh document list to show updated enrichment counts
        if (chatMode === "rag") loadRagDocs();
        // Clear status after 5 seconds
        setTimeout(() => setEnrichmentStatus(null), 5000);
      }
    }).then((fn) => { unlisten = fn; });
    return () => { unlisten?.(); };
  }, [chatMode]);

  const ingestFile = async () => {
    try {
      const selected = await open({
        multiple: false,
        filters: [
          { name: "Documents", extensions: ["pdf", "docx", "pptx", "txt", "md", "rs", "py", "js", "ts", "java", "c", "cpp", "go", "toml", "yaml", "json", "xml", "csv", "mp3", "wav", "m4a", "ogg", "flac"] },
        ],
      });
      if (selected && typeof selected === "string") {
        setRagIngesting(true);
        await invoke("ingest_document", { path: selected });
        await loadRagDocs();
        setRagIngesting(false);
      }
    } catch (e) {
      console.error(e);
      setRagIngesting(false);
      alert(`Ingest failed: ${e}`);
    }
  };

  const ingestDir = async () => {
    try {
      const selected = await open({ directory: true });
      if (selected && typeof selected === "string") {
        setRagIngesting(true);
        await invoke("ingest_folder", { path: selected });
        await loadRagDocs();
        setRagIngesting(false);
      }
    } catch (e) {
      console.error(e);
      setRagIngesting(false);
      alert(`Folder ingest failed: ${e}`);
    }
  };

  const deleteRagDoc = async (docId: number) => {
    try {
      await invoke("delete_rag_document", { docId });
      await loadRagDocs();
    } catch (e) {
      console.error(e);
      alert(`Delete failed: ${e}`);
    }
  };

  const sendPrompt = async () => {
    setResponse("");
    setAudioOutput("");
    setLoading(true);

    try {
      // RAG Mode
      if (chatMode === "rag") {
        try {
          const result = await invoke<RagResult>("query_rag", { query: prompt });
          setRagResult(result);
          setResponse(result.answer);
        } catch (e) {
          console.error(e);
          setResponse(`RAG query error: ${e}`);
        }
        setLoading(false);
        return;
      }

      // Audio Mode
      if (chatMode === "audio" && selectedAudioModel) {
         try {
           const path = await invoke<string>("generate_speech", {
             modelPath: selectedAudioModel,
             input: prompt,
           });
           setAudioOutput(convertFileSrc(path));
         } catch (e) {
           console.error(e);
           setResponse(`Error generating audio: ${e}`);
         }
         setLoading(false);
         return;
      }

      // Vision Mode - use streaming CLI backend
      if (chatMode === "vision") {
        if (!imagePath) {
          setResponse("Please select an image first");
          setLoading(false);
          return;
        }
        
        try {
          // Clean up any previous listener before registering a new one
          visionUnlistenRef.current?.();
          visionUnlistenRef.current = null;

          // Listen for streaming events from the backend
          const unlisten = await listen<{ chunk: string; done: boolean }>("vision-stream", (event) => {
            if (event.payload.done) {
              setLoading(false);
              visionUnlistenRef.current?.();
              visionUnlistenRef.current = null;
            } else if (event.payload.chunk) {
              setResponse(prev => prev + event.payload.chunk);
            }
          });
          visionUnlistenRef.current = unlisten;

          // Start the streaming vision chat
          await invoke("vision_chat_stream", {
            imagePath: imagePath,
            prompt: prompt || "What's in this image?",
            modelId: selectedVisionModel || null,
          });
        } catch (e) {
          console.error(e);
          setResponse(`Error: ${e}`);
          setLoading(false);
        } finally {
          // Always clean up listener if the backend didn't emit a done event
          // (e.g. invoke threw, process crashed, or exited without done)
          if (visionUnlistenRef.current) {
            visionUnlistenRef.current();
            visionUnlistenRef.current = null;
          }
        }
        return;
      }

      // Text Chat Mode - get dynamic port from backend (triggers lazy load)
      const port = await invoke<number>("get_llama_port");
      if (!port) {
        throw new Error("LLM server not running");
      }

      const res = await fetch(`http://127.0.0.1:${port}/v1/chat/completions`, {
        method: "POST",
        headers: { "Content-Type": "application/json" },
        body: JSON.stringify({
          messages: [
            { role: "user", content: prompt }
          ],
          max_tokens: 256,
          stream: true,
        }),
      });

      if (!res.body) throw new Error("No response body");

      const reader = res.body.getReader();
      const decoder = new TextDecoder();

      let buffer = "";

      while (true) {
        const { value, done } = await reader.read();
        if (done) break;

        buffer += decoder.decode(value, { stream: true });

        const lines = buffer.split("\n");
        buffer = lines.pop() || "";

        for (const line of lines) {
          if (!line.startsWith("data:")) continue;

          const payload = line.replace("data:", "").trim();
          if (payload === "[DONE]") {
            setLoading(false);
            return;
          }

          try {
            const json = JSON.parse(payload);
            
            const delta = json.choices?.[0]?.delta;
            if (delta && delta.content) {
              setResponse(prev => prev + delta.content);
            }
          } catch {
            // ignore parse errors
          }
        }
      }
      setLoading(false);
    } catch (err) {
      console.error(err);
      setResponse("Streaming error");
      setLoading(false);
    }
  };


  return (
    <div style={{ padding: 20 }}>
      <h1>GenHat Local Intelligence</h1>

      {/* Mode Selector */}
      <div style={{ marginBottom: 20, display: 'flex', gap: '10px' }}>
        <button 
          onClick={() => setChatMode("text")}
          style={{ 
            padding: '8px 16px', 
            background: chatMode === "text" ? '#007bff' : '#333',
            color: '#fff',
            border: 'none',
            borderRadius: 4,
            cursor: 'pointer'
          }}
        >
          💬 Text Chat
        </button>
        <button 
          onClick={() => setChatMode("vision")}
          style={{ 
            padding: '8px 16px', 
            background: chatMode === "vision" ? '#007bff' : '#333',
            color: '#fff',
            border: 'none',
            borderRadius: 4,
            cursor: 'pointer'
          }}
        >
          🖼️ Vision Chat
        </button>
        <button 
          onClick={() => setChatMode("audio")}
          style={{ 
            padding: '8px 16px', 
            background: chatMode === "audio" ? '#007bff' : '#333',
            color: '#fff',
            border: 'none',
            borderRadius: 4,
            cursor: 'pointer'
          }}
        >
          🔊 Audio
        </button>
        <button 
          onClick={() => setChatMode("rag")}
          style={{ 
            padding: '8px 16px', 
            background: chatMode === "rag" ? '#007bff' : '#333',
            color: '#fff',
            border: 'none',
            borderRadius: 4,
            cursor: 'pointer'
          }}
        >
          📚 RAG
        </button>
      </div>

      {/* Model Selectors (only show relevant ones) */}
      <div style={{ display: 'flex', gap: '20px', marginBottom: 20 }}>
        {chatMode === "text" && (
          <div>
            <label htmlFor="model-select" style={{ display: 'block', marginBottom: '5px' }}>LLM Model:</label>
            <select
              id="model-select"
              value={selectedModel}
              onChange={(e) => handleModelChange(e.target.value)}
              disabled={loading || models.length === 0}
              style={{ width: '200px' }}
            >
              {models.map((m) => (
                <option key={m.path} value={m.path}>
                  {m.name}
                </option>
              ))}
            </select>
          </div>
        )}

        {chatMode === "audio" && (
          <div>
            <label htmlFor="audio-select" style={{ display: 'block', marginBottom: '5px' }}>Audio Model:</label>
            <select
              id="audio-select"
              value={selectedAudioModel}
              onChange={(e) => setSelectedAudioModel(e.target.value)}
              disabled={loading}
              style={{ width: '200px' }}
            >
              <option value="None">Select a TTS model</option>
              {audioModels.map((m) => (
                <option key={m.path} value={m.path}>
                  {m.name}
                </option>
              ))}
            </select>
          </div>
        )}

        {chatMode === "vision" && (
          <div>
            <label htmlFor="vision-select" style={{ display: 'block', marginBottom: '5px' }}>Vision Model:</label>
            <select
              id="vision-select"
              value={selectedVisionModel}
              onChange={(e) => setSelectedVisionModel(e.target.value)}
              disabled={loading || visionModels.length === 0}
              style={{ width: '200px' }}
            >
              {visionModels.map((m) => (
                <option key={m.id} value={m.id}>
                  {m.name}
                </option>
              ))}
            </select>
          </div>
        )}
      </div>

      {/* Image Upload (Vision Mode only) */}
      {chatMode === "vision" && (
        <div style={{ marginBottom: 20, padding: 15, border: '2px dashed #555', borderRadius: 8 }}>
          <div style={{ display: 'flex', gap: '10px', alignItems: 'center', marginBottom: 10 }}>
            <button 
              onClick={selectImage} 
              disabled={loading}
              style={{ padding: '8px 16px', cursor: 'pointer' }}
            >
              📁 Select Image
            </button>
            {imagePath && (
              <button 
                onClick={clearImage} 
                disabled={loading}
                style={{ padding: '8px 16px', cursor: 'pointer', background: '#dc3545', color: '#fff', border: 'none', borderRadius: 4 }}
              >
                ✕ Clear
              </button>
            )}
            {imagePath && <span style={{ color: '#888', fontSize: '12px' }}>{imagePath}</span>}
          </div>
          {imagePreview && (
            <img 
              src={imagePreview} 
              alt="Selected" 
              style={{ maxWidth: '100%', maxHeight: '300px', borderRadius: 8 }} 
            />
          )}
        </div>
      )}

      {/* RAG Document Management (RAG Mode only) */}
      {chatMode === "rag" && (
        <div style={{ marginBottom: 20, padding: 15, border: '2px solid #444', borderRadius: 8 }}>
          <div style={{ display: 'flex', gap: '10px', alignItems: 'center', marginBottom: 10 }}>
            <strong>Knowledge Base</strong>
            <button onClick={ingestFile} disabled={ragIngesting} style={{ padding: '6px 12px', cursor: 'pointer' }}>
              📄 Add File
            </button>
            <button onClick={ingestDir} disabled={ragIngesting} style={{ padding: '6px 12px', cursor: 'pointer' }}>
              📁 Add Folder
            </button>
            {ragIngesting && <span style={{ color: '#ffaa00' }}>Ingesting...</span>}
            {enrichmentStatus && <span style={{ color: '#28a745', fontSize: '12px' }}>✓ {enrichmentStatus}</span>}
          </div>
          {ragDocs.length === 0 ? (
            <p style={{ color: '#888', margin: '5px 0' }}>No documents ingested yet. Add files to build your knowledge base.</p>
          ) : (
            <table style={{ width: '100%', borderCollapse: 'collapse', fontSize: '13px' }}>
              <thead>
                <tr style={{ borderBottom: '1px solid #555' }}>
                  <th style={{ textAlign: 'left', padding: '4px 8px' }}>Document</th>
                  <th style={{ textAlign: 'center', padding: '4px 8px' }}>Chunks</th>
                  <th style={{ textAlign: 'center', padding: '4px 8px' }}>Enriched</th>
                  <th style={{ textAlign: 'center', padding: '4px 8px' }}>Phase</th>
                  <th style={{ textAlign: 'center', padding: '4px 8px' }}></th>
                </tr>
              </thead>
              <tbody>
                {ragDocs.map((doc) => (
                  <tr key={doc.doc_id} style={{ borderBottom: '1px solid #333' }}>
                    <td style={{ padding: '4px 8px' }}>{doc.title}</td>
                    <td style={{ textAlign: 'center', padding: '4px 8px' }}>{doc.total_chunks}</td>
                    <td style={{ textAlign: 'center', padding: '4px 8px' }}>{doc.enriched_chunks}/{doc.total_chunks}</td>
                    <td style={{ textAlign: 'center', padding: '4px 8px' }}>
                      <span style={{ 
                        padding: '2px 6px', 
                        borderRadius: 3, 
                        fontSize: '11px',
                        background: doc.phase.includes('phase2_complete') ? '#28a745' : doc.phase.includes('phase2') ? '#ffaa00' : '#17a2b8',
                        color: '#fff'
                      }}>
                        {doc.phase.replace(/_/g, ' ')}
                      </span>
                    </td>
                    <td style={{ textAlign: 'center', padding: '4px 8px' }}>
                      <button onClick={() => deleteRagDoc(doc.doc_id)} style={{ padding: '2px 8px', cursor: 'pointer', background: '#dc3545', color: '#fff', border: 'none', borderRadius: 3, fontSize: '11px' }}>
                        ✕
                      </button>
                    </td>
                  </tr>
                ))}
              </tbody>
            </table>
          )}
        </div>
      )}

      <textarea
        rows={4}
        style={{ width: '100%', marginBottom: '10px' }}
        value={prompt}
        onChange={(e) => setPrompt(e.target.value)}
        placeholder={
          chatMode === "vision" 
            ? "Ask about the image (e.g., 'What's in this image?')" 
            : chatMode === "audio" 
              ? "Type text to generate speech..." 
              : chatMode === "rag"
                ? "Ask a question about your documents..."
                : "Type your prompt for the LLM..."
        }
      />

      <br />

      <button onClick={sendPrompt} disabled={loading} style={{ padding: '8px 16px', cursor: 'pointer' }}>
        {loading 
          ? "Processing..." 
          : chatMode === "vision" 
            ? "Ask Vision Model" 
            : chatMode === "audio" 
              ? "Generate Audio" 
              : chatMode === "rag"
                ? "Query Knowledge Base"
                : "Send to LLM"}
      </button>

      <div style={{ marginTop: 20 }}>
        {audioOutput && (
          <div style={{ marginBottom: 20, padding: 10, border: '1px solid #ccc', borderRadius: 4 }}>
            <p><strong>Generated Audio:</strong></p>
            <audio controls src={audioOutput} autoPlay style={{ width: '100%' }} />
          </div>
        )}
          <pre style={{ whiteSpace: "pre-wrap", background: '#000', color: '#fff', padding: 10, borderRadius: 4, minHeight: 50 }}>          {response}
        </pre>

        {/* RAG Source Citations */}
        {chatMode === "rag" && ragResult && ragResult.sources.length > 0 && (
          <div style={{ marginTop: 15, padding: 10, border: '1px solid #444', borderRadius: 4 }}>
            <strong style={{ marginBottom: 8, display: 'block' }}>📄 Sources ({ragResult.sources.length})</strong>
            {ragResult.sources.map((src, i) => (
              <details key={src.chunk_id} style={{ marginBottom: 8 }}>
                <summary style={{ cursor: 'pointer', fontSize: '13px', color: '#aaf' }}>
                  [Source {i + 1}] {src.doc_title} (score: {src.score.toFixed(4)})
                </summary>
                <pre style={{ whiteSpace: 'pre-wrap', fontSize: '12px', color: '#ccc', padding: '8px', background: '#111', borderRadius: 4, marginTop: 4 }}>
                  {src.text}
                </pre>
              </details>
            ))}
          </div>
        )}
      </div>
    </div>
  );
}

export default App;
