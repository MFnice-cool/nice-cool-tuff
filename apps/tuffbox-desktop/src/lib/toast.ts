import { writable } from "svelte/store";

export type ToastType = "success" | "error" | "info" | "warning";
export interface ToastAction { label: string; run: () => void; }
export interface Toast {
  id: number;
  message: string;
  type: ToastType;
  duration: number;
  timestamp: number;
  actions?: ToastAction[];
}

const MAX_VISIBLE = 4;

let nextId = 0;

function createToasts() {
  const { subscribe, update } = writable<Toast[]>([]);
  return {
    subscribe,
    show(msg: string, type: ToastType = "info", duration = 5000, actions?: ToastAction[]) {
      const id = nextId++;
      const toast: Toast = { id, message: msg, type, duration, timestamp: Date.now(), actions };
      update(t => {
        const next = [...t, toast];
        return next.length > MAX_VISIBLE ? next.slice(next.length - MAX_VISIBLE) : next;
      });
      if (duration > 0) setTimeout(() => { update(t => t.filter(x => x.id !== id)); }, duration);
    },
    dismiss(id: number) { update(t => t.filter(x => x.id !== id)); },
    success(msg: string, d = 4000, actions?: ToastAction[]) { this.show(msg, "success", d, actions); },
    error(msg: string, d = 8000, actions?: ToastAction[]) { this.show(msg, "error", d, actions); },
    info(msg: string, d = 4000, actions?: ToastAction[]) { this.show(msg, "info", d, actions); },
    warning(msg: string, d = 6000, actions?: ToastAction[]) { this.show(msg, "warning", d, actions); },
  };
}

export const toasts = createToasts();
