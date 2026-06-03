// Visual renderers for agent status payloads. Dark "control-room" aesthetic:
// threshold-colored ring gauges + usage bars, tabular-num readouts, compact tables.
import type { ReactNode } from "react";

export function fmtBytes(n: number | null | undefined): string {
  if (n == null || isNaN(n as number)) return "—";
  const u = ["B", "KB", "MB", "GB", "TB", "PB"];
  let v = Number(n);
  let i = 0;
  while (v >= 1024 && i < u.length - 1) {
    v /= 1024;
    i++;
  }
  return `${v.toFixed(v < 10 && i > 0 ? 1 : 0)} ${u[i]}`;
}

export function fmtUptime(s: number | null | undefined): string {
  if (s == null) return "—";
  const d = Math.floor(s / 86400);
  const h = Math.floor((s % 86400) / 3600);
  const m = Math.floor((s % 3600) / 60);
  const parts: string[] = [];
  if (d) parts.push(`${d}天`);
  if (h) parts.push(`${h}时`);
  if (!d) parts.push(`${m}分`);
  return parts.join(" ") || "<1分";
}

function tone(pct: number): string {
  if (pct >= 90) return "#fb7185"; // rose
  if (pct >= 70) return "#fbbf24"; // amber
  return "#34d399"; // emerald
}

export function Ring({ pct, label, sub }: { pct: number; label: string; sub?: string }) {
  const p = Math.max(0, Math.min(pct, 100));
  const r = 34;
  const c = 2 * Math.PI * r;
  const off = c * (1 - p / 100);
  const col = tone(p);
  return (
    <div className="flex flex-col items-center">
      <svg width="96" height="96" viewBox="0 0 92 92">
        <circle cx="46" cy="46" r={r} fill="none" stroke="#1e293b" strokeWidth="8" />
        <circle
          cx="46"
          cy="46"
          r={r}
          fill="none"
          stroke={col}
          strokeWidth="8"
          strokeLinecap="round"
          strokeDasharray={c}
          strokeDashoffset={off}
          transform="rotate(-90 46 46)"
          style={{ transition: "stroke-dashoffset .6s cubic-bezier(.4,0,.2,1)" }}
        />
        <text
          x="46"
          y="50"
          textAnchor="middle"
          fill="#e2e8f0"
          fontSize="20"
          fontWeight="700"
          style={{ fontVariantNumeric: "tabular-nums" }}
        >
          {Math.round(p)}
          <tspan fontSize="11" fill="#94a3b8">
            %
          </tspan>
        </text>
      </svg>
      <span className="mt-0.5 text-xs font-medium text-slate-300">{label}</span>
      {sub && <span className="text-[11px] text-slate-500 tabular-nums">{sub}</span>}
    </div>
  );
}

function Bar({ pct }: { pct: number }) {
  const p = Math.max(0, Math.min(pct, 100));
  return (
    <div className="h-2 w-full overflow-hidden rounded-full bg-slate-800">
      <div
        className="h-full rounded-full"
        style={{ width: `${p}%`, background: tone(p), transition: "width .5s ease" }}
      />
    </div>
  );
}

function Stat({ label, value }: { label: string; value: ReactNode }) {
  return (
    <div className="flex items-baseline justify-between gap-3 py-1">
      <span className="text-xs text-slate-500">{label}</span>
      <span className="text-xs font-medium text-slate-200 tabular-nums">{value}</span>
    </div>
  );
}

function Panel({ children }: { children: ReactNode }) {
  return <div className="rounded-lg border border-slate-700/70 bg-slate-800/40 p-3">{children}</div>;
}

// ── Overview rings (cpu / mem / disk) ──────────────────────────────────────────

export function OverviewRings({
  cpu,
  mem,
  disk,
}: {
  cpu?: any;
  mem?: any;
  disk?: any;
}) {
  const cpuPct = cpu?.global_usage_pct ?? null;
  const memPct =
    mem && mem.total_bytes ? (mem.used_bytes / mem.total_bytes) * 100 : null;
  const root =
    disk?.disks?.find((d: any) => d.mount === "/") ?? disk?.disks?.[0] ?? null;
  const diskPct = root && root.total_bytes ? (root.used_bytes / root.total_bytes) * 100 : null;

  return (
    <div className="grid grid-cols-3 gap-2 rounded-xl border border-slate-700/70 bg-gradient-to-b from-slate-800/60 to-slate-900/40 p-3">
      {cpuPct == null ? (
        <RingSkeleton label="CPU" />
      ) : (
        <Ring pct={cpuPct} label="CPU" sub={cpu.logical_cores ? `${cpu.logical_cores} 核` : undefined} />
      )}
      {memPct == null ? (
        <RingSkeleton label="内存" />
      ) : (
        <Ring pct={memPct} label="内存" sub={`${fmtBytes(mem.used_bytes)} / ${fmtBytes(mem.total_bytes)}`} />
      )}
      {diskPct == null ? (
        <RingSkeleton label="磁盘" />
      ) : (
        <Ring pct={diskPct} label={`磁盘 ${root.mount}`} sub={`${fmtBytes(root.used_bytes)} / ${fmtBytes(root.total_bytes)}`} />
      )}
    </div>
  );
}

