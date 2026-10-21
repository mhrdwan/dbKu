import { api, errorMessage } from "../api";
import type { ConnectionInput, ConnectionProfile, Driver, ServerInfo } from "../types";
import { savedQueries } from "./saved.svelte";
import { schema } from "./schema.svelte";

export type ConnStatus = "idle" | "connecting" | "connected" | "error";

export const DRIVERS: Record<Driver, { label: string; mono: string; color: string; port: number }> = {
  postgres: { label: "PostgreSQL", mono: "Pg", color: "#336791", port: 5432 },
  mysql: { label: "MySQL / MariaDB", mono: "My", color: "#c27100", port: 3306 },
  sqlite: { label: "SQLite", mono: "Lt", color: "#0f80cc", port: 0 },
  mssql: { label: "SQL Server", mono: "Ms", color: "#b5312f", port: 1433 },
};

/** Connection colours users pick to tell servers apart (e.g. red = production). */
export const CONNECTION_COLORS = ["#d64545", "#e07b1a", "#c9a20a", "#2f9e5b", "#1d8fb3", "#4f5bd5", "#9b4fd1", "#7a8580"];

class Connections {
  profiles = $state<ConnectionProfile[]>([]);
  status = $state<Record<string, ConnStatus>>({});
  info = $state<Record<string, ServerInfo>>({});
  errors = $state<Record<string, string>>({});
  loaded = $state(false);

  byId(id: string | null | undefined): ConnectionProfile | undefined {
    return this.profiles.find((p) => p.id === id);
  }

  isConnected(id: string | null | undefined): boolean {
    return !!id && this.status[id] === "connected";
  }

  get connected(): ConnectionProfile[] {
    return this.profiles.filter((p) => this.status[p.id] === "connected");
  }

  async load(): Promise<void> {
    const list = await api.listConnections();
    this.profiles = [...list].sort((a, b) => a.name.localeCompare(b.name));
    this.loaded = true;
  }

  async save(input: ConnectionInput): Promise<ConnectionProfile> {
    const saved = await api.saveConnection(input);
    const others = this.profiles.filter((p) => p.id !== saved.id);
    this.profiles = [...others, saved].sort((a, b) => a.name.localeCompare(b.name));
    // Saving drops the backend session so new settings apply immediately.
    this.status = { ...this.status, [saved.id]: "idle" };
    schema.forget(saved.id);
    return saved;
  }

  async remove(id: string): Promise<void> {
    await api.deleteConnection(id);
    this.profiles = this.profiles.filter((p) => p.id !== id);
    const { [id]: _s, ...status } = this.status;
    this.status = status;
    schema.forget(id);
    savedQueries.forgetConnection(id);
  }

  /** Returns null on success, or the error message. */
  async connect(id: string, password?: string): Promise<string | null> {
    this.status = { ...this.status, [id]: "connecting" };
    try {
      const info = await api.connect(id, password);
      this.info = { ...this.info, [id]: info };
      this.status = { ...this.status, [id]: "connected" };
      const { [id]: _e, ...errors } = this.errors;
      this.errors = errors;
      return null;
    } catch (e) {
      const message = errorMessage(e);
      this.status = { ...this.status, [id]: "error" };
      this.errors = { ...this.errors, [id]: message };
      return message;
    }
  }

  async disconnect(id: string): Promise<void> {
    await api.disconnect(id);
    this.status = { ...this.status, [id]: "idle" };
    schema.forget(id);
  }

  /** Needs a password prompt before connecting. */
  needsPassword(p: ConnectionProfile): boolean {
    return p.driver !== "sqlite" && !p.savePassword;
  }
}

export const connections = new Connections();

export function blankProfile(driver: Driver = "postgres"): ConnectionProfile {
  return {
    id: "",
    name: "",
    driver,
    host: driver === "sqlite" ? "" : "localhost",
    port: DRIVERS[driver].port,
    user: driver === "postgres" ? "postgres" : driver === "mssql" ? "sa" : driver === "mysql" ? "root" : "",
    database: "",
    sslMode: "prefer",
    trustServerCert: false,
    legacyTls: false,
    readOnly: false,
    color: null,
    group: null,
    savePassword: true,
  };
}
