import React, { createContext, useCallback, useContext, useState } from "react";

// ── Toast ─────────────────────────────────────────────────────────────────────

type Toast = { id: number; kind: "ok" | "err"; text: string };
const ToastCtx = createContext<(kind: "ok" | "err", text: string) => void>(() => {});
export const useToast = () => useContext(ToastCtx);

export function ToastProvider({ children }: { children: React.ReactNode }) {
  const [toasts, setToasts] = useState<Toast[]>([]);
  const push = useCallback((kind: "ok" | "err", text: string) => {
    const id = Date.now() + Math.floor(Math.random() * 1000);
    setToasts((t) => [...t, { id, kind, text }]);
    setTimeout(() => setToasts((t) => t.filter((x) => x.id !== id)), 3600);
  }, []);
  return (
    <ToastCtx.Provider value={push}>
      {children}
      <div className="fixed bottom-5 right-5 z-50 flex flex-col gap-2">
        {toasts.map((t) => (
          <div
            key={t.id}
            className={`badge max-w-sm px-3.5 py-2.5 text-sm shadow-lg ${
              t.kind === "ok" ? "bg-emerald-600 text-white" : "bg-red-600 text-white"
            }`}
          >
            {t.text}
          </div>
        ))}
      </div>
    </ToastCtx.Provider>
  );
}

// ── Spinner ───────────────────────────────────────────────────────────────────

export function Spinner({ label }: { label?: string }) {
  return (
    <span className="inline-flex items-center gap-2 text-slate-500">
      <span className="h-4 w-4 animate-spin rounded-full border-2 border-slate-300 border-t-brand" />
      {label}
    </span>
  );
}

// ── Modal ─────────────────────────────────────────────────────────────────────

export function Modal({
  title,
  onClose,
  children,
  wide,
}: {
  title: string;
  onClose: () => void;
  children: React.ReactNode;
  wide?: boolean;
}) {
  return (
    <div
      className="fixed inset-0 z-40 flex items-center justify-center bg-black/30 p-4"
      onClick={onClose}
    >
      <div
        className={`card w-full ${wide ? "max-w-2xl" : "max-w-lg"} max-h-[85vh] overflow-auto p-5`}
        onClick={(e) => e.stopPropagation()}
      >
        <div className="mb-4 flex items-center justify-between">
          <h3 className="text-base font-semibold text-slate-100">{title}</h3>
          <button
            className="text-slate-400 hover:text-slate-100 transition"
            onClick={onClose}
          >
            ✕
          </button>
        </div>
        {children}
      </div>
    </div>
  );
}
