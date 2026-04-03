/**
 * VoiceInputButton — Microphone button for real-time speech-to-text input.
 *
 * When clicked:
 * 1. Starts recording audio from the microphone
 * 2. On stop, transcribes the audio using Parakeet ASR
 * 3. Calls onTranscript with the resulting text
 */

import React, { useState, useCallback } from "react";
import { Mic, MicOff, Loader2, Square } from "lucide-react";
import { useAudioRecorder } from "../hooks/useAudioRecorder";
import { Api } from "../api";

interface VoiceInputButtonProps {
  /** Called with the transcribed text when recording is stopped */
  onTranscript: (text: string) => void;
  /** Whether the button should be disabled */
  disabled?: boolean;
  /** Optional class name for styling */
  className?: string;
}

export const VoiceInputButton: React.FC<VoiceInputButtonProps> = ({
  onTranscript,
  disabled = false,
  className = "",
}) => {
  const { state, startRecording, stopRecording, cancelRecording } = useAudioRecorder();
  const [isTranscribing, setIsTranscribing] = useState(false);
  const [error, setError] = useState<string | null>(null);

  const handleClick = useCallback(async () => {
    setError(null);

    if (state.isRecording) {
      // Stop recording and transcribe
      setIsTranscribing(true);
      try {
        const audioBlob = await stopRecording();
        if (audioBlob) {
          // Convert blob to base64
          const reader = new FileReader();
          const base64Promise = new Promise<string>((resolve, reject) => {
            reader.onloadend = () => {
              const dataUrl = reader.result as string;
              // Strip the data URL prefix (data:audio/wav;base64,)
              const base64 = dataUrl.split(",")[1];
              resolve(base64);
            };
            reader.onerror = reject;
          });
          reader.readAsDataURL(audioBlob);

          const base64Audio = await base64Promise;
          const transcript = await Api.transcribeAudioBase64(base64Audio);
          
          if (transcript && transcript.trim()) {
            onTranscript(transcript.trim());
          }
        }
      } catch (err) {
        console.error("Transcription error:", err);
        setError(err instanceof Error ? err.message : "Transcription failed");
      } finally {
        setIsTranscribing(false);
      }
    } else {
      // Start recording
      await startRecording();
    }
  }, [state.isRecording, startRecording, stopRecording, onTranscript]);

  const handleCancel = useCallback((e: React.MouseEvent) => {
    e.stopPropagation();
    cancelRecording();
  }, [cancelRecording]);

  // Format duration as MM:SS
  const formatDuration = (seconds: number): string => {
    const mins = Math.floor(seconds / 60);
    const secs = seconds % 60;
    return `${mins}:${secs.toString().padStart(2, "0")}`;
  };

  return (
    <div className={`relative inline-flex items-center ${className}`}>
      <button
        onClick={handleClick}
        disabled={disabled || isTranscribing}
        className={`
          glass-btn flex items-center justify-center w-9 h-9 
          bg-glass-bg border border-glass-border 
          cursor-pointer rounded-lg transition-all duration-200 backdrop-blur-sm
          ${state.isRecording 
            ? "text-danger border-danger/50 bg-danger/10 shadow-[0_0_12px_rgba(239,68,68,0.2)] animate-pulse" 
            : "text-txt-muted hover:text-neon hover:border-neon/30 hover:shadow-[0_0_8px_rgba(0,212,255,0.1)]"
          }
          ${isTranscribing ? "opacity-75" : ""}
          disabled:opacity-40 disabled:cursor-not-allowed
        `}
        title={state.isRecording ? "Stop recording" : "Start voice input"}
      >
        {isTranscribing ? (
          <Loader2 className="w-5 h-5 animate-spin" />
        ) : state.isRecording ? (
          <MicOff className="w-5 h-5" />
        ) : (
          <Mic className="w-5 h-5" />
        )}
      </button>

      {/* Recording indicator & duration */}
      {state.isRecording && (
        <div className="absolute bottom-full left-1/2 -translate-x-1/2 mb-2 flex items-center gap-2 py-1 px-2 rounded-lg bg-void-700/90 border border-glass-border text-xs whitespace-nowrap">
          <span className="w-2 h-2 rounded-full bg-danger animate-pulse" />
          <span className="text-txt-secondary">
            {formatDuration(state.duration)}
          </span>
          <button
            onClick={handleCancel}
            className="ml-1 p-0.5 rounded hover:bg-glass-hover text-txt-muted hover:text-danger transition-colors"
            title="Cancel recording"
          >
            <Square className="w-3 h-3" />
          </button>
        </div>
      )}

      {/* Transcribing indicator */}
      {isTranscribing && (
        <div className="absolute bottom-full left-1/2 -translate-x-1/2 mb-2 flex items-center gap-1.5 py-1 px-2 rounded-lg bg-void-700/90 border border-glass-border text-xs whitespace-nowrap">
          <Loader2 className="w-3 h-3 animate-spin text-neon" />
          <span className="text-txt-secondary">Transcribing...</span>
        </div>
      )}

      {/* Error message */}
      {error && (
        <div className="absolute bottom-full left-1/2 -translate-x-1/2 mb-2 py-1 px-2 rounded-lg bg-danger/20 border border-danger/30 text-xs text-danger whitespace-nowrap">
          {error}
        </div>
      )}

      {/* Microphone error */}
      {state.error && (
        <div className="absolute bottom-full left-1/2 -translate-x-1/2 mb-2 py-1 px-2 rounded-lg bg-danger/20 border border-danger/30 text-xs text-danger whitespace-nowrap max-w-[200px] truncate">
          {state.error}
        </div>
      )}
    </div>
  );
};

export default VoiceInputButton;
