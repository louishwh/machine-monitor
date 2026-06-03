// Compact, text-forward renderers for agent status. No progress bars —
// utilization is conveyed by threshold-colored numbers. High information density.
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
  return `${v.toFixed(v < 10 && i > 0 ? 1 : 0)}${u[i]}`;
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
  if (pct >= 75) return "#fbbf24"; // amber
  if (pct >= 50) return "#a3e635"; // lime
  return "#34d399"; // emerald
}

/** Threshold-colored percentage text. */
export function Pct({ v, big }: { v: number | null | undefined; big?: boolean }) {
  if (v == null || isNaN(v as number))
    return <span className="text-slate-600 tabular-nums">—</span>;
  return (
    <span
      className={`tabular-nums font-semibold ${big ? "text-base" : ""}`}
      style={{ color: tone(v) }}
    >
      {Math.round(v)}%
    </span>
  );
}

function KV({ k, v }: { k: string; v: ReactNode }) {
  return (
    <div className="flex items-baseline justify-between gap-2">
      <span className="text-slate-500">{k}</span>
      <span className="text-slate-200 tabular-nums">{v}</span>
    </div>
  );
}

// ── Compact overview strip (cpu / mem / disk) ─────────────────────────────────

export function OverviewStrip({ cpu, mem, disk }: { cpu?: any; mem?: any; disk?: any }) {
  const cpuPct = cpu?.global_usage_pct ?? null;
  const memPct = mem && mem.total_bytes ? (mem.used_bytes / mem.total_bytes) * 100 : null;
  const root = disk?.disks?.find((d: any) => d.mount === "/") ?? disk?.disks?.[0] ?? null;
  const diskPct = root && root.total_bytes ? (root.used_bytes / root.total_bytes) * 100 : null;

  const Cell = ({ label, pct, sub }: { label: string; pct: number | null; sub?: string }) => (
    <div className="px-2.5 py-1.5">
      <div className="text-[10px] uppercase tracking-wider text-slate-500">{label}</div>
      <div className="leading-none mt-0.5">
        <Pct v={pct} big />
      </div>
      {sub && <div className="mt-0.5 text-[10px] text-slate-500 tabular-nums">{sub}</div>}
    </div>
  );

  return (
    <div className="grid grid-cols-3 divide-x divide-slate-700/60 rounded-lg border border-slate-700/70 bg-slate-800/40">
      <Cell label="CPU" pct={cpuPct} sub={cpu?.logical_cores ? `${cpu.logical_cores} 核` : undefined} />
      <Cell
        label="内存"
        pct={memPct}
        sub={mem ? `${fmtBytes(mem.used_bytes)} / ${fmtBytes(mem.total_bytes)}` : undefined}
      />
      <Cell
        label={root ? `磁盘 ${root.mount}` : "磁盘"}
        pct={diskPct}
        sub={root ? `${fmtBytes(root.used_bytes)} / ${fmtBytes(root.total_bytes)}` : undefined}
      />
    </div>
  );
}

// ── Per-kind detail views (dense, text-color) ─────────────────────────────────

function HostView({ d }: { d: any }) {
  return (
    <div className="grid grid-cols-2 gap-x-4 gap-y-0.5 text-xs">
      <KV k="主机名" v={<span className="font-mono">{d.hostname}</span>} />
      <KV k="系统" v={d.os} />
      <KV k="内核" v={<span className="font-mono">{d.kernel}</span>} />
      <KV k="架构" v={d.cpu_arch} />
      <KV k="运行时长" v={fmtUptime(d.uptime_secs)} />
    </div>
  );
}

function CpuView({ d }: { d: any }) {
  return (
    <div className="space-y-1.5 text-xs">
      <div className="flex flex-wrap items-center gap-x-4 gap-y-0.5">
        <span className="text-slate-500">
          总体 <Pct v={d.global_usage_pct} />
        </span>
        <span className="text-slate-500">
          负载{" "}
          <span className="text-slate-300 tabular-nums font-mono">
            {d.load_avg?.one?.toFixed(2)} {d.load_avg?.five?.toFixed(2)} {d.load_avg?.fifteen?.toFixed(2)}
          </span>
        </span>
        <span className="text-slate-500">
          {d.physical_cores} 物理 / {d.logical_cores} 逻辑核
        </span>
      </div>
      <div className="grid grid-cols-4 gap-x-4 gap-y-0.5 font-mono">
        {(d.cores ?? []).map((c: any) => (
          <div key={c.core} className="flex items-baseline justify-between">
            <span className="text-slate-500">{c.name}</span>
            <Pct v={c.usage_pct} />
          </div>
        ))}
      </div>
    </div>
  );
}

