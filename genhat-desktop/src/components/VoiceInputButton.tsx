/**
 * VoiceInputButton — Microphone button for real-time speech-to-text input.
 *
 * Records audio via the native Rust backend (cpal) because the Tauri WebView
 * on macOS does not expose navigator.mediaDevices. When the user stops
 * recording, the audio is sent to Parakeet ASR for transcription.
 */

import React, { useState, useCallback, useRef, useEffect } from "react";
import { Mic, MicOff, Loader2, Square } from "lucide-react";
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
  const [isRecording, setIsRecording] = useState(false);
  const [isTranscribing, setIsTranscribing] = useState(false);
  const [error, setError] = useState<string | null>(null);
  const [duration, setDuration] = useState(0);
  const timerRef = useRef<number | null>(null);
  const startTimeRef = useRef(0);

  // Clean up timer on unmount
  useEffect(() => {
    return () => {
      if (timerRef.current) clearInterval(timerRef.current);
    };
  }, []);

  const handleClick = useCallback(async () => {
    setError(null);

    if (isRecording) {
      // Stop recording and transcribe
      if (timerRef.current) {
        clearInterval(timerRef.current);
        timerRef.current = null;
      }
      setIsRecording(false);
      setIsTranscribing(true);
      try {
        const base64Audio = await Api.stopMicRecording();
        const transcript = await Api.transcribeAudioBase64(base64Audio);
        if (transcript && transcript.trim()) {
          onTranscript(transcript.trim());
        }
      } catch (err) {
        console.error("Transcription error:", err);
        const msg =
          err instanceof Error
            ? err.message
            : typeof err === "string"
              ? err
              : "Transcription failed";
        setError(msg);
      } finally {
        setIsTranscribing(false);
        setDuration(0);
      }
    } else {
      // Start recording via native backend
      try {
        await Api.startMicRecording();
        setIsRecording(true);
        startTimeRef.current = Date.now();
        timerRef.current = window.setInterval(() => {
          setDuration(Math.floor((Date.now() - startTimeRef.current) / 1000));
        }, 1000);
      } catch (err) {
        console.error("Recording start error:", err);
        const msg =
          err instanceof Error
            ? err.message
            : typeof err === "string"
              ? err
              : "Failed to start recording";
        setError(msg);
      }
    }
  }, [isRecording, onTranscript]);

  const handleCancel = useCallback(
    async (e: React.MouseEvent) => {
      e.stopPropagation();
      if (timerRef.current) {
        clearInterval(timerRef.current);
        timerRef.current = null;
      }
      setIsRecording(false);
      setDuration(0);
      // Stop the native recording but discard the result
      try {
        await Api.stopMicRecording();
      } catch {
        // ignore — already stopped or never started
      }
    },
    []
  );

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
          ${isRecording 
            ? "text-danger border-danger/50 bg-danger/10 shadow-[0_0_12px_rgba(239,68,68,0.2)] animate-pulse" 
            : "text-txt-muted hover:text-neon hover:border-neon/30 hover:shadow-[0_0_8px_rgba(0,212,255,0.1)]"
          }
          ${isTranscribing ? "opacity-75" : ""}
          disabled:opacity-40 disabled:cursor-not-allowed
        `}
        title={isRecording ? "Stop recording" : "Start voice input"}
      >
        {isTranscribing ? (
          <Loader2 className="w-5 h-5 animate-spin" />
        ) : isRecording ? (
          <MicOff className="w-5 h-5" />
        ) : (
          <Mic className="w-5 h-5" />
        )}
      </button>

      {/* Recording indicator & duration */}
      {isRecording && (
        <div className="absolute bottom-full left-1/2 -translate-x-1/2 mb-2 flex items-center gap-2 py-1 px-2 rounded-lg bg-void-700/90 border border-glass-border text-xs whitespace-nowrap">
          <span className="w-2 h-2 rounded-full bg-danger animate-pulse" />
          <span className="text-txt-secondary">
            {formatDuration(duration)}
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
        <div className="absolute bottom-full left-1/2 -translate-x-1/2 mb-2 py-1 px-2 rounded-lg bg-danger/20 border border-danger/30 text-xs text-danger whitespace-nowrap max-w-[250px] text-wrap">
          {error}
        </div>
      )}
    </div>
  );
};

export default VoiceInputButton;
