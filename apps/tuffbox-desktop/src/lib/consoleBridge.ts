/**
 * JS console → Rust tracing bridge (tauri-plugin-tracing `plugin:tracing|log`).
 *
 * Replaces the plugin's `interceptConsole()`: its forwarder does
 * `invoke(...).catch(console.error)` while `console.error` is itself
 * intercepted, so a rejected forward (e.g. `tracing:allow-log` missing from
 * the capability) re-enters the bridge, gets rejected again, and never stops.
 * Every console line spawned one more endless IPC chain; the flood starved
 * all other IPC replies and Diagnose / AI sat on "Loading…" forever.
 *
 * Invariants kept here:
 * - a failed forward is reported through the ORIGINAL console, never the
 *   wrapped one, and permanently disables the bridge (console restored);
 * - only info/warn/error are forwarded: the Rust subscriber is capped at
 *   INFO, so debug/trace forwards were IPC round-trips that got dropped.
 */

type InvokeFn = (cmd: string, args: Record<string, unknown>) => Promise<unknown>;

type ConsoleMethod = "log" | "debug" | "info" | "warn" | "error";

/** Numeric levels of tauri-plugin-tracing's `LogLevel`. */
const LEVELS: Partial<Record<ConsoleMethod, number>> = {
  info: 3,
  warn: 4,
  error: 5,
};

const MAX_MESSAGE_CHARS = 8_000;

function stringifyArg(value: unknown): string {
  if (typeof value === "string") return value;
  if (value instanceof Error) return value.stack || `${value.name}: ${value.message}`;
  if (value === undefined) return "undefined";
  if (typeof value === "bigint") return value.toString();
  if (typeof value === "function") return `[function ${value.name || "anonymous"}]`;
  try {
    const seen = new WeakSet<object>();
    return (
      JSON.stringify(value, (_k, v) => {
        if (typeof v === "bigint") return v.toString();
        if (typeof v === "object" && v !== null) {
          if (seen.has(v)) return "[Circular]";
          seen.add(v);
        }
        return v;
      }) ?? String(value)
    );
  } catch {
    return String(value);
  }
}

export function formatConsoleArgs(args: unknown[]): string {
  const text = args.map(stringifyArg).join(" ");
  return text.length > MAX_MESSAGE_CHARS ? `${text.slice(0, MAX_MESSAGE_CHARS)}…` : text;
}

export interface ConsoleBridgeHandle {
  /** Restore the original console methods (idempotent). */
  restore: () => void;
  /** True while console calls are still being forwarded. */
  isActive: () => boolean;
}

export function installConsoleBridge(
  invoke: InvokeFn,
  target: Pick<Console, ConsoleMethod> = console,
): ConsoleBridgeHandle {
  const methods = Object.keys(LEVELS) as ConsoleMethod[];
  const originals = new Map<ConsoleMethod, (...args: unknown[]) => void>();
  for (const m of methods) originals.set(m, target[m]);
  const callOriginal = (m: ConsoleMethod, args: unknown[]) => originals.get(m)!.apply(target, args);

  let active = true;
  let forwarding = false;

  const restore = () => {
    if (!active) return;
    active = false;
    for (const m of methods) target[m] = originals.get(m)!;
  };

  const disable = (reason: unknown) => {
    if (!active) return;
    restore();
    callOriginal("warn", ["[tuffbox] console → tracing bridge disabled:", reason]);
  };

  for (const m of methods) {
    const level = LEVELS[m]!;
    target[m] = (...args: unknown[]) => {
      callOriginal(m, args);
      // Synchronous re-entry (an invoke shim that logs) must not recurse.
      if (!active || forwarding) return;
      forwarding = true;
      try {
        invoke("plugin:tracing|log", {
          level,
          message: [formatConsoleArgs(args)],
          callStack: new Error().stack ?? null,
        }).catch(disable);
      } catch (e) {
        disable(e);
      } finally {
        forwarding = false;
      }
    };
  }

  return { restore, isActive: () => active };
}
