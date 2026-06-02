import { useCallback, useEffect, useRef, useState } from "react";
import {
  listMachines,
  issueMachine,
  machineStatus,
  machineSnapshots,
  getServerUrl,
  errMsg,
  type Machine,
} from "../api";
import { Modal, Spinner, useToast } from "../ui";

// ── IssueMachineModal ─────────────────────────────────────────────────────────

function IssueMachineModal({ onClose, serverUrl }: { onClose: () => void; serverUrl: string }) {
  const toast = useToast();
  const [name, setName] = useState("");
  const [busy, setBusy] = useState(false);
  const [issued, setIssued] = useState<{ machineId: string; name: string; token: string } | null>(
    null
  );

  async function handleIssue() {
    if (!name.trim()) {
      toast("err", "请输入机器名称");
      return;
    }
    setBusy(true);
    try {
      const result = await issueMachine(name.trim());
      setIssued(result);
    } catch (e) {
      toast("err", errMsg(e));
    } finally {
      setBusy(false);
    }
  }

  function copyToken() {
    if (!issued) return;
    navigator.clipboard.writeText(issued.token).then(
      () => toast("ok", "令牌已复制"),
      () => toast("err", "复制失败，请手动复制")
    );
  }

  const enrollCmd = issued
    ? `fleetwatch-agent enroll --server ${serverUrl || "<server_url>"} --identity ${issued.token}`
    : "";

  function copyCmd() {
    navigator.clipboard.writeText(enrollCmd).then(
      () => toast("ok", "命令已复制"),
      () => toast("err", "复制失败，请手动复制")
    );
  }

  return (
    <Modal title="签发机器" onClose={onClose} wide>
      {!issued ? (
        <div className="space-y-4">
          <div>
            <label className="label">机器名称</label>
            <input
              className="input"
              type="text"
              placeholder="prod-server-01"
              value={name}
              onChange={(e) => setName(e.target.value)}
              onKeyDown={(e) => e.key === "Enter" && handleIssue()}
              autoFocus
            />
          </div>
          <button
            className="btn-primary w-full"
            onClick={handleIssue}
            disabled={busy || !name.trim()}
          >
            {busy ? <Spinner label="签发中…" /> : "签发身份令牌"}
          </button>
        </div>
      ) : (
        <div className="space-y-4">
          <div className="rounded-lg bg-emerald-900/30 border border-emerald-700/40 px-3 py-2 text-xs text-emerald-300">
            ✓ 身份令牌已签发 — 机器 ID: <span className="font-mono">{issued.machineId}</span>
          </div>

          <div>
            <div className="flex items-center justify-between mb-1">
              <label className="label">身份令牌（Identity Token）</label>
              <button className="btn-ghost text-xs py-0.5 px-2" onClick={copyToken}>
                复制
              </button>
            </div>
            <div className="rounded-lg bg-slate-900 border border-slate-700 p-2.5 font-mono text-xs text-slate-300 break-all select-all">
              {issued.token}
            </div>
          </div>

          <div>
            <div className="flex items-center justify-between mb-1">
              <label className="label">运维注册命令</label>
              <button className="btn-ghost text-xs py-0.5 px-2" onClick={copyCmd}>
                复制
              </button>
            </div>
            <div className="rounded-lg bg-slate-900 border border-slate-700 p-2.5 font-mono text-xs text-slate-400 break-all">
              {enrollCmd}
            </div>
          </div>

          <p className="text-xs text-slate-500">
            将此命令在目标机器上执行。Agent 注册后将通过此令牌向服务端上线，公钥由管理端的主密钥签名。
          </p>

          <button className="btn-ghost w-full" onClick={onClose}>
            关闭
          </button>
        </div>
      )}
    </Modal>
  );
}

// ── MachineDetailModal ────────────────────────────────────────────────────────

type StatusKind = "host" | "cpu" | "mem" | "disk" | "net" | "proc" | "service";

const STATUS_KINDS: { kind: StatusKind; label: string }[] = [
  { kind: "host", label: "主机" },
  { kind: "cpu", label: "CPU" },
  { kind: "mem", label: "内存" },
  { kind: "disk", label: "磁盘" },
  { kind: "net", label: "网络" },
  { kind: "proc", label: "进程" },
  { kind: "service", label: "服务" },
];

function JsonBlock({ data }: { data: unknown }) {
  return (
    <pre className="rounded-lg bg-slate-900 border border-slate-700 p-3 text-xs text-slate-300 overflow-auto max-h-64 whitespace-pre-wrap break-all">
      {JSON.stringify(data, null, 2)}
    </pre>
  );
}

