import { useCallback, useEffect, useRef, useState } from "react";
import {
  listMachines,
  issueMachine,
  machineStatus,
  machineSnapshots,
  getActiveServer,
  setShell,
  runShell,
  revokeMachine,
  audit,
  errMsg,
  type Machine,
  type ShellResult,
  type AuditEntry,
} from "../api";
import { Modal, Spinner, useToast } from "../ui";
import { StatusView, OverviewStrip, Pct, fmtUptime, fmtBytes } from "../components/StatusViz";
import TerminalModal from "../components/TerminalModal";

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
];

function MachineDetailModal({
  machine,
  onClose,
  onRefresh,
}: {
  machine: Machine;
  onClose: () => void;
  onRefresh: () => void;
}) {
  const toast = useToast();
  const [activeKind, setActiveKind] = useState<StatusKind | null>(null);
  const [statusData, setStatusData] = useState<Record<string, unknown>>({});
  const [loadingKind, setLoadingKind] = useState<StatusKind | null>(null);
  const [snapshots, setSnapshots] = useState<unknown[] | null>(null);
  const [loadingSnaps, setLoadingSnaps] = useState(false);

  // Shell state
  const [shellEnabled, setShellEnabled] = useState(machine.shellEnabled);
  const [togglingShell, setTogglingShell] = useState(false);
  const [shellCmd, setShellCmd] = useState("");
  const [shellRunning, setShellRunning] = useState(false);
  const [shellResult, setShellResult] = useState<ShellResult | null>(null);

  // Audit state
  const [auditRows, setAuditRows] = useState<AuditEntry[] | null>(null);
  const [loadingAudit, setLoadingAudit] = useState(false);

  // Revoke state
  const [revoking, setRevoking] = useState(false);

  // Auto-load + live-refresh the overview (host/cpu/mem/disk) every 5s.
  useEffect(() => {
    let alive = true;
    const load = async () => {
      const kinds: StatusKind[] = ["host", "cpu", "mem", "disk"];
      const results = await Promise.all(
        kinds.map((k) =>
          machineStatus(machine.id, k)
            .then((d) => [k, d] as const)
            .catch(() => [k, null] as const)
        )
      );
      if (!alive) return;
      const next: Record<string, unknown> = {};
      for (const [k, d] of results) if (d) next[k] = d;
      setStatusData((prev) => ({ ...prev, ...next }));
    };
    load();
    const t = setInterval(load, 5000);
    return () => {
      alive = false;
      clearInterval(t);
    };
  }, [machine.id]);

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

  async function handleToggleShell() {
    const next = !shellEnabled;
    setTogglingShell(true);
    try {
      await setShell(machine.id, next);
      setShellEnabled(next);
      toast("ok", next ? "Shell 权限已开启" : "Shell 权限已关闭");
    } catch (e) {
      toast("err", errMsg(e));
    } finally {
      setTogglingShell(false);
    }
  }

  async function handleRunShell() {
    if (!shellCmd.trim()) return;
    if (!window.confirm(`确认在 ${machine.name} 上执行命令？\n\n> ${shellCmd}`)) return;
    setShellRunning(true);
    setShellResult(null);
    try {
      const result = await runShell(machine.id, shellCmd.trim());
      setShellResult(result);
    } catch (e) {
      toast("err", errMsg(e));
    } finally {
      setShellRunning(false);
    }
  }

  async function handleRevoke() {
    if (!window.confirm(`确认吊销机器 ${machine.name}？\n\n此操作不可逆，Agent 将被立即踢下线。`)) return;
    setRevoking(true);
    try {
      await revokeMachine(machine.id);
      toast("ok", `机器 ${machine.name} 已吊销`);
      onRefresh();
      onClose();
    } catch (e) {
      toast("err", errMsg(e));
    } finally {
      setRevoking(false);
    }
  }

  async function fetchAudit() {
    if (auditRows !== null) return;
    setLoadingAudit(true);
    try {
      const rows = await audit(machine.id);
      setAuditRows(rows);
    } catch (e) {
      toast("err", errMsg(e));
    } finally {
      setLoadingAudit(false);
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
          {(statusData.host as any)?.uptime_secs != null && (
            <InfoRow label="运行时长" value={fmtUptime((statusData.host as any).uptime_secs)} />
          )}
          {machine.summary?.logical_cores ? (
            <InfoRow
              label="配置"
              value={`${machine.summary.logical_cores} 核 · ${fmtBytes(machine.summary.mem_total_bytes)} 内存 · ${fmtBytes(machine.summary.disk_total_bytes)} 硬盘`}
            />
          ) : null}
        </div>

        {/* Live overview strip */}
        <OverviewStrip
          cpu={statusData.cpu as any}
          mem={statusData.mem as any}
          disk={statusData.disk as any}
        />

        {/* Shell 权限 toggle */}
        <div className="rounded-lg border border-slate-700 bg-slate-800/50 px-3 py-2.5">
          <div className="flex items-center justify-between">
            <div>
              <p className="text-xs font-medium text-slate-200">Shell 权限</p>
              <p className="text-xs text-slate-500 mt-0.5">
                {shellEnabled ? "已开启 — 可执行远程命令" : "已关闭 — 执行命令将返回 403"}
              </p>
            </div>
            <button
              className={`relative inline-flex h-5 w-9 items-center rounded-full transition-colors ${
                shellEnabled ? "bg-emerald-600" : "bg-slate-600"
              } ${togglingShell ? "opacity-50 cursor-not-allowed" : "cursor-pointer"}`}
              onClick={handleToggleShell}
              disabled={togglingShell}
              title={shellEnabled ? "关闭 Shell" : "开启 Shell"}
            >
              <span
                className={`inline-block h-3.5 w-3.5 transform rounded-full bg-white shadow transition-transform ${
                  shellEnabled ? "translate-x-4" : "translate-x-0.5"
                }`}
              />
            </button>
          </div>

          {/* Command console — only shown when shell is enabled */}
          {shellEnabled && (
            <div className="mt-3 space-y-2">
              <div className="flex gap-2">
                <input
                  className="input flex-1 font-mono text-xs"
                  type="text"
                  placeholder="输入命令，例如: ls -la /tmp"
                  value={shellCmd}
                  onChange={(e) => setShellCmd(e.target.value)}
                  onKeyDown={(e) => e.key === "Enter" && !shellRunning && handleRunShell()}
                  disabled={shellRunning}
                />
                <button
                  className="btn-primary text-xs px-3 py-1"
                  onClick={handleRunShell}
                  disabled={shellRunning || !shellCmd.trim()}
                >
                  {shellRunning ? <Spinner /> : "执行"}
                </button>
              </div>
              {shellResult !== null && (
                <div className="rounded-lg bg-slate-900 border border-slate-700 p-2.5 font-mono text-xs space-y-1">
                  <div className="flex items-center gap-2">
                    <span className={`badge text-xs ${shellResult.exit === 0 ? "bg-emerald-700/40 text-emerald-300" : "bg-red-700/40 text-red-300"}`}>
                      exit {shellResult.exit}
                    </span>
                  </div>
                  {shellResult.stdout && (
                    <pre className="text-slate-300 whitespace-pre-wrap break-all max-h-40 overflow-auto">
                      {shellResult.stdout}
                    </pre>
                  )}
                  {shellResult.stderr && (
                    <pre className="text-red-400 whitespace-pre-wrap break-all max-h-24 overflow-auto">
                      {shellResult.stderr}
                    </pre>
                  )}
                </div>
              )}
            </div>
          )}
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
            <StatusView kind={activeKind} data={statusData[activeKind]} />
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
              <div className="space-y-1.5 max-h-56 overflow-auto">
                {snapshots.slice(0, 12).map((snap, i) => {
                  const s = snap as any;
                  let parsed: any = {};
                  try {
                    parsed = JSON.parse(s.json);
                  } catch {
                    /* leave empty */
                  }
                  const isSummary = parsed && typeof parsed.cpu_pct === "number";
                  return (
                    <div
                      key={s.id ?? i}
                      className="flex items-baseline gap-3 px-2 py-0.5 text-xs tabular-nums hover:bg-slate-800/40 rounded"
                    >
                      {isSummary ? (
                        <span className="flex items-baseline gap-2.5">
                          <span className="text-slate-500">C<Pct v={parsed.cpu_pct} /></span>
                          <span className="text-slate-500">M<Pct v={parsed.mem_pct} /></span>
                          <span className="text-slate-500">D<Pct v={parsed.disk_pct} /></span>
                        </span>
                      ) : (
                        <span className="text-slate-400">{s.kind ?? "?"}</span>
                      )}
                      <span className="ml-auto text-slate-600 whitespace-nowrap">
                        {s.captured_at ? new Date(s.captured_at).toLocaleTimeString("zh-CN") : ""}
                      </span>
                    </div>
                  );
                })}
              </div>
            )
          )}
        </div>

        {/* Audit log */}
        <div>
          <div className="flex items-center justify-between mb-2">
            <p className="label">审计日志</p>
            <button
              className="btn-ghost text-xs py-0.5 px-2"
              onClick={fetchAudit}
              disabled={loadingAudit}
            >
              {loadingAudit ? <Spinner /> : "加载审计"}
            </button>
          </div>
          {auditRows !== null && (
            auditRows.length === 0 ? (
              <p className="text-xs text-slate-500">暂无审计记录</p>
            ) : (
              <div className="space-y-1.5 max-h-48 overflow-auto">
                {auditRows.map((row) => (
                  <div
                    key={row.id}
                    className="rounded bg-slate-900 border border-slate-700 px-2.5 py-1.5 text-xs"
                  >
                    <div className="flex items-center gap-2 flex-wrap">
                      <span className="badge bg-slate-700 text-slate-300">{row.kind}</span>
                      <span className={`badge text-xs ${row.exit === 0 ? "bg-emerald-700/40 text-emerald-300" : row.exit === null ? "bg-slate-700 text-slate-400" : "bg-red-700/40 text-red-300"}`}>
                        exit {row.exit ?? "—"}
                      </span>
                      <span className="font-mono text-slate-300 truncate max-w-[200px]">
                        {row.request}
                      </span>
                      <span className="text-slate-600 ml-auto whitespace-nowrap">
                        {new Date(row.created_at).toLocaleString("zh-CN")}
                      </span>
                    </div>
                    {row.output && (
                      <pre className="mt-1 text-slate-500 whitespace-pre-wrap break-all max-h-16 overflow-auto">
                        {row.output}
                      </pre>
                    )}
                  </div>
                ))}
              </div>
            )
          )}
        </div>

        {/* Revoke */}
        <div className="pt-2 border-t border-slate-700">
          <button
            className="btn text-xs py-1.5 px-3 bg-red-900/30 border border-red-700/50 text-red-400 hover:bg-red-800/40 hover:text-red-300 transition w-full"
            onClick={handleRevoke}
            disabled={revoking}
          >
            {revoking ? <Spinner label="吊销中…" /> : "吊销此机器"}
          </button>
          <p className="text-xs text-slate-600 mt-1 text-center">
            吊销后 Agent 将被踢下线，操作不可逆
          </p>
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
  const [refreshing, setRefreshing] = useState(false);
  const [serverUrl, setServerUrl] = useState("");
  const [showIssue, setShowIssue] = useState(false);
  const [detailMachine, setDetailMachine] = useState<Machine | null>(null);
  const [terminalMachine, setTerminalMachine] = useState<Machine | null>(null);
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
    getActiveServer()
      .then((profile) => setServerUrl(profile?.url ?? ""))
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
    // Silent refresh — update values in place, never tear down the list.
    setRefreshing(true);
    try {
      const data = await listMachines();
      setMachines(Array.isArray(data) ? data : []);
    } catch (e) {
      toast("err", errMsg(e));
    } finally {
      setRefreshing(false);
    }
  }

  return (
    <div className="p-5 space-y-3">
      {/* Header */}
      <div className="flex items-center justify-between">
        <h2 className="text-lg font-bold text-slate-100">机器列表</h2>
        <div className="flex gap-2">
          <button className="btn-ghost text-sm" onClick={handleManualRefresh} disabled={refreshing}>
            {refreshing ? "刷新中…" : "刷新"}
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

      {/* List — spinner only on the very first load; later refreshes update in place */}
      {loading && machines.length === 0 ? (
        <div className="flex items-center justify-center py-16">
          <Spinner label="加载机器列表…" />
        </div>
      ) : machines.length === 0 ? (
        <div className="card p-10 text-center text-slate-500 text-sm">
          <p>暂无机器</p>
          <p className="mt-1 text-xs">使用「签发机器」生成身份令牌，然后在目标机器上运行 Agent</p>
        </div>
      ) : (
        <div className="space-y-1.5">
          {machines.map((m) => (
            <MachineRow
              key={m.id}
              machine={m}
              onClick={() => setDetailMachine(m)}
              onOpenTerminal={() => setTerminalMachine(m)}
            />
          ))}
        </div>
      )}

      {/* Modals */}
      {showIssue && (
        <IssueMachineModal onClose={() => setShowIssue(false)} serverUrl={serverUrl} />
      )}
      {detailMachine && (
        <MachineDetailModal
          machine={detailMachine}
          onClose={() => setDetailMachine(null)}
          onRefresh={fetchMachines}
        />
      )}
      {terminalMachine && (
        <TerminalModal
          machine={terminalMachine}
          onClose={() => setTerminalMachine(null)}
        />
      )}
    </div>
  );
}

