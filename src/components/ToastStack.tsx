import { useCallback, useRef, useState } from "react";
import { X } from "lucide-react";
import { theme } from "../ui/theme";

export type ToastKind = "error" | "warning";

export type ToastItem = { id: number; kind: ToastKind; message: string };

const AUTO_DISMISS_MS = 5200;

export function useToastStack() {
  const [toasts, setToasts] = useState<ToastItem[]>([]);
  const idRef = useRef(0);

  const pushToast = useCallback((kind: ToastKind, message: string) => {
    const text = message.trim();
    if (!text) return;
    const id = ++idRef.current;
    setToasts((prev) => [...prev, { id, kind, message: text }]);
    window.setTimeout(() => {
      setToasts((prev) => prev.filter((t) => t.id !== id));
    }, AUTO_DISMISS_MS);
  }, []);

  const dismiss = useCallback((id: number) => {
    setToasts((prev) => prev.filter((t) => t.id !== id));
  }, []);

  const stack = (
    <div
      className="cs-toast-stack"
      style={{
        position: "fixed",
        right: theme.space[3],
        bottom: theme.space[3],
        zIndex: 9000,
        display: "flex",
        flexDirection: "column-reverse",
        gap: theme.space[2],
        maxWidth: "min(440px, calc(100vw - 24px))",
        pointerEvents: "none",
      }}
    >
      {toasts.map((t, i) => (
        <div
          key={t.id}
          style={{
            pointerEvents: "auto",
            transform: `translateY(${-i * 6}px)`,
            boxShadow: theme.shell.contextMenuShadow,
          }}
        >
          <div
            role="status"
            style={{
              display: "flex",
              alignItems: "flex-start",
              gap: theme.space[2],
              padding: `${theme.space[2]}px ${theme.space[3]}px`,
              borderRadius: theme.radius.md,
              border: `1px solid ${
                t.kind === "error"
                  ? theme.shell.errorBannerBorder
                  : theme.shell.warningBannerBorder
              }`,
              background:
                t.kind === "error"
                  ? theme.shell.errorBannerBg
                  : theme.shell.warningBannerBg,
              color:
                t.kind === "error" ? theme.text.error : theme.text.warning,
              fontSize: "13px",
              lineHeight: 1.45,
            }}
          >
            <span style={{ flex: 1 }}>{t.message}</span>
            <button
              type="button"
              className="cs-toast-dismiss"
              aria-label="Dismiss notification"
              onClick={() => dismiss(t.id)}
              style={{
                flexShrink: 0,
                border: "none",
                background: "rgba(255,255,255,0.08)",
                color: "inherit",
                borderRadius: theme.radius.sm,
                width: 26,
                height: 26,
                display: "flex",
                alignItems: "center",
                justifyContent: "center",
                cursor: "pointer",
              }}
            >
              <X size={14} strokeWidth={2} />
            </button>
          </div>
        </div>
      ))}
    </div>
  );

  return { pushToast, toastStack: stack };
}
