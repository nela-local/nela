import { useState, useEffect } from "react";
import { invoke } from "@tauri-apps/api/core";
import { Api } from "./api";
import type { ChatMessage, ModelFile } from "./types";
import Sidebar from "./components/Sidebar";
import ChatWindow from "./components/ChatWindow";
import ModelSelector from "./components/ModelSelector";
import "./App.css";

function App() {
  const [models, setModels] = useState<ModelFile[]>([]);
  const [selectedModel, setSelectedModel] = useState<string>("");
  
  const [audioModels, setAudioModels] = useState<ModelFile[]>([]);
  const [selectedAudioModel, setSelectedAudioModel] = useState<string>("None");
  const [audioOutput, setAudioOutput] = useState<string>("");

  const [messages, setMessages] = useState<ChatMessage[]>([]);
  const [streamingContent, setStreamingContent] = useState<string>("");
  const [loading, setLoading] = useState(false);

  useEffect(() => {
    refreshModels();
  }, []);

  const refreshModels = () => {
    Api.listModels()
      .then((list) => {
        setModels(list);
        if (list.length > 0 && !selectedModel) {
          setSelectedModel(list[0].path);
        }
      })
      .catch(console.error);

    Api.listAudioModels()
      .then((list) => {
        setAudioModels(list);
      })
      .catch(console.error);
  };

  const handleModelChange = async (path: string) => {
    try {
      setSelectedModel(path);
      await Api.switchModel(path);
      setMessages([]); 
      // alert(`Switched to model: ${path.split("/").pop()}`); // Removed explicit alert for smoother UX
    } catch (err) {
      console.error(err);
      alert("Failed to switch model");
    }
  };

  const handleAddModel = () => {
    alert("To add a model, place the .gguf file into the 'models' folder of the application and restart/refresh.");
    // Future: Implement file picker + copy logic here
  };

  const handleSend = async (text: string) => {
    const newMsg: ChatMessage = { role: "user", content: text };
    setMessages((prev) => [...prev, newMsg]);
    setLoading(true);
    setStreamingContent("");
    setAudioOutput("");

    // If Audio Mode is enabled
    if (selectedAudioModel && selectedAudioModel !== "None") {
      try {
        let fullResponse = "";
        await Api.streamChat(
          [...messages, newMsg],
          (chunk) => {
            setStreamingContent((prev) => prev + chunk);
            fullResponse += chunk;
          },
          async () => {
             // 2. Generate Audio from LLM response
             try {
                const audioPath = await Api.generateSpeech(selectedAudioModel, fullResponse);
                setAudioOutput(audioPath);
             } catch (e) {
                console.error("TTS Error:", e);
             }
             setLoading(false);
             setMessages(prev => [...prev, { role: "assistant", content: fullResponse }]);
             setStreamingContent("");
          },
          (err) => {
             console.error(err);
             setLoading(false);
          }
        );

      } catch (e) {
        console.error(e);
        setLoading(false);
      }
      return;
    }

    // Normal Text Chat
    let fullResponse = "";
    Api.streamChat(
      [...messages, newMsg],
      (chunk) => {
        setStreamingContent((prev) => prev + chunk);
        fullResponse += chunk;
      },
      () => {
        setLoading(false);
        if (fullResponse) {
          setMessages((prev) => [...prev, { role: "assistant", content: fullResponse }]);
          setStreamingContent("");
        }
      },
      (err) => {
        console.error("Stream error", err);
        setLoading(false);
      }
    );
  };

  return (
    <div className="app-container">
      {/* Sidebar simplified or removed in favor of top bar? User asked for selection in chat bar. 
          We keep sidebar for Chat History potentially, but remove model selection from it. */}
      {/* <Sidebar ... />  <- removing for now as per request to focus on chat bar selection */}
      
      <main className="main-content">
        {/* Top Floating Bar for Models */}
        <div className="model-selector-group">
            <ModelSelector
                models={models}
                selectedModel={selectedModel}
                onSelect={handleModelChange}
                type="llm"
                onAdd={handleAddModel}
            />
            <ModelSelector
                models={audioModels}
                selectedModel={selectedAudioModel}
                onSelect={setSelectedAudioModel}
                type="audio"
                onAdd={handleAddModel}
            />
        </div>

        <ChatWindow 
           messages={messages}
           streamingContent={streamingContent}
           isLoading={loading}
           onSend={handleSend}
           audioSrc={audioOutput}
        />
      </main>
    </div>
  );
}

export default App;
