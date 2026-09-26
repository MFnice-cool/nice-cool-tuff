import { describe, expect, it, vi } from "vitest";
import { formatConsoleArgs, installConsoleBridge } from "./consoleBridge";

function fakeConsole() {
  return {
    log: vi.fn(),
    debug: vi.fn(),
    info: vi.fn(),
    warn: vi.fn(),
    error: vi.fn(),
  };
}

const flush = () => new Promise((r) => setTimeout(r, 0));

describe("installConsoleBridge", () => {
  it("forwards info/warn/error to plugin:tracing|log and keeps the original output", async () => {
    const target = fakeConsole();
    const origError = target.error;
    const invoke = vi.fn().mockResolvedValue(undefined);
    installConsoleBridge(invoke, target);

    target.error("boom", { a: 1 });
    await flush();

    expect(origError).toHaveBeenCalledWith("boom", { a: 1 });
    expect(invoke).toHaveBeenCalledTimes(1);
    expect(invoke.mock.calls[0][0]).toBe("plugin:tracing|log");
    expect(invoke.mock.calls[0][1]).toMatchObject({ level: 5, message: ['boom {"a":1}'] });
  });

  it("does not forward console.log/debug (Rust subscriber drops < INFO)", async () => {
    const target = fakeConsole();
    const invoke = vi.fn().mockResolvedValue(undefined);
    installConsoleBridge(invoke, target);

    target.log("x");
    target.debug("y");
    await flush();

    expect(invoke).not.toHaveBeenCalled();
  });

  // Regression: plugin's interceptConsole did invoke(...).catch(console.error)
  // through the wrapped console → endless rejected-IPC loop that starved
  // Diagnose / AI replies ("infinite loading").
  it("a rejected forward disables the bridge instead of looping", async () => {
    const target = fakeConsole();
    const origWarn = target.warn;
    const invoke = vi
      .fn()
      .mockRejectedValue("tracing.log not allowed. Permissions associated with this command: tracing:allow-log");
    const bridge = installConsoleBridge(invoke, target);

    target.error("first");
    for (let i = 0; i < 5; i++) await flush();

    expect(invoke).toHaveBeenCalledTimes(1);
    expect(bridge.isActive()).toBe(false);
    expect(target.warn).toBe(origWarn);
    expect(origWarn).toHaveBeenCalledTimes(1);

    target.error("second");
    target.warn("third");
    await flush();
    expect(invoke).toHaveBeenCalledTimes(1);
  });

  it("a synchronously throwing invoke disables the bridge", () => {
    const target = fakeConsole();
    const invoke = vi.fn(() => {
      throw new Error("no ipc");
    });
    const bridge = installConsoleBridge(invoke, target);

    target.warn("x");
    target.warn("y");

    expect(invoke).toHaveBeenCalledTimes(1);
    expect(bridge.isActive()).toBe(false);
  });

  it("synchronous re-entry from inside invoke is not forwarded again", async () => {
    const target = fakeConsole();
    const invoke = vi.fn(() => {
      target.error("logged by invoke shim");
      return Promise.resolve();
    });
    installConsoleBridge(invoke, target);

    target.info("hello");
    await flush();

    expect(invoke).toHaveBeenCalledTimes(1);
  });

  it("restore() puts the original methods back", () => {
    const target = fakeConsole();
    const orig = { ...target };
    const bridge = installConsoleBridge(vi.fn().mockResolvedValue(undefined), target);
    expect(target.error).not.toBe(orig.error);
    bridge.restore();
    expect(target.error).toBe(orig.error);
    expect(target.info).toBe(orig.info);
  });
});

describe("formatConsoleArgs", () => {
  it("handles errors, circular objects and truncation", () => {
    const circ: Record<string, unknown> = { name: "c" };
    circ.self = circ;
    expect(formatConsoleArgs([circ])).toBe('{"name":"c","self":"[Circular]"}');
    expect(formatConsoleArgs([new Error("bad")])).toContain("bad");
    expect(formatConsoleArgs([undefined, 1n])).toBe("undefined 1");
    expect(formatConsoleArgs(["x".repeat(9000)]).length).toBe(8001);
  });
});
