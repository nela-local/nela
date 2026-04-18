export interface DownloadProgress {
  progress: number;
  status: string;
  speedBps?: number;
}

export type DownloadStateMap = Record<string, DownloadProgress>;

export interface StartupModelToastState {
  open: boolean;
  phase: "prompt" | "downloading" | "done" | "declined" | "info";
  message: string;
  missingIds: string[];
  missingNames: string[];
  missingSizesMb: number[];
  selectedIds: string[];
  doneIds: string[];
  failedIds: string[];
  completed: number;
  total: number;
  failed: number;
}
