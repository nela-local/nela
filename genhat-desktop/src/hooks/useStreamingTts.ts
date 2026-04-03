/**
 * useStreamingTts — Hook for streaming text-to-speech output
 *
 * Takes text and generates audio in chunks, playing each chunk as it's ready.
 * This creates a more responsive experience for reading LLM responses.
 */

import { useState, useRef, useCallback, useEffect } from "react";

export interface StreamingTtsState {
  isPlaying: boolean;
  isSpeaking: boolean;
  isPaused: boolean;
  currentChunkIndex: number;
  totalChunks: number;
  error: string | null;
}

export interface UseStreamingTtsReturn {
  state: StreamingTtsState;
  speak: (text: string, voice?: string, speed?: number) => Promise<void>;
  pause: () => void;
  resume: () => void;
  stop: () => void;
}

export interface StreamingTtsOptions {
  /** Called when speech starts */
  onStart?: () => void;
  /** Called when speech ends */
  onEnd?: () => void;
  /** Called on each chunk completion */
  onChunkComplete?: (chunkIndex: number, totalChunks: number) => void;
  /** Called on error */
  onError?: (error: string) => void;
}

/**
 * Split text into sentence chunks for streaming TTS.
 * Keeps punctuation with the preceding sentence.
 */
function splitIntoSentences(text: string): string[] {
  // Split on sentence-ending punctuation while keeping the punctuation
  const sentences = text.match(/[^.!?]+[.!?]+|[^.!?]+$/g) || [text];
  
  return sentences
    .map((s) => s.trim())
    .filter((s) => s.length > 0);
}

export function useStreamingTts(
  generateSpeechChunk: (text: string, options?: { voice?: string; speed?: number }) => Promise<string>,
  options?: StreamingTtsOptions
): UseStreamingTtsReturn {
  const [state, setState] = useState<StreamingTtsState>({
    isPlaying: false,
    isSpeaking: false,
    isPaused: false,
    currentChunkIndex: 0,
    totalChunks: 0,
    error: null,
  });

  const audioRef = useRef<HTMLAudioElement | null>(null);
  const audioQueueRef = useRef<string[]>([]);
  const sentencesRef = useRef<string[]>([]);
  const currentIndexRef = useRef<number>(0);
  const isStoppedRef = useRef<boolean>(false);
  const isPausedRef = useRef<boolean>(false);
  const voiceRef = useRef<string | undefined>(undefined);
  const speedRef = useRef<number | undefined>(undefined);

  // Play the next audio chunk from the queue
  const playNextChunk = useCallback(() => {
    if (isStoppedRef.current || isPausedRef.current) return;

    if (audioQueueRef.current.length > 0) {
      const audioUrl = audioQueueRef.current.shift()!;
      
      if (audioRef.current) {
        audioRef.current.pause();
        audioRef.current.src = "";
      }

      const audio = new Audio(audioUrl);
      audioRef.current = audio;

      audio.onended = () => {
        currentIndexRef.current++;
        setState((prev) => ({
          ...prev,
          currentChunkIndex: currentIndexRef.current,
        }));
        options?.onChunkComplete?.(currentIndexRef.current, sentencesRef.current.length);
        
        // Play next chunk if available
        if (audioQueueRef.current.length > 0) {
          playNextChunk();
        } else if (currentIndexRef.current >= sentencesRef.current.length) {
          // All chunks played
          setState((prev) => ({
            ...prev,
            isPlaying: false,
            isSpeaking: false,
          }));
          options?.onEnd?.();
        }
      };

      audio.onerror = () => {
        const error = "Failed to play audio chunk";
        setState((prev) => ({ ...prev, error }));
        options?.onError?.(error);
      };

      setState((prev) => ({ ...prev, isSpeaking: true }));
      audio.play().catch((err) => {
        console.error("Audio play error:", err);
        // Try playing next chunk
        playNextChunk();
      });
    }
  }, [options]);

  // Generate audio for remaining sentences
  const generateRemainingChunks = useCallback(async () => {
    const sentences = sentencesRef.current;
    let generateIndex = 0;

    while (generateIndex < sentences.length && !isStoppedRef.current) {
      const sentence = sentences[generateIndex];
      
      try {
        const audioUrl = await generateSpeechChunk(sentence, {
          voice: voiceRef.current,
          speed: speedRef.current,
        });

        if (!isStoppedRef.current && audioUrl) {
          audioQueueRef.current.push(audioUrl);
          
          // Start playing if this is the first chunk
          if (generateIndex === 0 && !isPausedRef.current) {
            playNextChunk();
          }
        }
      } catch (err) {
        console.error(`Failed to generate chunk ${generateIndex}:`, err);
        // Continue with next chunk
      }

      generateIndex++;
    }

    // If all chunks are generated but nothing is playing yet, start playing
    if (!isStoppedRef.current && audioQueueRef.current.length > 0 && !audioRef.current?.currentTime) {
      playNextChunk();
    }
  }, [generateSpeechChunk, playNextChunk]);

  const speak = useCallback(async (text: string, voice?: string, speed?: number) => {
    // Stop any current playback
    isStoppedRef.current = true;
    if (audioRef.current) {
      audioRef.current.pause();
      audioRef.current = null;
    }
    audioQueueRef.current = [];

    // Reset state
    isStoppedRef.current = false;
    isPausedRef.current = false;
    currentIndexRef.current = 0;
    voiceRef.current = voice;
    speedRef.current = speed;

    // Split text into sentences
    const sentences = splitIntoSentences(text);
    sentencesRef.current = sentences;

    setState({
      isPlaying: true,
      isSpeaking: false,
      isPaused: false,
      currentChunkIndex: 0,
      totalChunks: sentences.length,
      error: null,
    });

    options?.onStart?.();

    // Start generating and playing chunks
    await generateRemainingChunks();
  }, [generateRemainingChunks, options]);

  const pause = useCallback(() => {
    isPausedRef.current = true;
    if (audioRef.current) {
      audioRef.current.pause();
    }
    setState((prev) => ({
      ...prev,
      isPaused: true,
      isSpeaking: false,
    }));
  }, []);

  const resume = useCallback(() => {
    isPausedRef.current = false;
    setState((prev) => ({ ...prev, isPaused: false }));
    
    if (audioRef.current && audioRef.current.paused) {
      audioRef.current.play().catch(console.error);
      setState((prev) => ({ ...prev, isSpeaking: true }));
    } else if (audioQueueRef.current.length > 0) {
      playNextChunk();
    }
  }, [playNextChunk]);

  const stop = useCallback(() => {
    isStoppedRef.current = true;
    isPausedRef.current = false;
    
    if (audioRef.current) {
      audioRef.current.pause();
      audioRef.current = null;
    }
    
    audioQueueRef.current = [];
    sentencesRef.current = [];
    currentIndexRef.current = 0;

    setState({
      isPlaying: false,
      isSpeaking: false,
      isPaused: false,
      currentChunkIndex: 0,
      totalChunks: 0,
      error: null,
    });
  }, []);

  // Cleanup on unmount
  useEffect(() => {
    return () => {
      isStoppedRef.current = true;
      if (audioRef.current) {
        audioRef.current.pause();
        audioRef.current = null;
      }
    };
  }, []);

  return {
    state,
    speak,
    pause,
    resume,
    stop,
  };
}
