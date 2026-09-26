<script lang="ts">
  import { onDestroy } from "svelte";
  import { open } from "@tauri-apps/plugin-shell";
  import { Loader2, Copy, Check, LogIn, X, User, Monitor, Globe, Shield, ExternalLink, Link2, ChevronRight } from "@lucide/svelte";
  import { api } from "../lib/api";
  import { authState, skinPath, loginTypeLabel, type SkinSource, type YggdrasilPreset } from "../lib/store";
  import { toasts } from "../lib/toast";

  let { onclose }: { onclose?: () => void } = $props();

  let mode = $state<"select" | "microsoft-webview" | "microsoft-code" | "microsoft-polling" | "microsoft-url" | "offline-form" | "yggdrasil-form">("select");
  let deviceCode = $state<{
    userCode: string;
    verificationUri: string;
    loginUrl: string;
    interval?: number;
  } | null>(null);
  let polling = $state(false);
  let errorMsg = $state("");
  let copied = $state(false);
  let copiedLink = $state(false);
  let pollTimer: ReturnType<typeof setTimeout> | null = null;
  let authUrlPaste = $state("");
  let msAuthorizeUrl = $state("");

  // Offline form
  let offlineUsername = $state("");
  let skinSource = $state<SkinSource>("mojang");
  let loggingIn = $state(false);

  // Yggdrasil form
  let yggPresets = $state<YggdrasilPreset[]>([]);
  let yggPresetId = $state("elyby");
  let yggAuthority = $state("");
  let yggUsername = $state("");
  let yggPassword = $state("");

  const existingAccounts = $derived($authState.accounts ?? []);

  async function switchExisting(uuid: string) {
    loggingIn = true;
    errorMsg = "";
    try {
      const state = await api.mcAuth.switchAccount(uuid);
      authState.set(state);
      if (state.profile) {
        try {
          skinPath.set(await api.mcAuth.getSkinPath(state.profile.uuid));
        } catch {
          skinPath.set(null);
        }
      }
      toasts.success(`Switched to ${state.profile?.name ?? "account"}`);
      setTimeout(() => onclose?.(), 400);
    } catch (e) {
      errorMsg = String(e);
    } finally {
      loggingIn = false;
    }
  }

  async function startMicrosoftLogin() {
    clearPollTimer();
    polling = false;
    mode = "microsoft-webview";
    errorMsg = "";
    loggingIn = true;
    try {
      const result = await api.mcAuth.startMicrosoftWebviewAuth();
      const state = await api.mcAuth.getAuthStatus();
      authState.set(state);
      if (result.profile.uuid) {
        try {
          skinPath.set(await api.mcAuth.getSkinPath(result.profile.uuid));
        } catch {
          skinPath.set(null);
        }
      }
      toasts.success(`Logged in as ${result.profile.name}`);
      mode = "select";
      setTimeout(() => onclose?.(), 600);
    } catch (e) {
      const msg = String(e);
      if (!msg.toLowerCase().includes("cancelled")) {
        errorMsg = msg;
      } else {
        errorMsg = "";
      }
      mode = "select";
    } finally {
      loggingIn = false;
    }
  }

  async function startDeviceCodeLogin() {
    if (loggingIn || polling || pollInFlight) return;
    mode = "microsoft-code";
    errorMsg = "";
    loggingIn = true;
    try {
      const info = await api.mcAuth.startDeviceCode();
      deviceCode = {
        userCode: info.userCode,
        verificationUri: info.verificationUri,
        loginUrl: info.loginUrl || `${info.verificationUri}?otc=${encodeURIComponent(info.userCode)}`,
        interval: info.interval,
      };
      mode = "microsoft-polling";
      startPolling();
      try {
        await open(deviceCode.loginUrl);
      } catch {
        try {
          await open(deviceCode.verificationUri);
        } catch {
          /* ignore */
        }
      }
    } catch (e) {
      const msg = String(e);
      // The Azure device-code public client (89484d4e-…) is blocked in some
      // tenants/regions and fails with AADSTS700016 (unauthorized_client).
      // Fall back to the working Live authorization-code (URL/paste) flow.
      if (
        msg.includes("unauthorized_client") ||
        msg.includes("AADSTS700016") ||
        msg.includes("700016")
      ) {
        try {
          await openMicrosoftUrlLogin();
          return;
        } catch {
          /* fall through to the error below */
        }
      }
      errorMsg = msg;
      mode = "select";
    } finally {
      loggingIn = false;
    }
  }

  async function reopenLoginLink() {
    const url = deviceCode?.loginUrl || deviceCode?.verificationUri;
    if (!url) return;
    try {
      await open(url);
    } catch {
      /* ignore */
    }
  }

  async function openMicrosoftUrlLogin() {
    clearPollTimer();
    polling = false;
    errorMsg = "";
    authUrlPaste = "";
    try {
      msAuthorizeUrl = await api.mcAuth.getMicrosoftLoginUrl();
      mode = "microsoft-url";
      try { await open(msAuthorizeUrl); } catch {}
    } catch (e) {
      errorMsg = String(e);
      mode = "select";
    }
  }

  async function reopenMicrosoftAuthorize() {
    if (!msAuthorizeUrl) {
      try {
        msAuthorizeUrl = await api.mcAuth.getMicrosoftLoginUrl();
      } catch (e) {
        errorMsg = String(e);
        return;
      }
    }
    try { await open(msAuthorizeUrl); } catch {}
  }

  async function submitMicrosoftUrlLogin() {
    if (!authUrlPaste.trim()) {
      errorMsg = "Paste the redirect URL from the browser";
      return;
    }
    loggingIn = true;
    errorMsg = "";
    try {
      const result = await api.mcAuth.loginWithAuthUrl(authUrlPaste.trim());
      const state = await api.mcAuth.getAuthStatus();
      authState.set(state);
      if (result.profile.uuid) {
        try {
          skinPath.set(await api.mcAuth.getSkinPath(result.profile.uuid));
        } catch {
          skinPath.set(null);
        }
      }
      toasts.success(`Logged in as ${result.profile.name}`);
      mode = "select";
      authUrlPaste = "";
      setTimeout(() => onclose?.(), 600);
    } catch (e) {
      errorMsg = String(e);
    } finally {
      loggingIn = false;
    }
  }

  let pollInFlight = $state(false);
  let pollIntervalMs = $state(3500);

  function scheduleNextPoll() {
    if (pollTimer) clearTimeout(pollTimer);
    pollTimer = setTimeout(async () => {
      // Stop entirely once polling was turned off (modal closed / destroyed) —
      // never re-schedule in that case, or timers leak forever after close.
      if (!polling) {
        pollTimer = null;
        return;
      }
      if (pollInFlight) {
        scheduleNextPoll();
        return;
      }
      pollInFlight = true;
      try {
        const result = await api.mcAuth.pollDeviceCode();
        polling = false;
        if (pollTimer) clearTimeout(pollTimer);
        pollTimer = null;
        const state = await api.mcAuth.getAuthStatus();
        authState.set(state);
        if (result.profile.uuid) {
          try {
            skinPath.set(await api.mcAuth.getSkinPath(result.profile.uuid));
          } catch {
            skinPath.set(null);
          }
        }
        toasts.success(`Logged in as ${result.profile.name}`);
        mode = "select";
        setTimeout(() => onclose?.(), 800);
      } catch (e) {
        const msg = String(e);
        if (msg.includes("slow_down")) {
          pollIntervalMs = Math.min(pollIntervalMs + 2000, 15000);
          scheduleNextPoll();
          return;
        }
        if (msg.includes("authorization_pending")) {
          scheduleNextPoll();
          return;
        }
        if (
          msg.includes("timed out") ||
          msg.includes("expired") ||
          msg.includes("declined") ||
          msg.includes("Invalid device")
        ) {
          polling = false;
          if (pollTimer) clearTimeout(pollTimer);
          pollTimer = null;
          errorMsg = msg;
          mode = "select";
        } else {
          scheduleNextPoll();
        }
      } finally {
        pollInFlight = false;
      }
    }, pollIntervalMs);
  }

  function startPolling() {
    clearPollTimer();
    polling = true;
    pollInFlight = false;
    const sec = deviceCode?.interval && deviceCode.interval > 0 ? deviceCode.interval : 5;
    pollIntervalMs = Math.max(sec, 1) * 1000;
    scheduleNextPoll();
  }

  async function handleOfflineLogin() {
    if (!offlineUsername.trim()) {
      errorMsg = "Enter a username";
      return;
    }
    loggingIn = true;
    errorMsg = "";
    try {
      const result = await api.mcAuth.offlineLogin(offlineUsername.trim(), skinSource);
      const state = await api.mcAuth.getAuthStatus();
      authState.set(state);
      if (result.profile.skinUrl || state.profile?.skinUrl) {
        try {
          const path = await api.mcAuth.getSkinPath(result.profile.uuid);
          skinPath.set(path);
        } catch {}
      }
      toasts.success(`Playing as ${result.profile.name}`);
      mode = "select";
      setTimeout(() => onclose?.(), 600);
    } catch (e) {
      errorMsg = String(e);
    } finally {
      loggingIn = false;
    }
  }

  async function openYggdrasilForm() {
    errorMsg = "";
    try {
      yggPresets = await api.mcAuth.listYggdrasilPresets();
    } catch {
      yggPresets = [
        { id: "elyby", label: "Ely.by", authority: "https://authserver.ely.by/api/authlib-injector" },
        { id: "littleskin", label: "LittleSkin", authority: "https://littleskin.cn/api/yggdrasil" },
        { id: "custom", label: "Custom", authority: "" },
      ];
    }
    const preset = yggPresets.find((p) => p.id === yggPresetId) ?? yggPresets[0];
    yggPresetId = preset?.id ?? "elyby";
    yggAuthority = preset?.authority ?? "";
    mode = "yggdrasil-form";
  }

  function selectYggPreset(id: string) {
    yggPresetId = id;
    const preset = yggPresets.find((p) => p.id === id);
    if (preset && id !== "custom") {
      yggAuthority = preset.authority;
    }
  }

  async function handleYggdrasilLogin() {
    if (!yggUsername.trim() || !yggPassword) {
      errorMsg = "Enter username and password";
      return;
    }
    if (!yggAuthority.trim()) {
      errorMsg = "Enter authority URL";
      return;
    }
    loggingIn = true;
    errorMsg = "";
    try {
      const result = await api.mcAuth.yggdrasilLogin(
        yggUsername.trim(),
        yggPassword,
        yggAuthority.trim()
      );
      const state = await api.mcAuth.getAuthStatus();
      authState.set(state);
      if (result.profile.skinUrl) {
        try {
          skinPath.set(await api.mcAuth.getSkinPath(result.profile.uuid));
        } catch {}
      }
      toasts.success(`Logged in as ${result.profile.name}`);
      mode = "select";
      yggPassword = "";
      setTimeout(() => onclose?.(), 600);
    } catch (e) {
      errorMsg = String(e);
    } finally {
      loggingIn = false;
    }
  }

  async function copyCode() {
    if (!deviceCode) return;
    try {
      await navigator.clipboard.writeText(deviceCode.userCode);
      copied = true;
      setTimeout(() => (copied = false), 2000);
    } catch {
      errorMsg = "Copy failed — select and copy the code manually.";
    }
  }

  async function copyLoginLink() {
    if (!deviceCode?.loginUrl) return;
    try {
      await navigator.clipboard.writeText(deviceCode.loginUrl);
      copiedLink = true;
      setTimeout(() => (copiedLink = false), 2000);
    } catch {
      /* ignore */
    }
  }

  function clearPollTimer() {
    if (pollTimer) {
      clearTimeout(pollTimer);
      pollTimer = null;
    }
  }

  function close() {
    clearPollTimer();
    polling = false;
    onclose?.();
  }

  onDestroy(() => {
    clearPollTimer();
  });
