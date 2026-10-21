// Saved queries, persisted by the backend (saved_queries.json).

import { api } from "../api";
import type { SavedQuery } from "../types";

class SavedQueries {
  list = $state<SavedQuery[]>([]);

  async load(): Promise<void> {
    this.list = await api.listSavedQueries();
  }

  forConnection(connId: string): SavedQuery[] {
    return this.list.filter((q) => q.connId === connId || q.connId === null);
  }

  byId(id: string | null | undefined): SavedQuery | undefined {
    return this.list.find((q) => q.id === id);
  }

  async save(query: Partial<SavedQuery> & { name: string; sql: string }): Promise<SavedQuery> {
    const saved = await api.saveQuery(query);
    const others = this.list.filter((q) => q.id !== saved.id);
    this.list = [...others, saved].sort((a, b) => a.name.localeCompare(b.name, undefined, { sensitivity: "base" }));
    return saved;
  }

  async remove(id: string): Promise<void> {
    await api.deleteSavedQuery(id);
    this.list = this.list.filter((q) => q.id !== id);
  }

  /** The backend drops a deleted connection's queries; mirror that. */
  forgetConnection(connId: string) {
    this.list = this.list.filter((q) => q.connId !== connId);
  }
}

export const savedQueries = new SavedQueries();
