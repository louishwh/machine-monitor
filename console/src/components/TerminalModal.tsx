import { useEffect, useRef, useState, type KeyboardEvent } from "react";
import {
  runShell,
  setShell,
  errMsg,
  type Machine,
  type ShellResult,
} from "../api";
import { Modal, Spinner, useToast } from "../ui";

interface Entry {
  cmd: string;
  result?: ShellResult;
  error?: string;
  running?: boolean;
}

/// A lightweight REPL-style terminal for one machine. Each command is a one-shot
/// run-shell request (no persistent PTY) — `cd` and shell state do NOT carry over
/// between commands. Every command is recorded in the server audit log.
export default function TerminalModal({
  machine,
  onClose,
}: {
  machine: Machine;
  onClose: () => void;
}) {
  const toast = useToast();
  const [enabled, setEnabled] = useState(machine.shellEnabled);
  const [enabling, setEnabling] = useState(false);
  const [history, setHistory] = useState<Entry[]>([]);
  const [cmd, setCmd] = useState("");
  const [running, setRunning] = useState(false);
  const [cmdHistory, setCmdHistory] = useState<string[]>([]);
  const [histIdx, setHistIdx] = useState<number | null>(null);
  const scrollRef = useRef<HTMLDivElement>(null);
  const inputRef = useRef<HTMLInputElement>(null);

  // Auto-scroll to the bottom whenever output grows.
  useEffect(() => {
    const el = scrollRef.current;
    if (el) el.scrollTop = el.scrollHeight;
  }, [history]);

  // Focus the input once shell is usable.
  useEffect(() => {
    if (enabled) inputRef.current?.focus();
  }, [enabled]);

  async function handleEnable() {
    setEnabling(true);
    try {
      await setShell(machine.id, true);
      setEnabled(true);
      toast("ok", "Shell 已开启");
    } catch (e) {
      toast("err", errMsg(e));
    } finally {
      setEnabling(false);
    }
  }

  async function run() {
    const c = cmd.trim();
    if (!c || running) return;
    setCmd("");
    setCmdHistory((h) => [...h, c]);
    setHistIdx(null);
    const idx = history.length;
    setHistory((h) => [...h, { cmd: c, running: true }]);
    setRunning(true);
    try {
      const result = await runShell(machine.id, c);
      setHistory((h) => h.map((e, i) => (i === idx ? { cmd: c, result } : e)));
    } catch (e) {
      setHistory((h) => h.map((e, i) => (i === idx ? { cmd: c, error: errMsg(e) } : e)));
    } finally {
      setRunning(false);
      inputRef.current?.focus();
    }
  }

  function onKeyDown(e: KeyboardEvent<HTMLInputElement>) {
    if (e.key === "Enter") {
      run();
      return;
    }
    if (e.key === "ArrowUp") {
      e.preventDefault();
      if (cmdHistory.length === 0) return;
      const ni = histIdx === null ? cmdHistory.length - 1 : Math.max(0, histIdx - 1);
      setHistIdx(ni);
      setCmd(cmdHistory[ni]);
    } else if (e.key === "ArrowDown") {
      e.preventDefault();
      if (histIdx === null) return;
      const ni = histIdx + 1;
      if (ni >= cmdHistory.length) {
        setHistIdx(null);
        setCmd("");
      } else {
        setHistIdx(ni);
        setCmd(cmdHistory[ni]);
      }
    }
  }

  return (
    <Modal title={`终端 — ${machine.name}`} onClose={onClose} wide>
      {!enabled ? (
        <div className="space-y-3">
          <p className="text-xs text-amber-400">
            该机器的 Shell 未开启。开启后可在此执行远程命令（以 agent 身份运行，通常为 root），所有命令都会记入审计。
          </p>
          <button className="btn-primary" onClick={handleEnable} disabled={enabling}>
            {enabling ? <Spinner label="开启中…" /> : "开启 Shell"}
          </button>
        </div>
      ) : (
        <div className="space-y-2">
          {!machine.online && (
            <p className="text-xs text-amber-400">机器当前离线，命令会超时。</p>
          )}

          {/* Scrollback */}
          <div
            ref={scrollRef}
            className="rounded-lg bg-black/70 border border-slate-700 p-3 font-mono text-xs h-80 overflow-auto space-y-2"
          >
            {history.length === 0 ? (
              <p className="text-slate-600">输入命令并回车执行。↑/↓ 浏览命令历史。</p>
            ) : (
              history.map((e, i) => (
                <div key={i}>
                  <div className="text-emerald-400 break-all">
                    <span className="text-slate-500">$</span> {e.cmd}
                  </div>
                  {e.running && <div className="text-slate-500">运行中…</div>}
                  {e.error && (
                    <div className="text-rose-400 whitespace-pre-wrap break-all">{e.error}</div>
                  )}
                  {e.result && (
                    <>
                      {e.result.stdout && (
                        <pre className="text-slate-300 whitespace-pre-wrap break-all">
                          {e.result.stdout}
                        </pre>
                      )}
                      {e.result.stderr && (
                        <pre className="text-rose-400 whitespace-pre-wrap break-all">
                          {e.result.stderr}
                        </pre>
                      )}
                      {e.result.exit !== 0 && (
                        <div className="text-slate-600">[exit {e.result.exit}]</div>
                      )}
                    </>
                  )}
                </div>
              ))
            )}
          </div>

          {/* Prompt */}
          <div className="flex items-center gap-2">
            <span className="font-mono text-emerald-400 text-xs">$</span>
            <input
              ref={inputRef}
              className="input flex-1 font-mono text-xs"
              value={cmd}
              onChange={(e) => setCmd(e.target.value)}
              onKeyDown={onKeyDown}
              placeholder={running ? "运行中…" : "输入命令…"}
              disabled={running}
              autoFocus
            />
            <button
              className="btn-primary text-xs px-3 py-1"
              onClick={run}
              disabled={running || !cmd.trim()}
            >
              {running ? <Spinner /> : "执行"}
            </button>
          </div>
          <p className="text-xs text-slate-600">
            每条命令独立执行（非持久会话，cd 等状态不跨命令保留）。所有命令均记入审计日志。
          </p>
        </div>
      )}
    </Modal>
  );
}
