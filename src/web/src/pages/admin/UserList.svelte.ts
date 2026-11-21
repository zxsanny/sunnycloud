import { api, type AdminUserList, type ApiError } from '$lib/api';

export type AdminUser = AdminUserList['items'][number];

const PAGE = 100;

/** The admin's user list, read a page at a time; the Users and Quotas tabs each own one. */
export class UserList {
  items = $state<AdminUser[] | null>(null);
  error = $state<ApiError | null>(null);
  hasMore = $state(false);
  moreError = $state<ApiError | null>(null);
  loadingMore = $state(false);

  async load() {
    this.error = null;
    const result = await api.getAdminUsers(0, PAGE);
    if (result.ok) {
      this.items = result.value.items;
      this.hasMore = result.value.items.length === PAGE;
    } else {
      this.error = result.error;
    }
  }

  async more() {
    if (this.items === null) return;
    this.loadingMore = true;
    this.moreError = null;
    const result = await api.getAdminUsers(this.items.length, PAGE);
    this.loadingMore = false;
    if (result.ok) {
      this.items = [...this.items, ...result.value.items];
      this.hasMore = result.value.items.length === PAGE;
    } else {
      this.moreError = result.error;
    }
  }

  /** Replaces one row after the server confirmed the change with its 204. */
  patch(id: string, change: Partial<AdminUser>) {
    if (this.items === null) return;
    this.items = this.items.map((user) => (user.id === id ? { ...user, ...change } : user));
  }
}