function MachineDetailModal({
  machine,
  onClose,
}: {
  machine: Machine;
  onClose: () => void;
}) {
  const toast = useToast();
  const [activeKind, setActiveKind] = useState<StatusKind | null>(null);
  const [statusData, setStatusData] = useState<Record<string, unknown>>({});
  const [loadingKind, setLoadingKind] = useState<StatusKind | null>(null);
  const [snapshots, setSnapshots] = useState<unknown[] | null>(null);
  const [loadingSnaps, setLoadingSnaps] = useState(false);

  async function fetchStatus(kind: StatusKind) {
    setActiveKind(kind);
    if (statusData[kind]) return; // already fetched
    setLoadingKind(kind);
    try {
      const data = await machineStatus(machine.id, kind);
      setStatusData((prev) => ({ ...prev, [kind]: data }));
    } catch (e) {
      toast("err", errMsg(e));
    } finally {
      setLoadingKind(null);
    }
  }

  async function fetchSnapshots() {
    if (snapshots !== null) return;
    setLoadingSnaps(true);
    try {
      const data = await machineSnapshots(machine.id);
      setSnapshots(data);
    } catch (e) {
      toast("err", errMsg(e));
    } finally {
      setLoadingSnaps(false);
    }
  }

  const lastSeenStr = machine.lastSeen
    ? new Date(machine.lastSeen).toLocaleString("zh-CN")
    : "—";

  return (
    <Modal title={`机器详情 — ${machine.name}`} onClose={onClose} wide>
      <div className="space-y-4">
        {/* Basic info */}
        <div className="grid grid-cols-2 gap-2 text-xs">
          <InfoRow label="机器 ID" value={machine.id} mono />
          <InfoRow label="主机名" value={machine.hostname} />
          <InfoRow label="系统" value={machine.os} />
          <InfoRow label="Agent 版本" value={machine.agentVersion} />
          <InfoRow label="状态" value={machine.online ? "在线" : "离线"} />
          <InfoRow label="最后心跳" value={lastSeenStr} />
        </div>

        {/* Status fetch buttons */}
        <div>
          <p className="label mb-2">拉取状态</p>
          <div className="flex flex-wrap gap-2">
            {STATUS_KINDS.map(({ kind, label }) => (
              <button
                key={kind}
                className={`btn text-xs py-1 px-2.5 ${
                  activeKind === kind ? "btn-primary" : "btn-ghost"
                }`}
                onClick={() => fetchStatus(kind)}
                disabled={loadingKind !== null}
              >
                {loadingKind === kind ? <Spinner /> : label}
              </button>
            ))}
          </div>
        </div>

        {/* Status data display */}
        {activeKind && statusData[activeKind] !== undefined && (
          <div>
            <p className="label mb-1">
              {STATUS_KINDS.find((k) => k.kind === activeKind)?.label} 状态
            </p>
            <JsonBlock data={statusData[activeKind]} />
          </div>
        )}

        {/* Snapshots */}
        <div>
          <div className="flex items-center justify-between mb-2">
            <p className="label">最近快照</p>
            <button
              className="btn-ghost text-xs py-0.5 px-2"
              onClick={fetchSnapshots}
              disabled={loadingSnaps}
            >
              {loadingSnaps ? <Spinner /> : "加载快照"}
            </button>
          </div>
          {snapshots !== null && (
            snapshots.length === 0 ? (
              <p className="text-xs text-slate-500">暂无快照</p>
            ) : (
              <div className="space-y-2 max-h-48 overflow-auto">
                {snapshots.slice(0, 10).map((snap, i) => (
                  <JsonBlock key={i} data={snap} />
                ))}
              </div>
            )
          )}
        </div>
      </div>
    </Modal>
  );
}

function InfoRow({ label, value, mono }: { label: string; value: string; mono?: boolean }) {
  return (
    <div>
      <span className="text-slate-500">{label}: </span>
      <span className={`text-slate-200 ${mono ? "font-mono break-all" : ""}`}>{value}</span>
    </div>
  );
}

// ── MachinesPage ──────────────────────────────────────────────────────────────

