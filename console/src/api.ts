import { invoke } from "@tauri-apps/api/core";

// ─── Types ────────────────────────────────────────────────────────────────────

export interface MasterStatus {
  hasKey: boolean;
  publicKeyB64: string | null;
}

export interface GeneratedKey {
  publicKeyB64: string;
}

export interface IssuedMachine {
  machineId: string;
  name: string;
  token: string;
}

export interface Machine {
  id: string;
  name: string;
  hostname: string;
  os: string;
  agentVersion: string;
  status: string;
  lastSeen: string;
  online: boolean;
  shellEnabled: boolean;
  summary?: {
    cpu_pct?: number;
    mem_pct?: number;
    disk_pct?: number;
    uptime_secs?: number;
    logical_cores?: number;
    mem_total_bytes?: number;
    disk_total_bytes?: number;
  } | null;
}

export interface ShellResult {
  exit: number;
  stdout: string;
  stderr: string;
}

export interface AuditEntry {
  id: string;
  machine_id: string;
  kind: string;
  request: string;
  exit: number | null;
  output: string;
  created_at: string;
}

/** A named server connection profile stored in the OS keychain. */
export interface ServerProfile {
  id: string;
  name: string;
  url: string;
  ca: string | null;
}

// ─── Master key commands ───────────────────────────────────────────────────────

export const masterStatus = () => invoke<MasterStatus>("master_status");

export const generateMasterKey = () =>
  invoke<GeneratedKey>("generate_master_key");

// ─── Legacy settings commands (kept for backward compat) ──────────────────────

export const getServerUrl = () =>
  invoke<string | null>("get_server_url_cmd");

export const setServerUrl = (url: string) =>
  invoke<void>("set_server_url_cmd", { url });

export const getServerCa = () =>
  invoke<string | null>("get_server_ca_cmd");

export const setServerCa = (pem: string) =>
  invoke<void>("set_server_ca_cmd", { pem });

// ─── Multi-server profile commands ───────────────────────────────────────────

export const listServers = () =>
  invoke<ServerProfile[]>("list_servers_cmd");

export const addServer = (name: string, url: string, ca: string | null) =>
  invoke<ServerProfile>("add_server_cmd", { name, url, ca });

export const updateServer = (
  id: string,
  name: string,
  url: string,
  ca: string | null
) => invoke<ServerProfile>("update_server_cmd", { id, name, url, ca });

export const removeServer = (id: string) =>
  invoke<void>("remove_server_cmd", { id });

export const setActiveServer = (id: string) =>
  invoke<void>("set_active_server_cmd", { id });

export const getActiveServer = () =>
  invoke<ServerProfile | null>("get_active_server_cmd");

// ─── Pairing commands ─────────────────────────────────────────────────────────

/** Pair the console with the ACTIVE server profile using a one-time pairing token. */
export const pairServer = (pairingToken: string) =>
  invoke<void>("pair_server", { pairingToken });

// ─── Machine commands ─────────────────────────────────────────────────────────

/** Issue a new machine identity token. */
export const issueMachine = (name: string) =>
  invoke<IssuedMachine>("issue_machine", { name });

/** List all known machines from the active server. */
export const listMachines = () =>
  invoke<Machine[]>("list_machines");

/** Fetch a status snapshot for a machine.
 *  kind ∈ "host" | "cpu" | "mem" | "disk" | "net" | "proc" | "service"
 */
export const machineStatus = (id: string, kind: string) =>
  invoke<unknown>("machine_status", { id, kind });

/** Fetch recent snapshots for a machine. */
export const machineSnapshots = (id: string) =>
  invoke<unknown[]>("machine_snapshots", { id });

// ─── Shell / revoke / audit commands ─────────────────────────────────────────

/** Enable or disable shell access for a machine. */
export const setShell = (id: string, enabled: boolean) =>
  invoke<unknown>("set_shell", { id, enabled });

/** Run a shell command on a machine. Returns exit/stdout/stderr. */
export const runShell = (id: string, command: string) =>
  invoke<ShellResult>("run_shell", { id, command });

/** Revoke a machine (adds to revocations, kicks live connection). */
export const revokeMachine = (id: string) =>
  invoke<unknown>("revoke_machine", { id });

/** Fetch audit log entries, optionally filtered by machine ID. */
export const audit = (machineId?: string) =>
  invoke<AuditEntry[]>("audit", { machineId: machineId ?? null });

// ─── Error helper ─────────────────────────────────────────────────────────────

export function errMsg(e: unknown): string {
  if (typeof e === "string") return e;
  if (e instanceof Error) return e.message;
  return String(e);
}