function MemView({ d }: { d: any }) {
  const memPct = d.total_bytes ? (d.used_bytes / d.total_bytes) * 100 : 0;
  const swapPct = d.total_swap_bytes ? (d.used_swap_bytes / d.total_swap_bytes) * 100 : 0;
  return (
    <div className="grid grid-cols-2 gap-x-4 gap-y-0.5 text-xs">
      <KV k="内存" v={<Pct v={memPct} />} />
      <KV k="已用/总" v={`${fmtBytes(d.used_bytes)} / ${fmtBytes(d.total_bytes)}`} />
      <KV k="可用" v={fmtBytes(d.available_bytes)} />
      <KV k="空闲" v={fmtBytes(d.free_bytes)} />
      {d.total_swap_bytes > 0 && (
        <>
          <KV k="Swap" v={<Pct v={swapPct} />} />
          <KV k="Swap 用/总" v={`${fmtBytes(d.used_swap_bytes)} / ${fmtBytes(d.total_swap_bytes)}`} />
        </>
      )}
    </div>
  );
}

function DiskView({ d }: { d: any }) {
  return (
    <div className="divide-y divide-slate-800 text-xs font-mono">
      {(d.disks ?? []).map((disk: any, i: number) => {
        const pct = disk.total_bytes ? (disk.used_bytes / disk.total_bytes) * 100 : 0;
        return (
          <div key={i} className="flex items-baseline justify-between gap-2 py-0.5">
            <span className="text-slate-300">{disk.mount}</span>
            <span className="text-slate-500 tabular-nums">
              {fmtBytes(disk.used_bytes)} / {fmtBytes(disk.total_bytes)} <Pct v={pct} />
            </span>
          </div>
        );
      })}
    </div>
  );
}

function NetView({ d }: { d: any }) {
  return (
    <table className="w-full text-xs font-mono">
      <thead>
        <tr className="text-slate-500">
          <th className="pb-0.5 text-left font-medium">接口</th>
          <th className="pb-0.5 text-right font-medium">↓ 接收</th>
          <th className="pb-0.5 text-right font-medium">↑ 发送</th>
        </tr>
      </thead>
      <tbody>
        {(d.interfaces ?? []).map((n: any) => (
          <tr key={n.interface}>
            <td className="py-px text-slate-300">{n.interface}</td>
            <td className="py-px text-right text-emerald-300 tabular-nums">{fmtBytes(n.rx_bytes)}</td>
            <td className="py-px text-right text-sky-300 tabular-nums">{fmtBytes(n.tx_bytes)}</td>
          </tr>
        ))}
      </tbody>
    </table>
  );
}

function ProcView({ d }: { d: any }) {
  return (
    <div className="text-xs">
      <div className="mb-0.5 text-[11px] text-slate-500">共 {d.total_processes} 进程 · Top CPU</div>
      <table className="w-full font-mono">
        <tbody>
          {(d.top_by_cpu ?? []).map((p: any) => (
            <tr key={p.pid}>
              <td className="py-px pr-2 text-slate-600 tabular-nums">{p.pid}</td>
              <td className="py-px text-slate-300">{p.name}</td>
              <td className="py-px pr-3 text-right tabular-nums">
                <Pct v={p.cpu_pct} />
              </td>
              <td className="py-px text-right text-slate-500 tabular-nums">{fmtBytes(p.mem_bytes)}</td>
            </tr>
          ))}
        </tbody>
      </table>
    </div>
  );
}

export function StatusView({ kind, data }: { kind: string; data: any }) {
  if (data && typeof data === "object" && "error" in data) {
    return (
      <div className="rounded border border-amber-700/40 bg-amber-900/20 px-2.5 py-1.5 text-xs text-amber-300">
        {String((data as any).error)}
      </div>
    );
  }
  const inner = (() => {
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
          <pre className="overflow-auto text-xs text-slate-300">{JSON.stringify(data, null, 2)}</pre>
        );
    }
  })();
  return <div className="rounded-lg border border-slate-700/70 bg-slate-800/40 px-3 py-2">{inner}</div>;
}