</script>

<div
  class="overlay"
  role="presentation"
  onclick={(e) => e.target === e.currentTarget && close()}
>
  <div class="modal">
    <div class="modal-header">
      <div class="modal-title">
        <LogIn size={18} />
        <h3>
          {#if mode === "offline-form"}Offline Login
          {:else if mode === "yggdrasil-form"}Yggdrasil Login
          {:else if mode === "microsoft-webview"}Microsoft Login
          {:else if mode === "microsoft-code" || mode === "microsoft-polling"}Microsoft — link
          {:else if mode === "microsoft-url"}Microsoft Login (URL)
          {:else}Sign In{/if}
        </h3>
      </div>
      <button class="close-btn" onclick={close} aria-label="Close">
        <X size={18} />
      </button>
    </div>

    <div class="modal-body">
      {#if mode === "select"}
        {#if existingAccounts.length > 0}
          <div class="existing-accounts">
            <div class="existing-label">Saved accounts</div>
            {#each existingAccounts as account (account.uuid)}
              <button
                class="account-row"
                class:active={account.uuid === $authState.activeAccountUuid}
                disabled={loggingIn}
                onclick={() => switchExisting(account.uuid)}
              >
                <div class="account-ico" class:ms={account.loginType === "microsoft"} class:ygg={account.loginType === "yggdrasil"}>
                  {#if account.loginType === "microsoft"}
                    <Globe size={16} />
                  {:else if account.loginType === "yggdrasil"}
                    <Shield size={16} />
                  {:else}
                    <User size={16} />
                  {/if}
                </div>
                <div class="account-text">
                  <span class="mc-nick">{account.name}</span>
                  <span
                    class="account-type"
                    class:mojang={account.loginType === "microsoft"}
                    class:offline={account.loginType === "offline"}
                    class:ygg={account.loginType === "yggdrasil"}
                  >
                    {loginTypeLabel(account.loginType, account.authority)}
                  </span>
                </div>
                {#if account.uuid === $authState.activeAccountUuid}
                  <Check size={14} class="check" />
                {/if}
              </button>
            {/each}
          </div>
          <div class="divider"><span>or add new</span></div>
        {/if}

        <div class="login-options">
          <button class="login-option" onclick={startDeviceCodeLogin} disabled={loggingIn}>
            <div class="option-icon ms">
              <Link2 size={20} />
            </div>
            <div class="option-info">
              <span class="option-title">Microsoft — open link</span>
              <span class="option-desc">Opens microsoft.com with your code — no Azure app needed</span>
            </div>
            <ExternalLink size={16} class="option-arrow" />
          </button>

          <button class="login-option" onclick={startMicrosoftLogin} disabled={loggingIn}>
            <div class="option-icon ms">
              <Globe size={20} />
            </div>
            <div class="option-info">
              <span class="option-title">Microsoft — popup</span>
              <span class="option-desc">Sign in in a popup window</span>
            </div>
            <ChevronRight size={16} class="option-arrow" />
          </button>

          <button class="login-option" onclick={() => (mode = "offline-form")}>
            <div class="option-icon offline">
              <User size={20} />
            </div>
            <div class="option-info">
              <span class="option-title">Offline Mode</span>
              <span class="option-desc">Play with custom username</span>
            </div>
            <ChevronRight size={16} class="option-arrow" />
          </button>

          <button class="login-option" onclick={openYggdrasilForm}>
            <div class="option-icon ygg">
              <Shield size={20} />
            </div>
            <div class="option-info">
              <span class="option-title">Ely.by / LittleSkin / Custom</span>
              <span class="option-desc">authlib-injector Yggdrasil providers</span>
            </div>
            <ChevronRight size={16} class="option-arrow" />
          </button>
        </div>

        <div class="ms-other">
          <span class="ms-other-label">Other Microsoft methods</span>
          <button class="link-btn" type="button" onclick={openMicrosoftUrlLogin}>Paste redirect URL</button>
        </div>

        {#if errorMsg}
          <div class="error-msg">{errorMsg}</div>
        {/if}

        <p class="hint">Offline mode fetches skins from Ely.by, TLauncher, or Mojang. Capes can be shown from OptiFine / TLauncher / Mojang.</p>

      {:else if mode === "microsoft-webview"}
        <div class="code-content">
          <div class="polling-indicator">
            <Loader2 size={16} class="spin" />
            <span>Complete sign-in in the popup window…</span>
          </div>
          <p class="instruction">
            A Microsoft login window opened. After you sign in it will close automatically.
          </p>
          {#if errorMsg}
            <div class="error-msg">{errorMsg}</div>
          {/if}
          <button class="link-btn" type="button" onclick={() => { mode = "select"; errorMsg = ""; }}>
            Back
          </button>
        </div>

      {:else if mode === "microsoft-code"}
        <div class="code-content">
          <div class="polling-indicator">
            <Loader2 size={16} class="spin" />
            <span>Contacting Microsoft…</span>
          </div>
          <p class="instruction">
            Requesting a device login code — a browser tab will open shortly.
          </p>
          <button class="link-btn" type="button" onclick={() => { mode = "select"; errorMsg = ""; }}>
            Back
          </button>
        </div>

      {:else if mode === "microsoft-polling" && deviceCode}
        <div class="code-content">
          <p class="instruction">
            A browser tab should open with your login code already filled in.
            Finish signing in there — this window waits automatically.
          </p>
          <button class="primary-btn" type="button" onclick={reopenLoginLink}>
            <ExternalLink size={16} /> Open login link
          </button>
          <div class="link-row">
            <code class="login-link" title={deviceCode.loginUrl}>{deviceCode.loginUrl}</code>
            <button class="copy-btn" onclick={copyLoginLink} title="Copy login link">
              {#if copiedLink}<Check size={16} />{:else}<Copy size={16} />{/if}
            </button>
          </div>
          <div class="manual-code">
            <span class="code-label">Or enter code manually at microsoft.com/link</span>
            <div class="code-display">
              <span class="code">{deviceCode.userCode}</span>
              <button class="copy-btn" onclick={copyCode} title="Copy code">
                {#if copied}<Check size={16} />{:else}<Copy size={16} />{/if}
              </button>
            </div>
          </div>
          <p class="instruction muted">
            Manual page: <a
              href={deviceCode.verificationUri}
              onclick={(e) => {
                e.preventDefault();
                const uri = deviceCode?.verificationUri;
                if (uri) void open(uri);
              }}>{deviceCode.verificationUri}</a
            >
          </p>
          <div class="polling-indicator">
            <Loader2 size={16} class="spin" />
            <span>Waiting for authentication…</span>
          </div>
          <button class="link-btn" type="button" onclick={openMicrosoftUrlLogin}>
            Prefer paste URL instead?
          </button>
          <button class="link-btn" type="button" onclick={startMicrosoftLogin}>
            Use popup window instead?
          </button>
          {#if errorMsg}
            <div class="error-msg">{errorMsg}</div>
          {/if}
        </div>

      {:else if mode === "microsoft-url"}
        <form class="offline-form" onsubmit={(e) => { e.preventDefault(); submitMicrosoftUrlLogin(); }}>
          <p class="instruction url-steps">
            1. Sign in in the browser window<br />
            2. When you land on a blank / “oauth20_desktop” page, copy the full address from the address bar<br />
            3. Paste it below
          </p>
          <button class="secondary-btn" type="button" onclick={reopenMicrosoftAuthorize} disabled={loggingIn}>
            <Globe size={14} /> Open Microsoft login again
          </button>
          <label class="field">
            <span>Redirect URL</span>
            <textarea
              bind:value={authUrlPaste}
              rows={3}
              placeholder="https://login.live.com/oauth20_desktop.srf?code=..."
              disabled={loggingIn}
            ></textarea>
          </label>

          {#if errorMsg}
            <div class="error-msg">{errorMsg}</div>
          {/if}

          <button class="primary-btn" type="submit" disabled={loggingIn || !authUrlPaste.trim()}>
            {#if loggingIn}
              <Loader2 size={16} class="spin" /> Signing in...
            {:else}
              <LogIn size={16} /> Complete login
            {/if}
          </button>
          <button class="link-btn" type="button" disabled={loggingIn} onclick={() => { mode = "select"; errorMsg = ""; }}>
            Back
          </button>
        </form>

      {:else if mode === "offline-form"}
        <form class="offline-form" onsubmit={(e) => { e.preventDefault(); handleOfflineLogin(); }}>
          <label class="field">
            <span>Username</span>
            <input
              bind:value={offlineUsername}
              placeholder="Enter username"
              maxlength={16}
              disabled={loggingIn}
            />
          </label>

          <label class="field">
            <span>Skin Source</span>
            <div class="skin-source-grid">
              <button
                type="button"
                class="source-option"
                class:active={skinSource === "mojang"}
                onclick={() => (skinSource = "mojang")}
              >
                <Monitor size={14} />
                Mojang
              </button>
              <button
                type="button"
                class="source-option"
                class:active={skinSource === "elyby"}
                onclick={() => (skinSource = "elyby")}
              >
                <Globe size={14} />
                Ely.by
              </button>
              <button
                type="button"
                class="source-option"
                class:active={skinSource === "tlauncher"}
                onclick={() => (skinSource = "tlauncher")}
              >
                <Globe size={14} />
                TLauncher
              </button>
              <button
                type="button"
                class="source-option"
                class:active={skinSource === "offline"}
                onclick={() => (skinSource = "offline")}
              >
                <User size={14} />
                None
              </button>
            </div>
          </label>

          {#if errorMsg}
            <div class="error-msg">{errorMsg}</div>
          {/if}

          <button class="primary-btn" type="submit" disabled={loggingIn || !offlineUsername.trim()}>
            {#if loggingIn}
              <Loader2 size={16} class="spin" /> Loading...
            {:else}
              <LogIn size={16} /> Play
            {/if}
          </button>
        </form>

      {:else if mode === "yggdrasil-form"}
        <form class="offline-form" onsubmit={(e) => { e.preventDefault(); handleYggdrasilLogin(); }}>
          <div class="skin-source-grid ygg-presets">
            {#each yggPresets as preset (preset.id)}
              <button
                type="button"
                class="source-option"
                class:active={yggPresetId === preset.id}
                onclick={() => selectYggPreset(preset.id)}
                disabled={loggingIn}
              >
                {preset.label}
              </button>
            {/each}
          </div>

          <label class="field">
            <span>Authority URL</span>
            <input
              bind:value={yggAuthority}
              placeholder="https://…/api/yggdrasil"
              disabled={loggingIn || yggPresetId !== "custom"}
            />
          </label>

          <label class="field">
            <span>Email / Username</span>
            <input bind:value={yggUsername} placeholder="account@example.com" disabled={loggingIn} />
          </label>

          <label class="field">
            <span>Password</span>
            <input type="password" bind:value={yggPassword} placeholder="••••••••" disabled={loggingIn} />
          </label>

          {#if errorMsg}
            <div class="error-msg">{errorMsg}</div>
          {/if}

          <button class="primary-btn" type="submit" disabled={loggingIn || !yggUsername.trim() || !yggPassword}>
            {#if loggingIn}
              <Loader2 size={16} class="spin" /> Signing in...
            {:else}
              <LogIn size={16} /> Sign in
            {/if}
          </button>
        </form>
      {/if}
    </div>
  </div>
</div>

<style>
  .overlay {
    position: fixed; inset: 0;
    background: rgba(0, 0, 0, 0.6); backdrop-filter: blur(4px);
    display: flex; align-items: center; justify-content: center;
    z-index: 200;
  }

  .modal {
    background: var(--bg-elevated); border: 1px solid var(--border-color);
    border-radius: var(--border-radius-xl); width: 460px; max-width: 90vw;
    box-shadow: 0 24px 64px rgba(0, 0, 0, 0.5); overflow: hidden;
    display: flex; flex-direction: column;
    max-height: min(84vh, 720px);
  }

  .modal-header {
    display: flex; align-items: center; justify-content: space-between;
    padding: 18px 22px; border-bottom: 1px solid var(--border-color);
  }

  .modal-title { display: flex; align-items: center; gap: 10px; color: var(--text-primary); }
  .modal-title h3 { font-size: 16px; font-weight: 700; }

  .close-btn {
    width: 32px; height: 32px; padding: 0; display: flex; align-items: center; justify-content: center;
    border-radius: var(--border-radius-sm); background: transparent; color: var(--text-muted); border: none;
  }
  .close-btn:hover { background: var(--bg-hover); color: var(--text-primary); }

  .modal-body { padding: 22px; overflow-y: auto; }

  /* ─── Login options ──────────────────────── */
  .existing-accounts { display: flex; flex-direction: column; gap: 8px; margin-bottom: 4px; }
  .existing-label { font-size: 12px; font-weight: 700; color: var(--text-muted); text-transform: uppercase; letter-spacing: 0.04em; }
  .account-row {
    display: flex; align-items: center; gap: 12px; width: 100%;
    padding: 10px 12px; border-radius: 10px; text-align: left;
    background: var(--bg-primary); border: 1px solid var(--border-color);
    color: var(--text-primary); cursor: pointer;
  }
  .account-row:hover { border-color: var(--accent-primary); }
  .account-row.active { border-color: var(--accent-primary); background: color-mix(in srgb, var(--accent-primary) 5%, transparent); }
  .account-ico {
    width: 32px; height: 32px; border-radius: var(--border-radius-sm);
    display: flex; align-items: center; justify-content: center;
    background: var(--bg-elevated); color: var(--text-muted);
  }
  .account-ico.ms { background: linear-gradient(135deg, #0078d4, #00a4ef); color: #fff; }
  .account-ico.ygg {
    background: var(--badge-ygg-bg, rgba(168, 85, 247, 0.18));
    color: var(--badge-ygg-fg, #e9d5ff);
  }
  .account-text { flex: 1; display: flex; flex-direction: column; gap: 8px; min-width: 0; }
  .mc-nick {
    font-family: var(--font-minecraft);
    font-size: 12px;
    letter-spacing: 0.4px;
    color: var(--mc-nick-color, var(--text-primary));
    text-shadow: var(--mc-nick-shadow-soft, 1px 1px 0 #3f3f3f);
  }
  .account-type { font-size: 12px; font-weight: 800; text-transform: uppercase; }
  .account-type.mojang { color: var(--badge-ms-fg, #93c5fd); }
  .account-type.offline { color: var(--badge-offline-fg, #fde68a); }
  .account-type.ygg { color: var(--badge-ygg-fg, #e9d5ff); }
  :global(.check) { color: var(--accent-primary); }
  .divider {
    display: flex; align-items: center; gap: 10px; margin: 14px 0;
    color: var(--text-muted); font-size: 12px;
  }
  .divider::before, .divider::after {
    content: ""; flex: 1; height: 1px; background: var(--border-color);
  }

  .login-options { display: flex; flex-direction: column; gap: 10px; }
  .ms-other {
    display: flex;
    flex-wrap: wrap;
    align-items: center;
    gap: 8px 12px;
    margin-top: 10px;
    justify-content: center;
  }
  .ms-other-label {
    width: 100%;
    text-align: center;
    font-size: 12px;
    color: var(--text-muted);
  }

  .login-option {
    display: flex; align-items: center; gap: 14px; padding: 14px 16px;
    background: var(--bg-primary); border: 1px solid var(--border-color);
    border-radius: var(--border-radius-lg); cursor: pointer; text-align: left;
    transition: all 0.15s ease; width: 100%;
  }
  .login-option:hover { border-color: var(--accent-primary); background: color-mix(in srgb, var(--accent-primary) 4%, transparent); }

  .option-icon {
    width: 40px; height: 40px; border-radius: 10px;
    display: flex; align-items: center; justify-content: center; flex-shrink: 0;
  }
  .option-icon.ms { background: linear-gradient(135deg, #0078d4, #00a4ef); color: #fff; }
  .option-icon.offline { background: var(--bg-elevated); color: var(--text-muted); border: 1px solid var(--border-color); }
  .option-icon.ygg {
    background: var(--badge-ygg-bg, rgba(168, 85, 247, 0.2));
    color: var(--badge-ygg-fg, #e9d5ff);
  }

  .option-info { flex: 1; display: flex; flex-direction: column; gap: 8px; }
  .option-title { font-weight: 700; font-size: 14px; color: var(--text-primary); }
  .option-desc { font-size: 12px; color: var(--text-muted); }

  :global(.option-arrow) { color: var(--text-muted); }

  .hint { font-size: 12px; color: var(--text-muted); text-align: center; margin-top: 12px; }

  /* ─── Code display ───────────────────────── */
  .code-content { display: flex; flex-direction: column; align-items: center; gap: 16px; }

  .code-display {
    display: flex; align-items: center; gap: 12px;
    background: var(--bg-primary); border: 2px solid var(--accent-primary);
    border-radius: var(--border-radius-md); padding: 14px 20px;
  }
  .code {
    font-family: var(--font-mono, ui-monospace, monospace); font-size: 28px; font-weight: 900;
    letter-spacing: 4px; color: var(--accent-primary);
  }
  .copy-btn {
    width: 36px; height: 36px; padding: 0; display: flex; align-items: center; justify-content: center;
    border-radius: var(--border-radius-sm); background: var(--bg-hover); color: var(--text-secondary); border: none;
  }
  .copy-btn:hover { background: var(--accent-primary); color: var(--on-accent, #000); }

  .instruction { color: var(--text-secondary); font-size: 13px; text-align: center; line-height: 1.6; }
  .instruction a { color: var(--accent-primary); text-decoration: none; font-weight: 600; }
  .instruction a:hover { text-decoration: underline; }
  .instruction.url-steps { text-align: left; margin: 0; }
  .instruction.muted { font-size: 12px; color: var(--text-muted); }
  .link-row {
    display: flex;
    align-items: center;
    gap: 8px;
    width: 100%;
    max-width: 100%;
  }
  .login-link {
    flex: 1;
    min-width: 0;
    padding: 8px 10px;
    border-radius: var(--border-radius-sm);
    border: 1px solid var(--border-color);
    background: var(--bg-tertiary);
    color: var(--text-muted);
    font-size: 12px;
    overflow: hidden;
    text-overflow: ellipsis;
    white-space: nowrap;
  }
  .code-label {
    font-size: 12px;
    font-weight: 600;
    color: var(--text-muted);
    text-align: center;
  }
  .manual-code {
    display: flex;
    flex-direction: column;
    align-items: center;
    gap: 8px;
    width: 100%;
  }
  .code-content .primary-btn {
    width: auto;
    min-width: 200px;
    display: inline-flex;
    align-items: center;
    justify-content: center;
    gap: 8px;
  }

  .polling-indicator { display: flex; align-items: center; gap: 8px; color: var(--text-muted); font-size: 13px; }

  .link-btn {
    background: transparent; border: none; color: var(--accent-primary);
    font-size: 12px; font-weight: 600; cursor: pointer; padding: 4px 0;
  }
  .link-btn:hover { text-decoration: underline; }
  .link-btn:disabled { opacity: 0.5; cursor: not-allowed; }

  .secondary-btn {
    display: inline-flex; align-items: center; justify-content: center; gap: 8px;
    width: 100%; padding: 10px 14px; border-radius: var(--border-radius-md);
    background: var(--bg-primary); border: 1px solid var(--border-color);
    color: var(--text-secondary); font-size: 12px; font-weight: 600; cursor: pointer;
  }
  .secondary-btn:hover { border-color: var(--accent-primary); color: var(--accent-primary); }

  /* ─── Offline form ───────────────────────── */
  .offline-form { display: flex; flex-direction: column; gap: 16px; }

  .field { display: flex; flex-direction: column; gap: 8px; }
  .field span { font-size: 12px; font-weight: 600; color: var(--text-secondary); }

  .field input,
  .field textarea {
    width: 100%; padding: 10px 14px; background: var(--bg-primary);
    border: 1px solid var(--border-color); border-radius: var(--border-radius-md);
    color: var(--text-primary); font-size: 14px; outline: none;
    font-family: inherit; resize: vertical; box-sizing: border-box;
  }
  .field input:focus,
  .field textarea:focus { border-color: var(--accent-primary); }
  .field textarea { min-height: 72px; line-height: 1.4; font-size: 12px; }

  .skin-source-grid { display: grid; grid-template-columns: repeat(4, 1fr); gap: 8px; }
  .ygg-presets { grid-template-columns: repeat(3, 1fr); }

  .source-option {
    display: flex; align-items: center; justify-content: center; gap: 8px;
    padding: 8px 6px; border-radius: var(--border-radius-sm); background: var(--bg-primary);
    border: 1px solid var(--border-color); color: var(--text-secondary);
    font-size: 12px; font-weight: 600; cursor: pointer; transition: all 0.15s;
  }
  .source-option:hover { border-color: var(--text-muted); color: var(--text-primary); }
  .source-option.active {
    border-color: var(--accent-primary); color: var(--accent-primary);
    background: color-mix(in srgb, var(--accent-primary) 6%, transparent);
  }

  .error-msg {
    color: #f87171; font-size: 12px; background: rgba(248, 113, 113, 0.1);
    border: 1px solid rgba(248, 113, 113, 0.2); border-radius: var(--border-radius-sm);
    padding: 8px 12px; text-align: center;
  }

  .primary-btn { width: 100%; padding: 12px 24px; font-size: 15px; }
</style>