function RingSkeleton({ label }: { label: string }) {
  return (
    <div className="flex flex-col items-center">
      <div className="h-24 w-24 animate-pulse rounded-full border-8 border-slate-800" />
      <span className="mt-0.5 text-xs text-slate-500">{label}</span>
      <span className="text-[11px] text-slate-600">读取中…</span>
    </div>
  );
}

// ── Per-kind detail views ───────────────────────────────────────────────────

function HostView({ d }: { d: any }) {
  return (
    <Panel>
      <Stat label="主机名" value={<span className="font-mono">{d.hostname}</span>} />
      <Stat label="系统" value={d.os} />
      <Stat label="内核" value={<span className="font-mono">{d.kernel}</span>} />
      <Stat label="架构" value={d.cpu_arch} />
      <Stat label="运行时长" value={fmtUptime(d.uptime_secs)} />
    </Panel>
  );
}

function CpuView({ d }: { d: any }) {
  return (
    <div className="space-y-2">
      <Panel>
        <div className="mb-1.5 flex items-center justify-between">
          <span className="text-xs text-slate-400">总体使用率</span>
          <span className="text-xs font-semibold text-slate-200 tabular-nums">
            {d.global_usage_pct?.toFixed(1)}%
          </span>
        </div>
        <Bar pct={d.global_usage_pct ?? 0} />
        <div className="mt-2 grid grid-cols-3 gap-2 text-center">
          <LoadCell label="1m" v={d.load_avg?.one} />
          <LoadCell label="5m" v={d.load_avg?.five} />
          <LoadCell label="15m" v={d.load_avg?.fifteen} />
        </div>
        <p className="mt-1.5 text-[11px] text-slate-500">
          物理核 {d.physical_cores} · 逻辑核 {d.logical_cores}
        </p>
      </Panel>
      <Panel>
        <div className="space-y-1.5">
          {(d.cores ?? []).map((c: any) => (
            <div key={c.core} className="flex items-center gap-2">
              <span className="w-12 shrink-0 font-mono text-[11px] text-slate-500">{c.name}</span>
              <div className="flex-1">
                <Bar pct={c.usage_pct ?? 0} />
              </div>
              <span className="w-12 shrink-0 text-right text-[11px] text-slate-300 tabular-nums">
                {(c.usage_pct ?? 0).toFixed(0)}%
              </span>
            </div>
          ))}
        </div>
      </Panel>
    </div>
  );
}

function LoadCell({ label, v }: { label: string; v: number | undefined }) {
  return (
    <div className="rounded bg-slate-900/60 py-1">
      <div className="text-sm font-semibold text-slate-200 tabular-nums">{v?.toFixed(2) ?? "—"}</div>
      <div className="text-[10px] text-slate-500">{label}</div>
    </div>
  );
}

function MemView({ d }: { d: any }) {
  const memPct = d.total_bytes ? (d.used_bytes / d.total_bytes) * 100 : 0;
  const swapPct = d.total_swap_bytes ? (d.used_swap_bytes / d.total_swap_bytes) * 100 : 0;
  return (
    <Panel>
      <div className="mb-1.5 flex items-center justify-between">
        <span className="text-xs text-slate-400">内存</span>
        <span className="text-xs font-semibold text-slate-200 tabular-nums">
          {fmtBytes(d.used_bytes)} / {fmtBytes(d.total_bytes)}
        </span>
      </div>
      <Bar pct={memPct} />
      <div className="mt-2 grid grid-cols-3 gap-2">
        <Stat label="可用" value={fmtBytes(d.available_bytes)} />
        <Stat label="空闲" value={fmtBytes(d.free_bytes)} />
        <Stat label="已用" value={`${memPct.toFixed(0)}%`} />
      </div>
      {d.total_swap_bytes > 0 && (
        <div className="mt-3 border-t border-slate-700/50 pt-2">
          <div className="mb-1.5 flex items-center justify-between">
            <span className="text-xs text-slate-400">Swap</span>
            <span className="text-xs font-semibold text-slate-200 tabular-nums">
              {fmtBytes(d.used_swap_bytes)} / {fmtBytes(d.total_swap_bytes)}
            </span>
          </div>
          <Bar pct={swapPct} />
        </div>
      )}
    </Panel>
  );
}

