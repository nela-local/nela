/**
 * useAudioRecorder — Real-time audio recording hook using Web Audio API
 * 
 * Records audio from the microphone and converts it to WAV format
 * suitable for the Parakeet ASR model (16kHz mono).
 */

import { useState, useRef, useCallback, useEffect } from "react";

export interface AudioRecorderState {
  isRecording: boolean;
  isPaused: boolean;
  duration: number;
  error: string | null;
}

export interface UseAudioRecorderReturn {
  state: AudioRecorderState;
  startRecording: () => Promise<void>;
  stopRecording: () => Promise<Blob | null>;
  pauseRecording: () => void;
  resumeRecording: () => void;
  cancelRecording: () => void;
}

/** Target sample rate for Parakeet ASR */
const TARGET_SAMPLE_RATE = 16000;

/**
 * Resample audio data from source sample rate to target sample rate using linear interpolation.
 */
function resampleAudio(
  samples: Float32Array,
  sourceSampleRate: number,
  targetSampleRate: number
): Float32Array {
  if (sourceSampleRate === targetSampleRate) {
    return samples;
  }

  const ratio = sourceSampleRate / targetSampleRate;
  const newLength = Math.round(samples.length / ratio);
  const result = new Float32Array(newLength);

  for (let i = 0; i < newLength; i++) {
    const srcIndex = i * ratio;
    const srcIndexFloor = Math.floor(srcIndex);
    const srcIndexCeil = Math.min(srcIndexFloor + 1, samples.length - 1);
    const frac = srcIndex - srcIndexFloor;

    result[i] = samples[srcIndexFloor] * (1 - frac) + samples[srcIndexCeil] * frac;
  }

  return result;
}

/**
 * Convert Float32Array audio samples to WAV Blob.
 */
function float32ToWav(samples: Float32Array, sampleRate: number): Blob {
  const numChannels = 1;
  const bitsPerSample = 16;
  const bytesPerSample = bitsPerSample / 8;
  const blockAlign = numChannels * bytesPerSample;

  const dataLength = samples.length * bytesPerSample;
  const buffer = new ArrayBuffer(44 + dataLength);
  const view = new DataView(buffer);

  // RIFF header
  writeString(view, 0, "RIFF");
  view.setUint32(4, 36 + dataLength, true);
  writeString(view, 8, "WAVE");

  // fmt sub-chunk
  writeString(view, 12, "fmt ");
  view.setUint32(16, 16, true); // Sub-chunk size
  view.setUint16(20, 1, true); // Audio format (PCM)
  view.setUint16(22, numChannels, true);
  view.setUint32(24, sampleRate, true);
  view.setUint32(28, sampleRate * blockAlign, true);
  view.setUint16(32, blockAlign, true);
  view.setUint16(34, bitsPerSample, true);

  // data sub-chunk
  writeString(view, 36, "data");
  view.setUint32(40, dataLength, true);

  // Convert float samples to 16-bit PCM
  let offset = 44;
  for (let i = 0; i < samples.length; i++) {
    const sample = Math.max(-1, Math.min(1, samples[i]));
    const intSample = sample < 0 ? sample * 0x8000 : sample * 0x7fff;
    view.setInt16(offset, intSample, true);
    offset += 2;
  }

  return new Blob([buffer], { type: "audio/wav" });
}

function writeString(view: DataView, offset: number, str: string): void {
  for (let i = 0; i < str.length; i++) {
    view.setUint8(offset + i, str.charCodeAt(i));
  }
}

