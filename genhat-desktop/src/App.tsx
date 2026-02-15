import { useState, useEffect } from "react";
import { invoke, convertFileSrc } from "@tauri-apps/api/core";
import { listen } from "@tauri-apps/api/event";
import { open } from "@tauri-apps/plugin-dialog";

type ChatMode = "text" | "vision" | "audio";

interface ModelFile {
  name: string;
  path: string;
}

function App() {
  const [models, setModels] = useState<ModelFile[]>([]);
  const [selectedModel, setSelectedModel] = useState("");
  
  const [audioModels, setAudioModels] = useState<ModelFile[]>([]);
  const [selectedAudioModel, setSelectedAudioModel] = useState("None");
  const [audioOutput, setAudioOutput] = useState("");

  const [chatMode, setChatMode] = useState<ChatMode>("text");
  const [imagePath, setImagePath] = useState<string | null>(null);
  const [imagePreview, setImagePreview] = useState<string | null>(null);

  const [prompt, setPrompt] = useState("");
  const [response, setResponse] = useState("");
  const [loading, setLoading] = useState(false);

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

  const sendPrompt = async () => {
    setResponse("");
    setAudioOutput("");
    setLoading(true);

    try {
      // Audio Mode
      if (chatMode === "audio" && selectedAudioModel && selectedAudioModel !== "None") {
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
          // Listen for streaming events from the backend
          const unlisten = await listen<{ chunk: string; done: boolean }>("vision-stream", (event) => {
            if (event.payload.done) {
              setLoading(false);
              unlisten();
            } else if (event.payload.chunk) {
              setResponse(prev => prev + event.payload.chunk);
            }
          });

          // Start the streaming vision chat
          await invoke("vision_chat_stream", {
            imagePath: imagePath,
            prompt: prompt || "What's in this image?",
          });
        } catch (e) {
          console.error(e);
          setResponse(`Error: ${e}`);
          setLoading(false);
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
              : "Send to LLM"}
      </button>

      <div style={{ marginTop: 20 }}>
        {audioOutput && (
          <div style={{ marginBottom: 20, padding: 10, border: '1px solid #ccc', borderRadius: 4 }}>
            <p><strong>Generated Audio:</strong></p>
            <audio controls src={audioOutput} autoPlay style={{ width: '100%' }} />
          </div>
        )}
        <pre style={{ whiteSpace: "pre-wrap", background: '#000', padding: 10, borderRadius: 4, minHeight: 50 }}>
          {response}
        </pre>
      </div>
    </div>
  );
}

export default App;
