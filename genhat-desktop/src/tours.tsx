import type { TourDefinition } from "./hooks/useTour";

export const TOURS: TourDefinition[] = [
  {
    id: "getting-started",
    name: "Getting Started (Overview)",
    version: 1,
    steps: [
      {
        id: "sidebar",
        title: "Navigate the app",
        body: (
          <span>
            Use the sidebar to switch between chats, audio files, and mindmaps.
          </span>
        ),
        target: '[data-tour="sidebar-chats"]',
        placement: "right",
      },
      {
        id: "workspaces",
        title: "Workspaces",
        body: (
          <span>
            Workspaces keep your sessions and files organized. Create one to begin, or switch between existing ones.
          </span>
        ),
        target: '[data-tour="workspace-selector"]',
        placement: "bottom",
      },
      {
        id: "chat-tabs",
        title: "Multiple chats",
        body: (
          <span>
            You can open multiple chat sessions and switch between them with tabs.
          </span>
        ),
        target: '[data-tour="chat-tabs"]',
        placement: "bottom",
      },
      {
        id: "chat-input",
        title: "Ask Nela anything",
        body: (
          <span>
            Type your question here and press Enter to send. Use Shift+Enter for a new line.
          </span>
        ),
        target: '[data-tour="chat-input"]',
        placement: "top",
      },
      {
        id: "mode-switch",
        title: "Switch modes",
        body: (
          <span>
            Nela supports different modes (Chat, Vision, Audio, Podcast, Mindmap). Use this menu to switch.
          </span>
        ),
        target: '[data-tour="mode-switch"]',
        placement: "top",
      },
      {
        id: "models",
        title: "Models",
        body: (
          <span>
            You can install and switch models depending on the mode you’re using.
          </span>
        ),
        target: '[data-tour="model-selector-llm"]',
        placement: "bottom",
      },
      {
        id: "settings",
        title: "Settings",
        body: (
          <span>
            Settings lets you manage models, downloads, and advanced parameters.
          </span>
        ),
        target: '[data-tour="sidebar-settings"]',
        placement: "right",
      },
      {
        id: "parameter-help",
        title: "Parameter help",
        body: (
          <span>
            Not sure what a model parameter means? Click the small <strong>?</strong> next to a parameter name for a plain-language explanation.
          </span>
        ),
        target: '[data-tour="runtime-param-help"]',
        placement: "left",
        centerTooltip: true,
      },
      {
        id: "help-tours",
        title: "Feature tours",
        body: (
          <span>
            You can revisit tours anytime from Help → Tours.
          </span>
        ),
        target: '[data-tour="sidebar-help-tours"]',
        placement: "right",
      },
    ],
  },
  {
    id: "models",
    name: "Models & Downloads",
    version: 1,
    steps: [
      {
        id: "model-selector",
        title: "Switch models",
        body: <span>Use this selector to switch between installed models for the current mode.</span>,
        target: '[data-tour="model-selector-llm"]',
        placement: "bottom",
      },
      {
        id: "settings",
        title: "Manage models",
        body: <span>Open Settings to manage model downloads, optional models, and runtime parameters.</span>,
        target: '[data-tour="sidebar-settings"]',
        placement: "right",
      },
    ],
  },
  {
    id: "mindmaps",
    name: "Mindmaps",
    version: 1,
    steps: [
      {
        id: "sidebar-mindmaps",
        title: "Mindmaps sidebar",
        body: <span>Open Mindmaps to browse and reopen previously generated graphs.</span>,
        target: '[data-tour="sidebar-mindmaps"]',
        placement: "right",
      },
    ],
  },
  {
    id: "documents",
    name: "Documents (RAG)",
    version: 1,
    steps: [
      {
        id: "attach",
        title: "Add documents",
        body: <span>Add files or folders to build a local knowledge base for retrieval.</span>,
        target: '[data-tour="attach-button"]',
        placement: "top",
      },
    ],
  },
];
