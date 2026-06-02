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
}

// ─── Master key commands ───────────────────────────────────────────────────────

export const masterStatus = () => invoke<MasterStatus>("master_status");

export const generateMasterKey = () =>
  invoke<GeneratedKey>("generate_master_key");

// ─── Settings commands ────────────────────────────────────────────────────────

export const getServerUrl = () =>
  invoke<string | null>("get_server_url_cmd");

export const setServerUrl = (url: string) =>
  invoke<void>("set_server_url_cmd", { url });

// ─── Pairing commands ─────────────────────────────────────────────────────────

/** Pair the console with the server using a one-time pairing token. */
export const pairServer = (pairingToken: string) =>
  invoke<void>("pair_server", { pairingToken });

// ─── Machine commands ─────────────────────────────────────────────────────────

/** Issue a new machine identity token. */
export const issueMachine = (name: string) =>
  invoke<IssuedMachine>("issue_machine", { name });

/** List all known machines from the server. */
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

// ─── Error helper ─────────────────────────────────────────────────────────────

export function errMsg(e: unknown): string {
  if (typeof e === "string") return e;
  if (e instanceof Error) return e.message;
  return String(e);
}