// ── MachineRow ────────────────────────────────────────────────────────────────

function MachineRow({
  machine,
  onClick,
  onOpenTerminal,
}: {
  machine: Machine;
  onClick: () => void;
  onOpenTerminal: () => void;
}) {
  const lastSeenStr = machine.lastSeen
    ? new Date(machine.lastSeen).toLocaleString("zh-CN")
    : "—";

  // Show live metrics for online machines; offline machines reserve the columns
  // with "—" so every row's CPU/内存/磁盘 values line up vertically.
  const s = machine.online ? machine.summary : null;

  return (
    <div
      role="button"
      tabIndex={0}
      className="card w-full grid grid-cols-[3rem_minmax(0,1fr)_15rem_4rem_auto] items-center gap-3 px-3.5 py-2 text-left cursor-pointer hover:border-slate-500 transition"
      onClick={onClick}
      onKeyDown={(e) => {
        if (e.key === "Enter" || e.key === " ") {
          e.preventDefault();
          onClick();
        }
      }}
    >
      {/* Online badge */}
      <span
        className={`badge justify-self-start ${
          machine.online
            ? "bg-emerald-700/40 text-emerald-300"
            : "bg-slate-700 text-slate-500"
        }`}
      >
        {machine.online ? "在线" : "离线"}
      </span>

      {/* Name + hardware spec (always visible; hostname/IP omitted) */}
      <div className="min-w-0">
        <p className="text-sm font-medium text-slate-100 truncate">{machine.name}</p>
        {machine.summary?.logical_cores ? (
          <p className="text-xs text-slate-500 truncate tabular-nums">
            {machine.summary.logical_cores} 核 · {fmtBytes(machine.summary.mem_total_bytes)} · {fmtBytes(machine.summary.disk_total_bytes)}
          </p>
        ) : null}
      </div>

      {/* Live usage — fixed 3-column grid so values align across rows */}
      <div className="grid grid-cols-3 gap-x-3 text-xs tabular-nums">
        <Metric label="CPU" v={s?.cpu_pct} />
        <Metric label="内存" v={s?.mem_pct} />
        <Metric label="磁盘" v={s?.disk_pct} />
      </div>

      {/* OS */}
      <span className="text-xs text-slate-400 truncate">{machine.os}</span>

      {/* Last seen + shell + arrow */}
      <span className="flex items-center gap-2 justify-end whitespace-nowrap">
        <span className="text-xs text-slate-500">{lastSeenStr}</span>
        <button
          className="shrink-0 rounded p-1 text-slate-500 hover:text-emerald-300 hover:bg-slate-700/60 transition"
          title="打开终端"
          aria-label="打开终端"
          onClick={(e) => {
            e.stopPropagation();
            onOpenTerminal();
          }}
        >
          <svg
            viewBox="0 0 24 24"
            className="w-4 h-4"
            fill="none"
            stroke="currentColor"
            strokeWidth={2}
            strokeLinecap="round"
            strokeLinejoin="round"
          >
            <path d="M6.75 7.5l3 2.25-3 2.25M11.25 12h3M5.25 3.75h13.5A2.25 2.25 0 0 1 21 6v12a2.25 2.25 0 0 1-2.25 2.25H5.25A2.25 2.25 0 0 1 3 18V6a2.25 2.25 0 0 1 2.25-2.25Z" />
          </svg>
        </button>
        <span className="text-slate-600 text-xs">›</span>
      </span>
    </div>
  );
}

// A single aligned metric cell: label on the left, percentage right-aligned in a
// fixed-width box so the numbers line up column-to-column down the list.
function Metric({ label, v }: { label: string; v?: number | null }) {
  return (
    <span className="flex items-baseline justify-end gap-1.5">
      <span className="text-slate-500">{label}</span>
      <span className="w-9 text-right">
        <Pct v={v} />
      </span>
    </span>
  );
}
