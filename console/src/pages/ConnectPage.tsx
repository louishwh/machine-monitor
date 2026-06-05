import { useEffect, useState } from "react";
import {
  masterStatus,
  generateMasterKey,
  listServers,
  addServer,
  removeServer,
  setActiveServer,
  pairServer,
  errMsg,
  type MasterStatus,
  type ServerProfile,
} from "../api";
import { Spinner, useToast } from "../ui";

interface Props {
  onServersChanged: () => void;
  activeServerId: string | null;
  onActiveChange: (id: string) => void;
}

export default function ConnectPage({
  onServersChanged,
  activeServerId,
  onActiveChange,
}: Props) {
  const toast = useToast();

  // ── Master key state ───────────────────────────────────────────────────────
  const [masterSt, setMasterSt] = useState<MasterStatus | null>(null);
  const [loadingMaster, setLoadingMaster] = useState(true);
  const [generatingKey, setGeneratingKey] = useState(false);

  // ── Server list state ──────────────────────────────────────────────────────
  const [servers, setServers] = useState<ServerProfile[]>([]);
  const [loadingServers, setLoadingServers] = useState(true);

  // ── Add-server form state ──────────────────────────────────────────────────
  const [addName, setAddName] = useState("");
  const [addUrl, setAddUrl] = useState("");
  const [addCa, setAddCa] = useState("");
  const [addingServer, setAddingServer] = useState(false);
  const [showAddForm, setShowAddForm] = useState(false);

  // ── Pair state ─────────────────────────────────────────────────────────────
  const [pairingToken, setPairingToken] = useState("");
  const [pairing, setPairing] = useState(false);

  // ── Boot ───────────────────────────────────────────────────────────────────
  useEffect(() => {
    masterStatus()
      .then((ms) => setMasterSt(ms))
      .catch(() => {})
      .finally(() => setLoadingMaster(false));

    refreshServers();
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, []);

  function refreshServers() {
    setLoadingServers(true);
    listServers()
      .then((list) => setServers(list))
      .catch(() => {})
      .finally(() => setLoadingServers(false));
  }

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

  async function handleAddServer() {
    const name = addName.trim();
    const url = addUrl.trim();
    if (!name || !url) {
      toast("err", "请填写服务端名称和地址");
      return;
    }
    setAddingServer(true);
    try {
      const profile = await addServer(name, url, addCa.trim() || null);
      toast("ok", `服务端「${profile.name}」已添加`);
      setAddName("");
      setAddUrl("");
      setAddCa("");
      setShowAddForm(false);
      refreshServers();
      onServersChanged();
      // Auto-activate the first added server
      if (servers.length === 0) {
        onActiveChange(profile.id);
      }
    } catch (e) {
      toast("err", errMsg(e));
    } finally {
      setAddingServer(false);
    }
  }

  async function handleRemoveServer(id: string) {
    const profile = servers.find((s) => s.id === id);
    if (!profile) return;
    if (!window.confirm(`确认删除服务端「${profile.name}」？`)) return;
    try {
      await removeServer(id);
      toast("ok", `已删除「${profile.name}」`);
      refreshServers();
      onServersChanged();
    } catch (e) {
      toast("err", errMsg(e));
    }
  }

  async function handleSetActive(id: string) {
    try {
      await setActiveServer(id);
      onActiveChange(id);
      toast("ok", "已切换活动服务端");
    } catch (e) {
      toast("err", errMsg(e));
    }
  }

  async function handlePair() {
    const token = pairingToken.trim();
    if (!token) {
      toast("err", "请输入配对码");
      return;
    }
    if (!activeServerId) {
      toast("err", "请先选择或添加一个服务端");
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
  const activeProfile = servers.find((s) => s.id === activeServerId) ?? null;

  return (
    <div className="mx-auto max-w-xl space-y-6 p-6">
      <h2 className="text-lg font-bold text-slate-100">连接 &amp; 配对</h2>

      {/* ── Server List ───────────────────────────────────────────────── */}
      <div className="card p-4 space-y-3">
        <div className="flex items-center justify-between">
          <h3 className="text-sm font-semibold text-slate-300">服务端列表</h3>
          <button
            className="btn-ghost text-xs py-1 px-2"
            onClick={() => setShowAddForm((v) => !v)}
          >
            {showAddForm ? "取消" : "+ 添加服务端"}
          </button>
        </div>

        {loadingServers ? (
          <Spinner label="加载中…" />
        ) : servers.length === 0 ? (
          <p className="text-xs text-slate-500">暂无服务端配置。点击「添加服务端」开始。</p>
        ) : (
          <div className="space-y-2">
            {servers.map((s) => {
              const isActive = s.id === activeServerId;
              return (
                <div
                  key={s.id}
                  className={`rounded-lg border px-3 py-2.5 flex items-start justify-between gap-3 ${
                    isActive
                      ? "border-slate-500 bg-slate-800/70"
                      : "border-slate-700 bg-slate-800/30"
                  }`}
                >
                  <div className="flex-1 min-w-0">
                    <div className="flex items-center gap-2">
                      <p className="text-sm font-medium text-slate-100 truncate">{s.name}</p>
                      {isActive && (
                        <span className="badge bg-emerald-700/40 text-emerald-300 text-xs">
                          活动
                        </span>
                      )}
                    </div>
                    <p className="text-xs text-slate-400 font-mono truncate mt-0.5">{s.url}</p>
                    <p className="text-xs text-slate-600 mt-0.5">
                      {s.ca ? "✓ 已配置 CA" : "系统信任库"}
                    </p>
                  </div>
                  <div className="flex gap-1.5 shrink-0 mt-0.5">
                    {!isActive && (
                      <button
                        className="btn-ghost text-xs py-0.5 px-2"
                        onClick={() => handleSetActive(s.id)}
                      >
                        设为活动
                      </button>
                    )}
                    <button
                      className="btn text-xs py-0.5 px-2 bg-red-900/20 border border-red-700/40 text-red-400 hover:bg-red-800/30"
                      onClick={() => handleRemoveServer(s.id)}
                    >
                      删除
                    </button>
                  </div>
                </div>
              );
            })}
          </div>
        )}

        {/* ── Add server form ──────────────────────────────────────── */}
        {showAddForm && (
          <div className="rounded-lg border border-slate-600 bg-slate-800/60 p-3 space-y-3">
            <p className="text-xs font-semibold text-slate-400">新增服务端</p>
            <div className="space-y-2">
              <div>
                <label className="label">名称</label>
                <input
                  className="input w-full"
                  type="text"
                  placeholder="生产服务器"
                  value={addName}
                  onChange={(e) => setAddName(e.target.value)}
                />
              </div>
              <div>
                <label className="label">地址 (URL)</label>
                <input
                  className="input w-full"
                  type="url"
                  placeholder="http://192.168.1.10:3000"
                  value={addUrl}
                  onChange={(e) => setAddUrl(e.target.value)}
                />
              </div>
              <div>
                <label className="label">CA 证书 PEM（可选，留空用系统信任库）</label>
                <textarea
                  className="input w-full font-mono text-xs"
                  rows={4}
                  placeholder={"-----BEGIN CERTIFICATE-----\n…\n-----END CERTIFICATE-----"}
                  value={addCa}
                  onChange={(e) => setAddCa(e.target.value)}
                />
              </div>
            </div>
            <button
              className="btn-primary w-full"
              onClick={handleAddServer}
              disabled={addingServer || !addName.trim() || !addUrl.trim()}
            >
              {addingServer ? <Spinner label="添加中…" /> : "添加"}
            </button>
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
          <p className="text-xs text-amber-400">请先添加并选择一个活动服务端。</p>
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