export default function MachinesPage() {
  const toast = useToast();
  const [machines, setMachines] = useState<Machine[]>([]);
  const [loading, setLoading] = useState(true);
  const [serverUrl, setServerUrl] = useState("");
  const [showIssue, setShowIssue] = useState(false);
  const [detailMachine, setDetailMachine] = useState<Machine | null>(null);
  const intervalRef = useRef<ReturnType<typeof setInterval> | null>(null);

  const fetchMachines = useCallback(async () => {
    try {
      const data = await listMachines();
      setMachines(Array.isArray(data) ? data : []);
    } catch {
      // silently ignore poll errors; first load shows spinner
    } finally {
      setLoading(false);
    }
  }, []);

  useEffect(() => {
    getServerUrl()
      .then((url) => setServerUrl(url ?? ""))
      .catch(() => {});
  }, []);

  useEffect(() => {
    fetchMachines();
    intervalRef.current = setInterval(fetchMachines, 5000);
    return () => {
      if (intervalRef.current) clearInterval(intervalRef.current);
    };
  }, [fetchMachines]);

  async function handleManualRefresh() {
    setLoading(true);
    try {
      const data = await listMachines();
      setMachines(Array.isArray(data) ? data : []);
    } catch (e) {
      toast("err", errMsg(e));
    } finally {
      setLoading(false);
    }
  }

  return (
    <div className="p-6 space-y-5">
      {/* Header */}
      <div className="flex items-center justify-between">
        <h2 className="text-lg font-bold text-slate-100">机器列表</h2>
        <div className="flex gap-2">
          <button className="btn-ghost text-sm" onClick={handleManualRefresh}>
            刷新
          </button>
          <button className="btn-primary text-sm" onClick={() => setShowIssue(true)}>
            + 签发机器
          </button>
        </div>
      </div>

      {/* Online summary */}
      {machines.length > 0 && (
        <div className="flex gap-4 text-xs text-slate-400">
          <span>
            共 <strong className="text-slate-200">{machines.length}</strong> 台
          </span>
          <span>
            在线{" "}
            <strong className="text-emerald-400">
              {machines.filter((m) => m.online).length}
            </strong>
          </span>
          <span>
            离线{" "}
            <strong className="text-slate-400">
              {machines.filter((m) => !m.online).length}
            </strong>
          </span>
          <span className="text-slate-600">每 5 秒自动刷新</span>
        </div>
      )}

      {/* List */}
      {loading ? (
        <div className="flex items-center justify-center py-16">
          <Spinner label="加载机器列表…" />
        </div>
      ) : machines.length === 0 ? (
        <div className="card p-10 text-center text-slate-500 text-sm">
          <p>暂无机器</p>
          <p className="mt-1 text-xs">使用「签发机器」生成身份令牌，然后在目标机器上运行 Agent</p>
        </div>
      ) : (
        <div className="space-y-2">
          {machines.map((m) => (
            <MachineRow
              key={m.id}
              machine={m}
              onClick={() => setDetailMachine(m)}
            />
          ))}
        </div>
      )}

      {/* Modals */}
      {showIssue && (
        <IssueMachineModal onClose={() => setShowIssue(false)} serverUrl={serverUrl} />
      )}
      {detailMachine && (
        <MachineDetailModal machine={detailMachine} onClose={() => setDetailMachine(null)} />
      )}
    </div>
  );
}

// ── MachineRow ────────────────────────────────────────────────────────────────

function MachineRow({ machine, onClick }: { machine: Machine; onClick: () => void }) {
  const lastSeenStr = machine.lastSeen
    ? new Date(machine.lastSeen).toLocaleString("zh-CN")
    : "—";

  return (
    <button
      className="card w-full flex items-center gap-4 px-4 py-3 text-left hover:border-slate-500 transition"
      onClick={onClick}
    >
      {/* Online badge */}
      <span
        className={`badge ${
          machine.online
            ? "bg-emerald-700/40 text-emerald-300"
            : "bg-slate-700 text-slate-500"
        }`}
      >
        {machine.online ? "在线" : "离线"}
      </span>

      {/* Name + hostname */}
      <div className="flex-1 min-w-0">
        <p className="text-sm font-medium text-slate-100 truncate">{machine.name}</p>
        <p className="text-xs text-slate-500 truncate">{machine.hostname}</p>
      </div>

      {/* OS */}
      <span className="text-xs text-slate-400 hidden sm:block">{machine.os}</span>

      {/* Last seen */}
      <span className="text-xs text-slate-500 whitespace-nowrap">{lastSeenStr}</span>

      {/* Arrow */}
      <span className="text-slate-600 text-xs">›</span>
    </button>
  );
}
