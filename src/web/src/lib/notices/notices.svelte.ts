export interface Notice {
  /** A notice with the same key replaces the one on screen. */
  key: string;
  variant: 'info' | 'warning' | 'danger';
  title: string;
  text: string;
  /** Epoch milliseconds; the notice counts down to it and removes itself. `text` is then ignored. */
  retryAt?: number;
  requestId?: string;
  action?: 'reload';
  dismissible?: boolean;
}

class NoticeStore {
  items = $state<Notice[]>([]);

  show(notice: Notice) {
    this.items = [...this.items.filter((item) => item.key !== notice.key), notice];
  }

  dismiss(key: string) {
    this.items = this.items.filter((item) => item.key !== key);
  }
}

export const notices = new NoticeStore();
