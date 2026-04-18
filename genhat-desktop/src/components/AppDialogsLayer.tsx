import type { DownloadStateMap } from "../app/types";
import type {
  ImportModelProfile,
  RegisteredModel,
} from "../types";
import AppModal, { type AppModalKind } from "./AppModal";
import HuggingFaceModal from "./HuggingFaceModal";
import ModelsSettingsModal from "./ModelsSettingsModal";
import StartupModal from "./StartupModal";
import ToursModal from "./ToursModal";

interface AppModalState {
  open: boolean;
  kind: AppModalKind;
  title: string;
  message: string;
  confirmLabel: string;
  cancelLabel: string;
  showCancel: boolean;
}

interface AppDialogsLayerProps {
  showStartupModal: boolean;
  onContinueWorkspace: () => void;
  canContinueWorkspace: boolean;
  continueWorkspaceName: string | null;
  onNewProject: () => void;
  onImportProject: () => void;
  onStartTour: () => void;
  workspaceBusy: boolean;
  appModal: AppModalState;
  onModalConfirm: () => void;
  onModalCancel: () => void;
  settingsOpen: boolean;
  onCloseSettings: () => void;
  registeredModels: RegisteredModel[];
  modelCatalog: RegisteredModel[];
  onModelsUpdated: () => Promise<RegisteredModel[]>;
  downloads: DownloadStateMap;
  onDownloadModel: (modelId: string) => Promise<void>;
  onCancelDownload: (modelId: string) => Promise<void>;
  onUninstallModel: (modelId: string) => Promise<void>;
  onDownloadMissingOptional: () => Promise<void>;
  onConfirmAction: (
    title: string,
    message: string,
    confirmLabel?: string,
    cancelLabel?: string
  ) => Promise<boolean>;
  activeWorkspaceId?: string;
  hfModalOpen: boolean;
  onCloseHfModal: () => void;
  hfModalPreset: {
    folder: string;
    profile: "none" | ImportModelProfile;
  };
  onModelImported: () => Promise<RegisteredModel[]>;
  toursOpen: boolean;
  onCloseTours: () => void;
}

export default function AppDialogsLayer({
  showStartupModal,
  onContinueWorkspace,
  canContinueWorkspace,
  continueWorkspaceName,
  onNewProject,
  onImportProject,
  onStartTour,
  workspaceBusy,
  appModal,
  onModalConfirm,
  onModalCancel,
  settingsOpen,
  onCloseSettings,
  registeredModels,
  modelCatalog,
  onModelsUpdated,
  downloads,
  onDownloadModel,
  onCancelDownload,
  onUninstallModel,
  onDownloadMissingOptional,
  onConfirmAction,
  activeWorkspaceId,
  hfModalOpen,
  onCloseHfModal,
  hfModalPreset,
  onModelImported,
  toursOpen,
  onCloseTours,
}: AppDialogsLayerProps) {
  return (
    <>
      {showStartupModal && (
        <StartupModal
          onContinueWorkspace={onContinueWorkspace}
          canContinueWorkspace={canContinueWorkspace}
          continueWorkspaceName={continueWorkspaceName}
          onNewProject={onNewProject}
          onImportProject={onImportProject}
          onStartTour={onStartTour}
          busy={workspaceBusy}
        />
      )}

      <AppModal
        isOpen={appModal.open}
        kind={appModal.kind}
        title={appModal.title}
        message={appModal.message}
        confirmLabel={appModal.confirmLabel}
        cancelLabel={appModal.cancelLabel}
        showCancel={appModal.showCancel}
        onConfirm={onModalConfirm}
        onCancel={onModalCancel}
      />

      <ModelsSettingsModal
        isOpen={settingsOpen}
        onClose={onCloseSettings}
        models={registeredModels}
        modelCatalog={modelCatalog}
        onModelsUpdated={onModelsUpdated}
        downloads={downloads}
        onDownload={onDownloadModel}
        onCancelDownload={onCancelDownload}
        onUninstall={onUninstallModel}
        onDownloadMissingOptional={onDownloadMissingOptional}
        onConfirm={onConfirmAction}
        workspaceId={activeWorkspaceId}
      />

      <HuggingFaceModal
        isOpen={hfModalOpen}
        onClose={onCloseHfModal}
        onModelImported={onModelImported}
        defaultFolder={hfModalPreset.folder}
        defaultImportProfile={hfModalPreset.profile}
      />

      <ToursModal isOpen={toursOpen} onClose={onCloseTours} />
    </>
  );
}