export function useAudioRecorder(): UseAudioRecorderReturn {
  const [state, setState] = useState<AudioRecorderState>({
    isRecording: false,
    isPaused: false,
    duration: 0,
    error: null,
  });

  const mediaStreamRef = useRef<MediaStream | null>(null);
  const audioContextRef = useRef<AudioContext | null>(null);
  const processorRef = useRef<ScriptProcessorNode | null>(null);
  const sourceRef = useRef<MediaStreamAudioSourceNode | null>(null);
  const chunksRef = useRef<Float32Array[]>([]);
  const durationIntervalRef = useRef<number | null>(null);
  const startTimeRef = useRef<number>(0);

  const cleanup = useCallback(() => {
    if (durationIntervalRef.current) {
      clearInterval(durationIntervalRef.current);
      durationIntervalRef.current = null;
    }
    if (processorRef.current) {
      processorRef.current.disconnect();
      processorRef.current = null;
    }
    if (sourceRef.current) {
      sourceRef.current.disconnect();
      sourceRef.current = null;
    }
    if (audioContextRef.current && audioContextRef.current.state !== "closed") {
      audioContextRef.current.close();
      audioContextRef.current = null;
    }
    if (mediaStreamRef.current) {
      mediaStreamRef.current.getTracks().forEach((track) => track.stop());
      mediaStreamRef.current = null;
    }
    chunksRef.current = [];
  }, []);

  useEffect(() => {
    return cleanup;
  }, [cleanup]);

  const startRecording = useCallback(async () => {
    try {
      cleanup();

      // Check if mediaDevices API is available (may be undefined in Tauri WebView)
      if (!navigator.mediaDevices || !navigator.mediaDevices.getUserMedia) {
        throw new Error(
          "Microphone access is not available. Please ensure microphone permissions are granted in System Settings > Privacy & Security > Microphone."
        );
      }

      // Request microphone access
      const stream = await navigator.mediaDevices.getUserMedia({
        audio: {
          channelCount: 1,
          sampleRate: { ideal: TARGET_SAMPLE_RATE },
          echoCancellation: true,
          noiseSuppression: true,
        },
      });

      mediaStreamRef.current = stream;

      // Create audio context
      const audioContext = new AudioContext();
      audioContextRef.current = audioContext;

      // Create source from microphone
      const source = audioContext.createMediaStreamSource(stream);
      sourceRef.current = source;

      // Use ScriptProcessorNode to capture raw audio data
      const bufferSize = 4096;
      const processor = audioContext.createScriptProcessor(bufferSize, 1, 1);
      processorRef.current = processor;

      processor.onaudioprocess = (e) => {
        const inputData = e.inputBuffer.getChannelData(0);
        // Copy the data (it gets reused by the audio API)
        chunksRef.current.push(new Float32Array(inputData));
      };

      source.connect(processor);
      processor.connect(audioContext.destination);

      startTimeRef.current = Date.now();
      durationIntervalRef.current = window.setInterval(() => {
        setState((prev) => ({
          ...prev,
          duration: Math.floor((Date.now() - startTimeRef.current) / 1000),
        }));
      }, 1000);

      setState({
        isRecording: true,
        isPaused: false,
        duration: 0,
        error: null,
      });
    } catch (err) {
      const errorMessage =
        err instanceof Error ? err.message : "Failed to access microphone";
      setState((prev) => ({
        ...prev,
        error: errorMessage,
      }));
      cleanup();
    }
  }, [cleanup, state.isPaused]);

  const stopRecording = useCallback(async (): Promise<Blob | null> => {
    if (!audioContextRef.current || chunksRef.current.length === 0) {
      cleanup();
      setState({
        isRecording: false,
        isPaused: false,
        duration: 0,
        error: null,
      });
      return null;
    }

    const sourceSampleRate = audioContextRef.current.sampleRate;

    // Concatenate all chunks
    const totalLength = chunksRef.current.reduce(
      (acc, chunk) => acc + chunk.length,
      0
    );
    const fullAudio = new Float32Array(totalLength);
    let offset = 0;
    for (const chunk of chunksRef.current) {
      fullAudio.set(chunk, offset);
      offset += chunk.length;
    }

    // Resample to 16kHz for Parakeet ASR
    const resampled = resampleAudio(fullAudio, sourceSampleRate, TARGET_SAMPLE_RATE);

    // Convert to WAV
    const wavBlob = float32ToWav(resampled, TARGET_SAMPLE_RATE);

    cleanup();
    setState({
      isRecording: false,
      isPaused: false,
      duration: 0,
      error: null,
    });

    return wavBlob;
  }, [cleanup]);

  const pauseRecording = useCallback(() => {
    setState((prev) => ({ ...prev, isPaused: true }));
  }, []);

  const resumeRecording = useCallback(() => {
    setState((prev) => ({ ...prev, isPaused: false }));
  }, []);

  const cancelRecording = useCallback(() => {
    cleanup();
    setState({
      isRecording: false,
      isPaused: false,
      duration: 0,
      error: null,
    });
  }, [cleanup]);

  return {
    state,
    startRecording,
    stopRecording,
    pauseRecording,
    resumeRecording,
    cancelRecording,
  };
}
