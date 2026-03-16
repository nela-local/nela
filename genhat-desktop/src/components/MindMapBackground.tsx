import React, { useEffect, useRef } from "react";
/**
 * Animated particle background for mindmap, inspired by the provided HTML/CSS/JS.
 * Renders a full-screen canvas with interactive particles matching the app's neon/cyan theme.
 *
 * Usage: <MindMapBackground />
 */
// Neon/cyan theme color (matches #00d4ff with some alpha)
const PARTICLE_COLOR = "rgba(0, 213, 255, 0.89)";
const PARTICLE_DISTANCE = 40;
const PARTICLE_RADIUS = 2;
const MOUSE_RADIUS = 100;
const MAX_PARTICLES = 3200;
const TARGET_FPS = 45;

interface Particle {
  x: number;
  y: number;
  baseX: number;
  baseY: number;
  size: number;
  speed: number;
}

interface MindMapBackgroundProps {
  width?: number;
  height?: number;
}

const MindMapBackground: React.FC<MindMapBackgroundProps> = ({ width, height }) => {
  const canvasRef = useRef<HTMLCanvasElement>(null);
  const animationRef = useRef<number | null>(null);
  const particlesRef = useRef<Particle[]>([]);
  const mouseRef = useRef<{ x: number | undefined; y: number | undefined }>({ x: undefined, y: undefined });
  const mouseActiveRef = useRef<boolean>(false);
  const lastFrameTimeRef = useRef<number>(0);

  // Resize and initialize particles
  const resizeParticles = () => {
    const canvas = canvasRef.current;
    if (!canvas) return;
    const w = width || window.innerWidth;
    const h = height || window.innerHeight;
    canvas.width = w;
    canvas.height = h;
    const particles: Particle[] = [];
    let count = 0;
    let reachedCap = false;
    for (
      let y = (((h - PARTICLE_DISTANCE) % PARTICLE_DISTANCE) + PARTICLE_DISTANCE) / 2;
      y < h;
      y += PARTICLE_DISTANCE
    ) {
      for (
        let x = (((w - PARTICLE_DISTANCE) % PARTICLE_DISTANCE) + PARTICLE_DISTANCE) / 2;
        x < w;
        x += PARTICLE_DISTANCE
      ) {
        if (count >= MAX_PARTICLES) {
          reachedCap = true;
          break;
        }
        particles.push({
          x,
          y,
          baseX: x,
          baseY: y,
          size: PARTICLE_RADIUS,
          speed: Math.random() * 25 + 5,
        });
        count++;
      }
      if (reachedCap) break;
    }
    particlesRef.current = particles;
  };

  // Animation loop
  const animate = (ts: number) => {
    const canvas = canvasRef.current;
    if (!canvas) return;

    const minInterval = 1000 / TARGET_FPS;
    if (ts - lastFrameTimeRef.current < minInterval) {
      animationRef.current = requestAnimationFrame(animate);
      return;
    }
    lastFrameTimeRef.current = ts;

    if (typeof document !== "undefined" && document.visibilityState === "hidden") {
      animationRef.current = requestAnimationFrame(animate);
      return;
    }

    const ctx = canvas.getContext("2d");
    if (!ctx) return;
    const w = canvas.width;
    const h = canvas.height;
    ctx.clearRect(0, 0, w, h);
    const mouse = mouseRef.current;
    const mouseActive = mouseActiveRef.current;
    for (const p of particlesRef.current) {
      // Update
      if (mouseActive && mouse.x !== undefined && mouse.y !== undefined) {
        const dx = mouse.x - p.x;
        const dy = mouse.y - p.y;
        const distance = Math.sqrt(dx * dx + dy * dy);
        const force = (MOUSE_RADIUS - distance) / MOUSE_RADIUS;
        const directionX = distance > 0.0001 ? dx / distance : 0;
        const directionY = distance > 0.0001 ? dy / distance : 0;
        if (distance < MOUSE_RADIUS) {
          p.x -= directionX * force * p.speed;
          p.y -= directionY * force * p.speed;
        } else {
          if (p.x !== p.baseX) p.x -= (p.x - p.baseX) / 10;
          if (p.y !== p.baseY) p.y -= (p.y - p.baseY) / 10;
        }
      } else {
        // Always return to base if mouse is not active
        if (p.x !== p.baseX) p.x -= (p.x - p.baseX) / 10;
        if (p.y !== p.baseY) p.y -= (p.y - p.baseY) / 10;
      }
      // Draw
      ctx.fillStyle = PARTICLE_COLOR;
      ctx.beginPath();
      ctx.arc(p.x, p.y, p.size, 0, Math.PI * 2);
      ctx.closePath();
      ctx.fill();
    }
    animationRef.current = requestAnimationFrame(animate);
  };


  useEffect(() => {
    resizeParticles();
    animationRef.current = requestAnimationFrame(animate);
    let cleanup: (() => void) | undefined;
    if (!width && !height) {
      // Fullscreen mode: use window events
      const handleWindowMouseMove = (e: MouseEvent) => {
        mouseRef.current.x = e.clientX;
        mouseRef.current.y = e.clientY;
        mouseActiveRef.current = true;
      };
      const handleWindowMouseOut = () => {
        mouseRef.current.x = undefined;
        mouseRef.current.y = undefined;
        mouseActiveRef.current = false;
      };

      window.addEventListener("resize", resizeParticles);
      window.addEventListener("mousemove", handleWindowMouseMove);
      window.addEventListener("mouseout", handleWindowMouseOut);
      cleanup = () => {
        window.removeEventListener("resize", resizeParticles);
        window.removeEventListener("mousemove", handleWindowMouseMove);
        window.removeEventListener("mouseout", handleWindowMouseOut);
      };
    } else {
      // Modal/canvas-bound mode: use window mouse events and project into canvas space.
      // This keeps particle interaction working even when another interactive layer (e.g. SVG nodes)
      // sits above this canvas and captures pointer events.
      const canvas = canvasRef.current;
      if (canvas) {
        const handleWindowMouseMove = (e: MouseEvent) => {
          const rect = canvas.getBoundingClientRect();
          const localX = e.clientX - rect.left;
          const localY = e.clientY - rect.top;
          const inside = localX >= 0 && localX <= rect.width && localY >= 0 && localY <= rect.height;

          if (inside) {
            mouseRef.current.x = localX;
            mouseRef.current.y = localY;
            mouseActiveRef.current = true;
          } else {
            mouseRef.current.x = undefined;
            mouseRef.current.y = undefined;
            mouseActiveRef.current = false;
          }
        };

        const handleWindowMouseOut = () => {
          mouseRef.current.x = undefined;
          mouseRef.current.y = undefined;
          mouseActiveRef.current = false;
        };

        window.addEventListener("mousemove", handleWindowMouseMove);
        window.addEventListener("mouseout", handleWindowMouseOut);
        cleanup = () => {
          window.removeEventListener("mousemove", handleWindowMouseMove);
          window.removeEventListener("mouseout", handleWindowMouseOut);
        };
      }
    }
    return () => {
      if (cleanup) cleanup();
      if (animationRef.current !== null) {
        cancelAnimationFrame(animationRef.current);
        animationRef.current = null;
      }
    };
    // eslint-disable-next-line
  }, [width, height]);

  return (
    <canvas
      ref={canvasRef}
      style={{
        position: "absolute",
        left: 0,
        top: 0,
        width: width ? width : "100vw",
        height: height ? height : "100vh",
        zIndex: 1,
        pointerEvents: width && height ? "auto" : "none",
        borderRadius: width && height ? 18 : undefined,
      }}
    />
  );
};

export default MindMapBackground;
