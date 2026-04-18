import AudioPlayer from "./AudioPlayer";
import type { ChatSession } from "../types";

interface AudioSidebarProps {
  sessions: ChatSession[];
  onDeleteAudio: (sessionId: string, msgIdx: number) => void;
}

export default function AudioSidebar({ sessions, onDeleteAudio }: AudioSidebarProps) {
  const allAudio = sessions.flatMap((session) =>
    session.messages
      .map((msg, idx, arr) => {
        if (!msg.audioUrl || msg.audioSaved === false) return null;

        let userMsg = null;
        for (let i = idx - 1; i >= 0; i -= 1) {
          if (arr[i].role === "user") {
            userMsg = arr[i];
            break;
          }
        }

        return {
          audioUrl: msg.audioUrl,
          sessionId: session.id,
          sessionTitle: session.title,
          msgIdx: idx,
          userQuery: userMsg ? userMsg.content : "(Unknown query)",
        };
      })
      .filter(Boolean)
  );

  const filteredAudio = allAudio.filter(Boolean);

  return (
    <aside className="w-[280px] min-w-[280px] border-r border-glass-border bg-void-800/80 backdrop-blur-xl flex flex-col">
      <div className="h-10 px-4 flex items-center justify-between shrink-0">
        <div className="flex items-center gap-2 text-txt">
          <span className="text-2xl font-semibold mt-2">Audio</span>
        </div>
      </div>

      <div className="flex-1 p-2 flex flex-col">
        <div className="flex-1 bg-void-900 border border-glass-border rounded-xl p-2 flex flex-col gap-2 shadow-md overflow-y-auto overflow-x-hidden">
          {filteredAudio.length > 0 ? (
            <ul className="flex flex-col gap-2">
              {filteredAudio.map((item) => (
                <li key={item!.audioUrl} className="flex flex-col gap-1 group relative">
                  <span className="text-[0.82rem] font-medium truncate">{item!.userQuery}</span>
                  <span className="text-[0.75rem] text-txt-muted truncate">{item!.sessionTitle}</span>
                  <AudioPlayer src={item!.audioUrl} barCount={20} />
                  <button
                    className="absolute top-1 right-1 opacity-60 group-hover:opacity-100 transition-opacity text-danger hover:text-danger/80"
                    title="Delete audio"
                    onClick={() => onDeleteAudio(item!.sessionId, item!.msgIdx)}
                  >
                    <svg width="16" height="16" viewBox="0 0 24 24" fill="none" stroke="currentColor" strokeWidth="2" strokeLinecap="round" strokeLinejoin="round">
                      <line x1="18" y1="6" x2="6" y2="18" />
                      <line x1="6" y1="6" x2="18" y2="18" />
                    </svg>
                  </button>
                </li>
              ))}
            </ul>
          ) : (
            <div className="text-[0.9rem] text-txt-muted">No audio generated yet for any session.</div>
          )}
        </div>
      </div>
    </aside>
  );
}
