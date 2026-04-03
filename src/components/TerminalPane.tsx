import { useEffect, useRef } from "react";
import { Terminal } from "@xterm/xterm";
import { FitAddon } from "@xterm/addon-fit";
import { invoke } from "@tauri-apps/api/core";
import { listen } from "@tauri-apps/api/event";
import "@xterm/xterm/css/xterm.css";
import { theme } from "../ui/theme";

type Props = {
  projectRoot: string | null;
};

export function TerminalPane({ projectRoot }: Props) {
  const containerRef = useRef<HTMLDivElement | null>(null);
  const termRef = useRef<Terminal | null>(null);
  const fitRef = useRef<FitAddon | null>(null);
  const sessionIdRef = useRef<number | null>(null);

  useEffect(() => {
    const t = theme.terminal;
    const term = new Terminal({
      fontFamily: theme.font.mono,
      fontSize: 12,
      // Large scrollback + megabytes from `cargo run` can exhaust WebView memory.
      scrollback: 800,
      theme: {
        background: t.background,
        foreground: t.foreground,
        cursor: t.cursor,
        cursorAccent: t.cursorAccent,
        selectionBackground: t.selection,
        black: t.black,
      },
    });
    const fit = new FitAddon();
    term.loadAddon(fit);
    termRef.current = term;
    fitRef.current = fit;

    let resizeObserver: ResizeObserver | undefined;
    if (containerRef.current) {
      term.open(containerRef.current);
      fit.fit();
      resizeObserver = new ResizeObserver(() => {
        if (containerRef.current && fitRef.current && termRef.current && sessionIdRef.current != null) {
          fitRef.current.fit();
          void invoke("resize_pty", {
            sessionId: sessionIdRef.current,
            cols: termRef.current.cols,
            rows: termRef.current.rows,
          });
        }
      });
      resizeObserver.observe(containerRef.current);
    }

    let unlistenData: (() => void) | undefined;
    let unlistenExit: (() => void) | undefined;

    (async () => {
      const sessionId = await invoke<number>("create_pty", {
        shell: null,
        cwd: projectRoot,
      });
      sessionIdRef.current = sessionId;

      unlistenData = await listen<{
        sessionId: number;
        data: string;
      }>("pty-data", (event) => {
        if (event.payload.sessionId === sessionIdRef.current) {
          term.write(event.payload.data);
        }
      });

      unlistenExit = await listen<{
        sessionId: number;
        code: number;
      }>("pty-exit", (event) => {
        if (event.payload.sessionId === sessionIdRef.current) {
          // Only surface non-zero exit codes; for a normal shell exit
          // just leave the last prompt/output as-is.
          if (event.payload.code !== 0) {
            term.writeln(`\r\n[process exited with code ${event.payload.code}]`);
          }
        }
      });

      // Forward all keyboard data directly to the PTY. The shell running
      // inside the PTY handles line editing and decides when to execute.
      term.onData((data) => {
        if (sessionIdRef.current != null) {
          void invoke("write_pty", {
            sessionId: sessionIdRef.current,
            data,
          });
        }
      });
    })();

    const handleResize = () => {
      if (containerRef.current && fitRef.current && termRef.current && sessionIdRef.current != null) {
        fitRef.current.fit();
        const cols = termRef.current.cols;
        const rows = termRef.current.rows;
        void invoke("resize_pty", {
          sessionId: sessionIdRef.current,
          cols,
          rows,
        });
      }
    };

    window.addEventListener("resize", handleResize);

    return () => {
      resizeObserver?.disconnect();
      window.removeEventListener("resize", handleResize);
      if (unlistenData) unlistenData();
      if (unlistenExit) unlistenExit();
      if (sessionIdRef.current != null) {
        void invoke("close_pty", { sessionId: sessionIdRef.current });
      }
      term.dispose();
    };
  }, [projectRoot]);

  return (
    <div
      ref={containerRef}
      style={{
        width: "100%",
        height: "100%",
      }}
    />
  );
}

