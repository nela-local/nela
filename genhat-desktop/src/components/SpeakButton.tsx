/**
 * SpeakButton — Button to read text aloud using streaming TTS.
 *
 * Uses KittenTTS to convert text to speech, playing audio in chunks
 * as they're generated for a responsive experience.
 */

import React, { useCallback } from "react";
import { Volume2, VolumeX, Pause, Play, Loader2 } from "lucide-react";
import { useStreamingTts } from "../hooks/useStreamingTts";
import { Api } from "../api";

interface SpeakButtonProps {
  /** Text content to speak */
  text: string;
  /** TTS voice to use */
  voice?: string;
  /** TTS speed multiplier */
  speed?: number;
  /** Whether the button should be disabled */
  disabled?: boolean;
  /** Optional class name for styling */
  className?: string;
  /** Compact mode (smaller button) */
  compact?: boolean;
}

export const SpeakButton: React.FC<SpeakButtonProps> = ({
  text,
  voice = "Leo",
  speed = 1.0,
  disabled = false,
  className = "",
  compact = false,
}) => {
  const generateChunk = useCallback(
    (chunkText: string, opts?: { voice?: string; speed?: number }) =>
      Api.generateSpeechChunk(chunkText, opts),
    []
  );

  const { state, speak, pause, resume, stop } = useStreamingTts(generateChunk);

  const handleClick = useCallback(() => {
    if (state.isPlaying) {
      if (state.isPaused) {
        resume();
      } else {
        pause();
      }
    } else {
      speak(text, voice, speed);
    }
  }, [state.isPlaying, state.isPaused, speak, pause, resume, text, voice, speed]);

  const handleStop = useCallback((e: React.MouseEvent) => {
    e.stopPropagation();
    stop();
  }, [stop]);

  const size = compact ? "w-7 h-7" : "w-9 h-9";
  const iconSize = compact ? "w-4 h-4" : "w-5 h-5";

  // Show loading spinner while generating/playing
  const isGenerating = state.isPlaying && !state.isSpeaking && state.currentChunkIndex === 0;

  return (
    <div className={`relative inline-flex items-center gap-1 ${className}`}>
      <button
        onClick={handleClick}
        disabled={disabled || !text?.trim()}
        className={`
          glass-btn flex items-center justify-center ${size}
          bg-glass-bg border border-glass-border 
          cursor-pointer rounded-lg transition-all duration-200 backdrop-blur-sm
          ${state.isPlaying 
            ? "text-neon border-neon/50 bg-neon/10 shadow-[0_0_12px_rgba(0,212,255,0.2)]" 
            : "text-txt-muted hover:text-neon hover:border-neon/30 hover:shadow-[0_0_8px_rgba(0,212,255,0.1)]"
          }
          disabled:opacity-40 disabled:cursor-not-allowed
        `}
        title={
          state.isPlaying 
            ? state.isPaused 
              ? "Resume speaking" 
              : "Pause speaking" 
            : "Read aloud"
        }
      >
        {isGenerating ? (
          <Loader2 className={`${iconSize} animate-spin`} />
        ) : state.isPlaying ? (
          state.isPaused ? (
            <Play className={iconSize} />
          ) : (
            <Pause className={iconSize} />
          )
        ) : (
          <Volume2 className={iconSize} />
        )}
      </button>

      {/* Stop button when playing */}
      {state.isPlaying && (
        <button
          onClick={handleStop}
          className={`
            glass-btn flex items-center justify-center ${size}
            bg-glass-bg border border-glass-border 
            cursor-pointer rounded-lg transition-all duration-200 backdrop-blur-sm
            text-txt-muted hover:text-danger hover:border-danger/30 hover:bg-danger/10
          `}
          title="Stop speaking"
        >
          <VolumeX className={iconSize} />
        </button>
      )}

      {/* Progress indicator */}
      {state.isPlaying && state.totalChunks > 1 && (
        <div className="text-[0.65rem] text-txt-muted ml-1">
          {state.currentChunkIndex}/{state.totalChunks}
        </div>
      )}

      {/* Error indicator */}
      {state.error && (
        <div className="absolute bottom-full left-1/2 -translate-x-1/2 mb-2 py-1 px-2 rounded-lg bg-danger/20 border border-danger/30 text-xs text-danger whitespace-nowrap">
          {state.error}
        </div>
      )}
    </div>
  );
};

export default SpeakButton;
