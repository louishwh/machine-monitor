import { useEffect, useState } from "react";
import {
  masterStatus,
  generateMasterKey,
  getActiveServer,
  pairServer,
  errMsg,
  type MasterStatus,
  type ServerProfile,
} from "../api";
import { Spinner, useToast } from "../ui";

interface Props {
  activeServerId: string | null;
}

export default function ConnectPage({ activeServerId }: Props) {
  const toast = useToast();

  // ── Master key state ───────────────────────────────────────────────────────
  const [masterSt, setMasterSt] = useState<MasterStatus | null>(null);
  const [loadingMaster, setLoadingMaster] = useState(true);
  const [generatingKey, setGeneratingKey] = useState(false);

  // ── Active server (read-only here; manage via the ⚙ button in the sidebar) ──
  const [activeProfile, setActiveProfile] = useState<ServerProfile | null>(null);
  const [loadingServer, setLoadingServer] = useState(true);

  // ── Pair state ─────────────────────────────────────────────────────────────
  const [pairingToken, setPairingToken] = useState("");
  const [pairing, setPairing] = useState(false);

  // ── Boot: master key ─────────────────────────────────────────────────────────
  useEffect(() => {
    masterStatus()
      .then((ms) => setMasterSt(ms))
      .catch(() => {})
      .finally(() => setLoadingMaster(false));
  }, []);

  // Reload the active server profile whenever it changes (sidebar switch / ⚙ edit).
  useEffect(() => {
    setLoadingServer(true);
    getActiveServer()
      .then((p) => setActiveProfile(p))
      .catch(() => {})
      .finally(() => setLoadingServer(false));
  }, [activeServerId]);

  // ── Handlers ───────────────────────────────────────────────────────────────
  async function handleGenerateKey() {
    setGeneratingKey(true);
    try {
      const result = await generateMasterKey();
      setMasterSt({ hasKey: true, publicKeyB64: result.publicKeyB64 });
      toast("ok", "主密钥生成成功");
    } catch (e) {
      toast("err", errMsg(e));
    } finally {
      setGeneratingKey(false);
    }
  }

  async function handlePair() {
    const token = pairingToken.trim();
    if (!token) {
      toast("err", "请输入配对码");
      return;
    }
    if (!activeProfile) {
      toast("err", "请先在顶部 ⚙ 添加并选择一个服务端");
      return;
    }
    setPairing(true);
    try {
      await pairServer(token);
      toast("ok", "配对成功！服务端已注册主公钥。");
      setPairingToken("");
    } catch (e) {
      toast("err", errMsg(e));
    } finally {
      setPairing(false);
    }
  }

  // ── Render ─────────────────────────────────────────────────────────────────
  const hasKey = masterSt?.hasKey ?? false;
  const pubKey = masterSt?.publicKeyB64 ?? null;

  return (
    <div className="mx-auto max-w-xl space-y-6 p-6">
      <h2 className="text-lg font-bold text-slate-100">连接</h2>

      {/* ── Current server (read-only) ────────────────────────────────── */}
      <div className="card p-4 space-y-3">
        <div className="flex items-center justify-between">
          <h3 className="text-sm font-semibold text-slate-300">当前服务端</h3>
          <span className="text-xs text-slate-600">管理多个服务端请用顶部 ⚙</span>
        </div>

        {loadingServer ? (
          <Spinner label="加载中…" />
        ) : !activeProfile ? (
          <p className="text-xs text-slate-500">
            尚未选择服务端。点击侧边栏顶部的 ⚙ 添加并选择一个服务端。
          </p>
        ) : (
          <div className="rounded-lg border border-slate-500 bg-slate-800/70 px-3 py-2.5">
            <div className="flex items-center gap-2">
              <p className="text-sm font-medium text-slate-100 truncate">{activeProfile.name}</p>
              <span className="badge bg-emerald-700/40 text-emerald-300 text-xs">活动</span>
            </div>
            <p className="text-xs text-slate-400 font-mono truncate mt-0.5">{activeProfile.url}</p>
            <p className="text-xs text-slate-600 mt-0.5">
              {activeProfile.ca ? "✓ 已配置 CA" : "系统信任库"}
            </p>
          </div>
        )}
      </div>

      {/* ── Master Key ───────────────────────────────────────────────── */}
      <div className="card p-4 space-y-3">
        <div className="flex items-center justify-between">
          <h3 className="text-sm font-semibold text-slate-300">主密钥</h3>
          {loadingMaster ? null : hasKey ? (
            <span className="badge bg-emerald-700/40 text-emerald-300">已生成</span>
          ) : (
            <span className="badge bg-slate-700 text-slate-400">未生成</span>
          )}
        </div>

        {loadingMaster ? (
          <Spinner label="加载中…" />
        ) : hasKey && pubKey ? (
          <div>
            <label className="label">公钥（Base64）</label>
            <div className="rounded-lg bg-slate-900 border border-slate-700 p-2.5 font-mono text-xs text-slate-300 break-all">
              {pubKey}
            </div>
            <p className="mt-1.5 text-xs text-slate-500">
              此公钥将在配对时注册到服务端。私钥存储于本机钥匙串，永不离开本机。
            </p>
          </div>
        ) : (
          <div className="space-y-2">
            <p className="text-xs text-slate-500">
              尚未生成主密钥。生成后将存储在本机钥匙串，用于签名所有控制面请求。
            </p>
            <button
              className="btn-primary"
              onClick={handleGenerateKey}
              disabled={generatingKey}
            >
              {generatingKey ? <Spinner label="生成中…" /> : "生成主密钥"}
            </button>
          </div>
        )}
      </div>

      {/* ── Pairing ──────────────────────────────────────────────────── */}
      <div className="card p-4 space-y-3">
        <h3 className="text-sm font-semibold text-slate-300">配对服务端</h3>

        {!hasKey && (
          <p className="text-xs text-amber-400">请先生成主密钥再进行配对。</p>
        )}
        {!activeProfile && (
          <p className="text-xs text-amber-400">请先在顶部 ⚙ 添加并选择一个活动服务端。</p>
        )}
        {activeProfile && (
          <p className="text-xs text-slate-500">
            将对活动服务端配对：
            <span className="font-medium text-slate-300 ml-1">{activeProfile.name}</span>
            <span className="font-mono text-slate-500 ml-1">({activeProfile.url})</span>
          </p>
        )}

        <div>
          <label className="label">一次性配对码（pairing_token）</label>
          <div className="flex gap-2">
            <input
              className="input flex-1 font-mono"
              type="text"
              placeholder="server.toml 中的 pairing_token"
              value={pairingToken}
              onChange={(e) => setPairingToken(e.target.value)}
              onKeyDown={(e) => e.key === "Enter" && handlePair()}
              disabled={!hasKey || !activeProfile}
            />
            <button
              className="btn-primary whitespace-nowrap"
              onClick={handlePair}
              disabled={pairing || !hasKey || !activeProfile || !pairingToken.trim()}
            >
              {pairing ? <Spinner /> : "配对"}
            </button>
          </div>
          <p className="mt-1.5 text-xs text-slate-500">
            配对成功后服务端将注册本控制台公钥，后续所有控制面请求由主密钥签名鉴权。
          </p>
        </div>
      </div>
    </div>
  );
}
