import { useEffect, useState } from "react";
import {
  masterStatus,
  generateMasterKey,
  getServerUrl,
  setServerUrl,
  pairServer,
  errMsg,
  type MasterStatus,
} from "../api";
import { Spinner, useToast } from "../ui";

export default function ConnectPage() {
  const toast = useToast();

  // ── State ──────────────────────────────────────────────────────────────────
  const [status, setStatus] = useState<MasterStatus | null>(null);
  const [serverUrl, setServerUrlState] = useState("");
  const [savedUrl, setSavedUrl] = useState<string | null>(null);
  const [pairingToken, setPairingToken] = useState("");

  const [loadingStatus, setLoadingStatus] = useState(true);
  const [savingUrl, setSavingUrl] = useState(false);
  const [generatingKey, setGeneratingKey] = useState(false);
  const [pairing, setPairing] = useState(false);

  // ── Boot ───────────────────────────────────────────────────────────────────
  useEffect(() => {
    Promise.all([
      masterStatus().catch(() => null),
      getServerUrl().catch(() => null),
    ]).then(([ms, url]) => {
      if (ms) setStatus(ms);
      if (url) {
        setSavedUrl(url);
        setServerUrlState(url);
      }
      setLoadingStatus(false);
    });
  }, []);

  // ── Handlers ───────────────────────────────────────────────────────────────
  async function handleSaveUrl() {
    const trimmed = serverUrl.trim();
    if (!trimmed) return;
    setSavingUrl(true);
    try {
      await setServerUrl(trimmed);
      setSavedUrl(trimmed);
      toast("ok", "服务端地址已保存");
    } catch (e) {
      toast("err", errMsg(e));
    } finally {
      setSavingUrl(false);
    }
  }

  async function handleGenerateKey() {
    setGeneratingKey(true);
    try {
      const result = await generateMasterKey();
      setStatus({ hasKey: true, publicKeyB64: result.publicKeyB64 });
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
  if (loadingStatus) {
    return (
      <div className="flex h-full items-center justify-center">
        <Spinner label="加载中…" />
      </div>
    );
  }

  const hasKey = status?.hasKey ?? false;
  const pubKey = status?.publicKeyB64 ?? null;
  const urlSaved = !!savedUrl;

  return (
    <div className="mx-auto max-w-xl space-y-6 p-6">
      <h2 className="text-lg font-bold text-slate-100">连接 &amp; 配对</h2>

      {/* ── Server URL ────────────────────────────────────────────────── */}
      <div className="card p-4 space-y-3">
        <h3 className="text-sm font-semibold text-slate-300">服务端地址</h3>
        <div className="flex gap-2">
          <input
            className="input flex-1"
            type="url"
            placeholder="http://192.168.1.10:3000"
            value={serverUrl}
            onChange={(e) => setServerUrlState(e.target.value)}
            onKeyDown={(e) => e.key === "Enter" && handleSaveUrl()}
          />
          <button
            className="btn-primary whitespace-nowrap"
            onClick={handleSaveUrl}
            disabled={savingUrl || !serverUrl.trim()}
          >
            {savingUrl ? <Spinner /> : "保存"}
          </button>
        </div>
        {urlSaved && (
          <p className="text-xs text-emerald-400">
            ✓ 当前: <span className="font-mono">{savedUrl}</span>
          </p>
        )}
      </div>

      {/* ── Master Key ───────────────────────────────────────────────── */}
      <div className="card p-4 space-y-3">
        <div className="flex items-center justify-between">
          <h3 className="text-sm font-semibold text-slate-300">主密钥</h3>
          {hasKey ? (
            <span className="badge bg-emerald-700/40 text-emerald-300">已生成</span>
          ) : (
            <span className="badge bg-slate-700 text-slate-400">未生成</span>
          )}
        </div>

        {hasKey && pubKey ? (
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
          <p className="text-xs text-amber-400">
            请先生成主密钥再进行配对。
          </p>
        )}
        {!urlSaved && (
          <p className="text-xs text-amber-400">
            请先保存服务端地址再进行配对。
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
              disabled={!hasKey || !urlSaved}
            />
            <button
              className="btn-primary whitespace-nowrap"
              onClick={handlePair}
              disabled={pairing || !hasKey || !urlSaved || !pairingToken.trim()}
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
