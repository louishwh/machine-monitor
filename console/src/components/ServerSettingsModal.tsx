import { useEffect, useState } from "react";
import {
  listServers,
  addServer,
  updateServer,
  removeServer,
  setActiveServer,
  errMsg,
  type ServerProfile,
} from "../api";
import { Modal, Spinner, useToast } from "../ui";

interface Props {
  activeServerId: string | null;
  onClose: () => void;
  /** Called after any change (add/edit/remove/set-active) so the parent reloads. */
  onChanged: () => void;
  /** Called when the active server changes from within the modal. */
  onActiveChange: (id: string) => void;
}

type Draft = { name: string; url: string; ca: string };

export default function ServerSettingsModal({
  activeServerId,
  onClose,
  onChanged,
  onActiveChange,
}: Props) {
  const toast = useToast();
  const [servers, setServers] = useState<ServerProfile[]>([]);
  const [loading, setLoading] = useState(true);
  const [busy, setBusy] = useState(false);

  // id of the profile being edited (or "__new__" for the add form)
  const [editingId, setEditingId] = useState<string | null>(null);
  const [draft, setDraft] = useState<Draft>({ name: "", url: "", ca: "" });

  function refresh() {
    setLoading(true);
    listServers()
      .then((list) => setServers(list))
      .catch((e) => toast("err", errMsg(e)))
      .finally(() => setLoading(false));
  }

  useEffect(() => {
    refresh();
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, []);

  function startEdit(s: ServerProfile) {
    setEditingId(s.id);
    setDraft({ name: s.name, url: s.url, ca: s.ca ?? "" });
  }

  function startAdd() {
    setEditingId("__new__");
    setDraft({ name: "", url: "", ca: "" });
  }

  function cancelEdit() {
    setEditingId(null);
    setDraft({ name: "", url: "", ca: "" });
  }

  async function saveDraft() {
    const name = draft.name.trim();
    const url = draft.url.trim();
    const ca = draft.ca.trim() || null;
    if (!name || !url) {
      toast("err", "请填写名称和地址");
      return;
    }
    setBusy(true);
    try {
      if (editingId === "__new__") {
        const created = await addServer(name, url, ca);
        toast("ok", `服务端「${created.name}」已添加`);
        if (servers.length === 0) onActiveChange(created.id);
      } else if (editingId) {
        await updateServer(editingId, name, url, ca);
        toast("ok", "已保存");
      }
      cancelEdit();
      refresh();
      onChanged();
    } catch (e) {
      toast("err", errMsg(e));
    } finally {
      setBusy(false);
    }
  }

  async function handleSetActive(id: string) {
    try {
      await setActiveServer(id);
      onActiveChange(id);
      onChanged();
      toast("ok", "已切换活动服务端");
    } catch (e) {
      toast("err", errMsg(e));
    }
  }

  async function handleRemove(s: ServerProfile) {
    if (!window.confirm(`确认删除服务端「${s.name}」？`)) return;
    try {
      await removeServer(s.id);
      toast("ok", `已删除「${s.name}」`);
      if (editingId === s.id) cancelEdit();
      refresh();
      onChanged();
    } catch (e) {
      toast("err", errMsg(e));
    }
  }

  return (
    <Modal title="服务端配置" onClose={onClose} wide>
      <div className="space-y-3">
        {loading ? (
          <Spinner label="加载中…" />
        ) : servers.length === 0 ? (
          <p className="text-xs text-slate-500">暂无服务端配置。点击下方「+ 添加服务端」开始。</p>
        ) : (
          <div className="space-y-2">
            {servers.map((s) => {
              const isActive = s.id === activeServerId;
              const isEditing = editingId === s.id;
              return (
                <div
                  key={s.id}
                  className={`rounded-lg border px-3 py-2.5 ${
                    isActive
                      ? "border-slate-500 bg-slate-800/70"
                      : "border-slate-700 bg-slate-800/30"
                  }`}
                >
                  {isEditing ? (
                    <DraftForm
                      draft={draft}
                      setDraft={setDraft}
                      busy={busy}
                      onSave={saveDraft}
                      onCancel={cancelEdit}
                    />
                  ) : (
                    <div className="flex items-start justify-between gap-3">
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
                          className="btn-ghost text-xs py-0.5 px-2"
                          onClick={() => startEdit(s)}
                        >
                          编辑
                        </button>
                        <button
                          className="btn text-xs py-0.5 px-2 bg-red-900/20 border border-red-700/40 text-red-400 hover:bg-red-800/30"
                          onClick={() => handleRemove(s)}
                        >
                          删除
                        </button>
                      </div>
                    </div>
                  )}
                </div>
              );
            })}
          </div>
        )}

        {/* Add form / button */}
        {editingId === "__new__" ? (
          <div className="rounded-lg border border-slate-600 bg-slate-800/60 p-3">
            <p className="text-xs font-semibold text-slate-400 mb-2">新增服务端</p>
            <DraftForm
              draft={draft}
              setDraft={setDraft}
              busy={busy}
              onSave={saveDraft}
              onCancel={cancelEdit}
            />
          </div>
        ) : (
          <button className="btn-ghost text-xs py-1 px-2" onClick={startAdd}>
            + 添加服务端
          </button>
        )}
      </div>
    </Modal>
  );
}

function DraftForm({
  draft,
  setDraft,
  busy,
  onSave,
  onCancel,
}: {
  draft: Draft;
  setDraft: (d: Draft) => void;
  busy: boolean;
  onSave: () => void;
  onCancel: () => void;
}) {
  return (
    <div className="space-y-2">
      <div>
        <label className="label">名称</label>
        <input
          className="input w-full"
          type="text"
          placeholder="生产服务器"
          value={draft.name}
          onChange={(e) => setDraft({ ...draft, name: e.target.value })}
          autoFocus
        />
      </div>
      <div>
        <label className="label">地址 (URL)</label>
        <input
          className="input w-full font-mono text-xs"
          type="url"
          placeholder="https://monitor.example.com"
          value={draft.url}
          onChange={(e) => setDraft({ ...draft, url: e.target.value })}
        />
      </div>
      <div>
        <label className="label">CA 证书 PEM（可选，留空用系统信任库）</label>
        <textarea
          className="input w-full font-mono text-xs"
          rows={3}
          placeholder={"-----BEGIN CERTIFICATE-----\n…\n-----END CERTIFICATE-----"}
          value={draft.ca}
          onChange={(e) => setDraft({ ...draft, ca: e.target.value })}
        />
      </div>
      <div className="flex gap-2">
        <button
          className="btn-primary text-xs py-1 px-3"
          onClick={onSave}
          disabled={busy || !draft.name.trim() || !draft.url.trim()}
        >
          {busy ? <Spinner label="保存中…" /> : "保存"}
        </button>
        <button className="btn-ghost text-xs py-1 px-3" onClick={onCancel} disabled={busy}>
          取消
        </button>
      </div>
    </div>
  );
}
