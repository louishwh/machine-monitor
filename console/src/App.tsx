import { useEffect, useState } from "react";
import { ToastProvider } from "./ui";
import ConnectPage from "./pages/ConnectPage";
import MachinesPage from "./pages/MachinesPage";
import {
  listServers,
  setActiveServer,
  errMsg,
  type ServerProfile,
} from "./api";

type View = "connect" | "machines";

export default function App() {
  const [view, setView] = useState<View>("machines");
  const [servers, setServers] = useState<ServerProfile[]>([]);
  const [activeServerId, setActiveServerId] = useState<string | null>(null);

  // Load server list on mount (also triggers after ConnectPage saves changes).
  function reloadServers() {
    listServers()
      .then((list) => {
        setServers(list);
        // Find the active one (first with matching id in the list after potential
        // migration; we read it via list — the backend set_active_server_cmd keeps
        // it consistent, and get_active_server_cmd is called here indirectly via
        // the fact that list_servers_cmd calls ensure_migrated).
        // We'll sync activeServerId from the list: pick the id stored in keychain
        // by calling getActiveServer separately below.
      })
      .catch(() => {});
    import("./api")
      .then(({ getActiveServer }) => getActiveServer())
      .then((p) => {
        if (p) setActiveServerId(p.id);
      })
      .catch(() => {});
  }

  useEffect(() => {
    reloadServers();
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, []);

  async function handleServerChange(id: string) {
    try {
      await setActiveServer(id);
      setActiveServerId(id);
    } catch (e) {
      console.error("切换服务端失败:", errMsg(e));
    }
  }

  const activeProfile = servers.find((s) => s.id === activeServerId) ?? null;

  return (
    <ToastProvider>
      <div className="flex h-full">
        {/* ── Sidebar ─────────────────────────────────────────────── */}
        <nav className="flex w-44 flex-col bg-slate-900 border-r border-slate-700 py-4 gap-1">
          <div className="px-4 pb-3 border-b border-slate-700 mb-2">
            <h1 className="text-sm font-bold text-slate-100">FleetWatch</h1>
            <p className="text-xs text-slate-500 mt-0.5">管理端</p>

            {/* Server dropdown */}
            <div className="mt-2">
              {servers.length === 0 ? (
                <p className="text-xs text-slate-600 italic">未配置服务端</p>
              ) : (
                <select
                  className="w-full rounded bg-slate-800 border border-slate-700 text-slate-300 text-xs px-1.5 py-1 focus:outline-none focus:border-slate-500"
                  value={activeServerId ?? ""}
                  onChange={(e) => handleServerChange(e.target.value)}
                >
                  {servers.map((s) => (
                    <option key={s.id} value={s.id}>
                      {s.name}
                    </option>
                  ))}
                </select>
              )}
              {activeProfile && (
                <p className="text-xs text-slate-600 truncate mt-0.5 font-mono">
                  {activeProfile.url}
                </p>
              )}
            </div>
          </div>

          <NavItem
            label="机器"
            icon="🖥"
            active={view === "machines"}
            onClick={() => setView("machines")}
          />
          <NavItem
            label="连接 & 配对"
            icon="🔗"
            active={view === "connect"}
            onClick={() => setView("connect")}
          />
        </nav>

        {/* ── Main ────────────────────────────────────────────────── */}
        <main className="flex-1 overflow-auto">
          {view === "connect" && (
            <ConnectPage
              onServersChanged={reloadServers}
              activeServerId={activeServerId}
              onActiveChange={handleServerChange}
            />
          )}
          {view === "machines" && (
            // key forces MachinesPage to remount (and re-fetch) when active server changes.
            <MachinesPage key={activeServerId ?? "__none__"} />
          )}
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