function DiskView({ d }: { d: any }) {
  return (
    <div className="space-y-2">
      {(d.disks ?? []).map((disk: any, i: number) => {
        const pct = disk.total_bytes ? (disk.used_bytes / disk.total_bytes) * 100 : 0;
        return (
          <Panel key={i}>
            <div className="mb-1.5 flex items-baseline justify-between gap-2">
              <span className="font-mono text-xs text-slate-200">{disk.mount}</span>
              <span className="text-[11px] text-slate-500">
                {disk.name} · {disk.fs}
              </span>
            </div>
            <Bar pct={pct} />
            <div className="mt-1.5 flex justify-between text-[11px] text-slate-400 tabular-nums">
              <span>已用 {fmtBytes(disk.used_bytes)}</span>
              <span>可用 {fmtBytes(disk.available_bytes)}</span>
              <span className="text-slate-300">{pct.toFixed(0)}%</span>
            </div>
          </Panel>
        );
      })}
    </div>
  );
}

function NetView({ d }: { d: any }) {
  return (
    <Panel>
      <table className="w-full text-xs">
        <thead>
          <tr className="text-slate-500">
            <th className="pb-1 text-left font-medium">接口</th>
            <th className="pb-1 text-right font-medium">↓ 接收</th>
            <th className="pb-1 text-right font-medium">↑ 发送</th>
          </tr>
        </thead>
        <tbody className="font-mono">
          {(d.interfaces ?? []).map((n: any) => (
            <tr key={n.interface} className="border-t border-slate-700/40">
              <td className="py-1 text-slate-300">{n.interface}</td>
              <td className="py-1 text-right text-emerald-300 tabular-nums">{fmtBytes(n.rx_bytes)}</td>
              <td className="py-1 text-right text-sky-300 tabular-nums">{fmtBytes(n.tx_bytes)}</td>
            </tr>
          ))}
        </tbody>
      </table>
    </Panel>
  );
}

function ProcView({ d }: { d: any }) {
  return (
    <Panel>
      <div className="mb-1.5 flex items-center justify-between">
        <span className="text-xs text-slate-400">Top 进程（按 CPU）</span>
        <span className="text-[11px] text-slate-500">共 {d.total_processes} 个进程</span>
      </div>
      <table className="w-full text-xs">
        <thead>
          <tr className="text-slate-500">
            <th className="pb-1 text-left font-medium">PID</th>
            <th className="pb-1 text-left font-medium">名称</th>
            <th className="pb-1 text-right font-medium">CPU</th>
            <th className="pb-1 text-right font-medium">内存</th>
          </tr>
        </thead>
        <tbody className="font-mono">
          {(d.top_by_cpu ?? []).map((p: any) => (
            <tr key={p.pid} className="border-t border-slate-700/40">
              <td className="py-1 text-slate-500 tabular-nums">{p.pid}</td>
              <td className="py-1 text-slate-300">{p.name}</td>
              <td className="py-1 text-right tabular-nums" style={{ color: tone(p.cpu_pct ?? 0) }}>
                {(p.cpu_pct ?? 0).toFixed(1)}%
              </td>
              <td className="py-1 text-right text-slate-400 tabular-nums">{fmtBytes(p.mem_bytes)}</td>
            </tr>
          ))}
        </tbody>
      </table>
    </Panel>
  );
}

export function MiniUsage({ label, pct }: { label: string; pct: number }) {
  return (
    <div>
      <div className="flex justify-between text-[11px] text-slate-500">
        <span>{label}</span>
        <span className="text-slate-300 tabular-nums">{Math.round(pct)}%</span>
      </div>
      <Bar pct={pct} />
    </div>
  );
}

export function StatusView({ kind, data }: { kind: string; data: any }) {
  if (data && typeof data === "object" && "error" in data) {
    return (
      <div className="rounded-lg border border-amber-700/40 bg-amber-900/20 px-3 py-2 text-xs text-amber-300">
        {String((data as any).error)}
      </div>
    );
  }
  switch (kind) {
    case "host":
      return <HostView d={data} />;
    case "cpu":
      return <CpuView d={data} />;
    case "mem":
      return <MemView d={data} />;
    case "disk":
      return <DiskView d={data} />;
    case "net":
      return <NetView d={data} />;
    case "proc":
      return <ProcView d={data} />;
    default:
      return (
        <pre className="overflow-auto rounded-lg border border-slate-700 bg-slate-900 p-3 text-xs text-slate-300">
          {JSON.stringify(data, null, 2)}
        </pre>
      );
  }
}
