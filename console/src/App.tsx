import { useState } from "react";
import { ToastProvider } from "./ui";
import ConnectPage from "./pages/ConnectPage";
import MachinesPage from "./pages/MachinesPage";

type View = "connect" | "machines";

export default function App() {
  const [view, setView] = useState<View>("connect");

  return (
    <ToastProvider>
      <div className="flex h-full">
        {/* ── Sidebar ─────────────────────────────────────────────── */}
        <nav className="flex w-44 flex-col bg-slate-900 border-r border-slate-700 py-4 gap-1">
          <div className="px-4 pb-3 border-b border-slate-700 mb-2">
            <h1 className="text-sm font-bold text-slate-100">FleetWatch</h1>
            <p className="text-xs text-slate-500 mt-0.5">管理端</p>
          </div>
          <NavItem
            label="连接 & 配对"
            icon="🔗"
            active={view === "connect"}
            onClick={() => setView("connect")}
          />
          <NavItem
            label="机器"
            icon="🖥"
            active={view === "machines"}
            onClick={() => setView("machines")}
          />
        </nav>

        {/* ── Main ────────────────────────────────────────────────── */}
        <main className="flex-1 overflow-auto">
          {view === "connect" && <ConnectPage />}
          {view === "machines" && <MachinesPage />}
        </main>
      </div>
    </ToastProvider>
  );
}

function NavItem({
  label,
  icon,
  active,
  onClick,
}: {
  label: string;
  icon: string;
  active: boolean;
  onClick: () => void;
}) {
  return (
    <button
      onClick={onClick}
      className={`mx-2 flex items-center gap-2 rounded-lg px-3 py-2 text-sm transition ${
        active
          ? "bg-slate-700 text-slate-100 font-medium"
          : "text-slate-400 hover:bg-slate-800 hover:text-slate-200"
      }`}
    >
      <span>{icon}</span>
      <span>{label}</span>
    </button>
  );
}
