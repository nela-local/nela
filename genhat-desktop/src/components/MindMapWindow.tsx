import React from "react";
import MindMapBackground from "./MindMapBackground";

interface MindMapWindowProps {
  onClose: () => void;
}

const MindMapWindow: React.FC<MindMapWindowProps> = ({ onClose }) => {
  return (
    <div
      style={{
        position: "fixed",
        top: 0,
        left: 0,
        width: "100vw",
        height: "100vh",
        zIndex: 1000,
        display: "flex",
        alignItems: "center",
        justifyContent: "center",
        background: "rgba(13,13,13,0.32)", // semi-transparent overlay
      }}
    >
      <div
        style={{
          position: "relative",
          width: 900,
          height: 650,
          background: "#181c20",
          borderRadius: 18,
          boxShadow: "0 8px 32px rgba(0,0,0,0.5)",
          border: "3px solid rgba(0, 213, 255, 0.89)",
          overflow: "hidden",
          display: "flex",
          flexDirection: "column",
          alignItems: "center",
          justifyContent: "center",
        }}
      >
        {/* Animated background inside modal */}
        <div style={{
          position: "absolute",
          left: 0,
          top: 0,
          width: "100%",
          height: "100%",
          zIndex: 1,
          pointerEvents: "none",
        }}>
          <MindMapBackground width={900} height={650} />
        </div>
        {/* Close button */}
        <div
          style={{
            position: "absolute",
            top: 18,
            right: 24,
            zIndex: 2,
          }}
        >
          <button
            style={{
              background: "#222",
              color: "#ff8c00",
              border: "1px solid #ff8c00",
              borderRadius: 8,
              padding: "8px 16px",
              fontWeight: 600,
              fontSize: 15,
              cursor: "pointer",
              boxShadow: "0 2px 8px rgba(0,0,0,0.2)",
            }}
            onClick={onClose}
          >
            Close Mind Map
          </button>
        </div>
        {/* Content */}
      </div>
    </div>
  );
};

export default MindMapWindow;
