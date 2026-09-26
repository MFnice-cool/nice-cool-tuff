<script lang="ts">
  import {
    X,
    Plus,
    User,
    Globe,
    Monitor,
    Trash2,
    ArrowLeftRight,
    Shield,
  } from "@lucide/svelte";
  import { api } from "../lib/api";
  import { focusInit } from "../lib/focusInit";
  import {
    authState,
    skinPath,
    loginTypeLabel,
    type AccountEntry,
    type SkinSource,
    type YggdrasilPreset,
  } from "../lib/store";
  import { toasts } from "../lib/toast";

  let { onclose }: { onclose?: () => void } = $props();

  let mode = $state<"list" | "add-select" | "add-offline" | "add-yggdrasil">("list");
  let accounts = $state<AccountEntry[]>([]);
  let activeUuid = $state<string | null>(null);
  let offlineUsername = $state("");
  let offlineSkinSource = $state<SkinSource>("mojang");
  let yggPresets = $state<YggdrasilPreset[]>([]);
  let yggPresetId = $state("elyby");
  let yggAuthority = $state("");
  let yggUsername = $state("");
  let yggPassword = $state("");
  let busy = $state(false);
  let errorMsg = $state("");

  $effect(() => {
    accounts = $authState.accounts;
    activeUuid = $authState.activeAccountUuid;
  });

  async function loadAccounts() {
    try {
      const state = await api.mcAuth.getAuthStatus();
      authState.set(state);
      accounts = state.accounts;
      activeUuid = state.activeAccountUuid;
    } catch {}
  }

  async function switchAccount(uuid: string) {
    if (uuid === activeUuid) return;
    busy = true;
    try {
      const state = await api.mcAuth.switchAccount(uuid);
      authState.set(state);
      if (state.profile) {
        try {
          const path = await api.mcAuth.getSkinPath(state.profile.uuid);
          skinPath.set(path);
        } catch {
          skinPath.set(null);
        }
      } else {
        skinPath.set(null);
      }
      toasts.success(`Switched to ${state.profile?.name ?? "account"}`);
    } catch (e) {
      toasts.error(String(e));
    } finally {
      busy = false;
    }
  }

  async function removeAccount(uuid: string) {
    busy = true;
    try {
      const state = await api.mcAuth.removeAccount(uuid);
      authState.set(state);
      accounts = state.accounts;
      activeUuid = state.activeAccountUuid;
      if (state.profile?.uuid) {
        try {
          skinPath.set(await api.mcAuth.getSkinPath(state.profile.uuid));
        } catch {
          skinPath.set(null);
        }
      } else {
        skinPath.set(null);
      }
      toasts.info("Account removed");
    } catch (e) {
      toasts.error(String(e));
    } finally {
      busy = false;
    }
  }

  async function addOffline() {
    if (!offlineUsername.trim()) {
      errorMsg = "Enter a username";
      return;
    }
    busy = true;
    errorMsg = "";
    try {
      const result = await api.mcAuth.offlineLogin(
        offlineUsername.trim(),
        offlineSkinSource
      );
      const state = await api.mcAuth.getAuthStatus();
      authState.set(state);
      await loadAccounts();
      if (result.profile.skinUrl || state.profile?.skinUrl) {
        try {
          const path = await api.mcAuth.getSkinPath(result.profile.uuid);
          skinPath.set(path);
        } catch {}
      }
      toasts.success(`Added ${result.profile.name}`);
      mode = "list";
      offlineUsername = "";
    } catch (e) {
      errorMsg = String(e);
    } finally {
      busy = false;
    }
  }

  async function startMicrosoftAdd() {
    busy = true;
    errorMsg = "";
    try {
      const result = await api.mcAuth.startMicrosoftWebviewAuth();
      await loadAccounts();
      if (result.profile.uuid) {
        try {
          skinPath.set(await api.mcAuth.getSkinPath(result.profile.uuid));
        } catch {}
      }
      toasts.success(`Added ${result.profile.name}`);
      mode = "list";
    } catch (e) {
      const msg = String(e);
      if (!msg.toLowerCase().includes("cancelled")) {
        errorMsg = msg;
      }
    } finally {
      busy = false;
    }
  }

  async function openYggdrasil() {
    errorMsg = "";
    try {
      yggPresets = await api.mcAuth.listYggdrasilPresets();
    } catch {
      yggPresets = [
        { id: "elyby", label: "Ely.by", authority: "https://authserver.ely.by/api/authlib-injector" },
        { id: "littleskin", label: "LittleSkin", authority: "https://littleskin.cn/api/yggdrasil" },
        { id: "custom", label: "Custom authlib-injector / Yggdrasil", authority: "" },
      ];
    }
    const preset = yggPresets.find((p) => p.id === yggPresetId) ?? yggPresets[0];
    yggPresetId = preset?.id ?? "elyby";
    yggAuthority = preset?.authority ?? "";
    mode = "add-yggdrasil";
  }

  function selectYggPreset(id: string) {
    yggPresetId = id;
    const preset = yggPresets.find((p) => p.id === id);
    if (preset && id !== "custom") {
      yggAuthority = preset.authority;
    }
  }

  async function addYggdrasil() {
    if (!yggUsername.trim() || !yggPassword) {
      errorMsg = "Enter username and password";
      return;
    }
    if (!yggAuthority.trim()) {
      errorMsg = "Enter authlib-injector / Yggdrasil authority URL";
      return;
    }
    busy = true;
    errorMsg = "";
    try {
      const result = await api.mcAuth.yggdrasilLogin(
        yggUsername.trim(),
        yggPassword,
        yggAuthority.trim()
      );
      await loadAccounts();
      if (result.profile.skinUrl) {
        try {
          skinPath.set(await api.mcAuth.getSkinPath(result.profile.uuid));
        } catch {}
      }
      toasts.success(`Added ${result.profile.name}`);
      mode = "list";
      yggPassword = "";
      yggUsername = "";
    } catch (e) {
      errorMsg = String(e);
    } finally {
      busy = false;
    }
  }

  function close() {
    onclose?.();
  }
</script>

<!-- svelte-ignore a11y_click_events_have_key_events -->
<!-- svelte-ignore a11y_no_static_element_interactions -->
<div class="overlay" onclick={(e) => e.target === e.currentTarget && close()}>
  <div class="modal">
    <div class="modal-header">
      {#if mode !== "list"}
        <button class="back-btn" onclick={() => (mode = "list")} aria-label="Back">
          <ArrowLeftRight size={16} />
        </button>
      {/if}
      <div class="modal-title">
        <User size={18} />
        <h3>
          {#if mode === "list"}Accounts
          {:else if mode === "add-select"}Add Account
          {:else if mode === "add-yggdrasil"}Yggdrasil Login
          {:else}Add Offline Account{/if}
        </h3>
      </div>
      <button class="close-btn" onclick={close} aria-label="Close">
        <X size={18} />
      </button>
    </div>

    <div class="modal-body">
      {#if mode === "list"}
        {#if accounts.length === 0}
          <div class="empty-accounts">
            <User size={32} />
            <p>No accounts added yet</p>
            <button class="accent-btn" onclick={() => (mode = "add-select")}>
              <Plus size={16} /> Add Account
            </button>
          </div>
        {:else}
          <div class="account-list">
            {#each accounts as account}
              <div
                class="account-item"
                class:active={account.uuid === activeUuid}
              >
                <div
                  class="account-avatar"
                  class:ms={account.loginType === "microsoft"}
                  class:off={account.loginType === "offline"}
                  class:ygg={account.loginType === "yggdrasil"}
                >
                  {#if account.loginType === "microsoft"}
                    <Globe size={16} />
                  {:else if account.loginType === "yggdrasil"}
                    <Shield size={16} />
                  {:else}
                    <User size={16} />
                  {/if}
                </div>
                <div class="account-info">
                  <span class="account-name mc-font">{account.name}</span>
                  <span class="account-meta">
                    <span
                      class="type-pill"
                      class:mojang={account.loginType === "microsoft"}
                      class:offline={account.loginType === "offline"}
                      class:ygg={account.loginType === "yggdrasil"}
                    >
                      {loginTypeLabel(account.loginType, account.authority)}
                    </span>
                    {#if account.loginType === "offline" && account.skinSource !== "offline"}
                      <span class="skin-src">{account.skinSource}</span>
                    {/if}
                    {#if account.uuid === activeUuid}
                      <span class="active-badge">Active</span>
                    {/if}
                  </span>
                </div>
                <div class="account-actions">
                  {#if account.uuid !== activeUuid}
                    <button
                      class="icon-btn small"
                      onclick={() => switchAccount(account.uuid)}
                      disabled={busy}
                      title="Switch to this account"
                    >
                      <ArrowLeftRight size={14} />
                    </button>
                  {/if}
                  <button
                    class="icon-btn small danger"
                    onclick={() => removeAccount(account.uuid)}
                    disabled={busy}
                    title="Remove account"
                  >
                    <Trash2 size={14} />
                  </button>
                </div>
              </div>
            {/each}
          </div>

          <button class="accent-btn full-width" onclick={() => (mode = "add-select")}>
            <Plus size={16} /> Add Account
          </button>
        {/if}

      {:else if mode === "add-select"}
        <div class="add-options">
          <button class="add-option" onclick={startMicrosoftAdd} disabled={busy}>
            <div class="option-icon ms"><Globe size={20} /></div>
            <div class="option-info">
              <span class="option-title">Microsoft Account</span>
              <span class="option-desc">Sign in in a popup — online play, skins, Realms</span>
            </div>
          </button>

          <button class="add-option" onclick={() => (mode = "add-offline")} disabled={busy}>
            <div class="option-icon offline"><User size={20} /></div>
            <div class="option-info">
              <span class="option-title">Offline Account</span>
              <span class="option-desc">Play with custom username</span>
            </div>
          </button>

          <button class="add-option" onclick={openYggdrasil} disabled={busy}>
            <div class="option-icon ygg"><Shield size={20} /></div>
            <div class="option-info">
              <span class="option-title">Ely.by / LittleSkin / Custom</span>
              <span class="option-desc">authlib-injector Yggdrasil login</span>
            </div>
          </button>
        </div>

      {:else if mode === "add-yggdrasil"}
        <form class="offline-form" onsubmit={(e) => { e.preventDefault(); addYggdrasil(); }}>
          <div class="skin-source-grid ygg-presets">
            {#each yggPresets as preset}
              <button
                type="button"
                class="source-option"
                class:active={yggPresetId === preset.id}
                onclick={() => selectYggPreset(preset.id)}
                disabled={busy}
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
              disabled={busy || yggPresetId !== "custom"}
            />
          </label>

          <label class="field">
            <span>Email / Username</span>
            <input bind:value={yggUsername} placeholder="account@example.com" use:focusInit disabled={busy} />
          </label>

          <label class="field">
            <span>Password</span>
            <input type="password" bind:value={yggPassword} placeholder="••••••••" disabled={busy} />
          </label>

          {#if errorMsg}
            <div class="error-msg">{errorMsg}</div>
          {/if}

          <button class="accent-btn full-width" type="submit" disabled={busy || !yggUsername.trim() || !yggPassword}>
            {busy ? "Signing in..." : "Sign in"}
          </button>
        </form>

      {:else if mode === "add-offline"}
        <form class="offline-form" onsubmit={(e) => { e.preventDefault(); addOffline(); }}>
          <label class="field">
            <span>Username</span>
            <input
              bind:value={offlineUsername}
              placeholder="Enter username"
              maxlength={16}
              use:focusInit
              disabled={busy}
            />
          </label>

          <label class="field">
            <span>Skin Source</span>
            <div class="skin-source-grid">
              <button type="button" class="source-option" class:active={offlineSkinSource === "mojang"} onclick={() => (offlineSkinSource = "mojang")}>
                <Monitor size={14} /> Mojang
              </button>
              <button type="button" class="source-option" class:active={offlineSkinSource === "elyby"} onclick={() => (offlineSkinSource = "elyby")}>
                <Globe size={14} /> Ely.by
              </button>
              <button type="button" class="source-option" class:active={offlineSkinSource === "tlauncher"} onclick={() => (offlineSkinSource = "tlauncher")}>
                <Globe size={14} /> TLauncher
              </button>
              <button type="button" class="source-option" class:active={offlineSkinSource === "offline"} onclick={() => (offlineSkinSource = "offline")}>
                <User size={14} /> None
              </button>
            </div>
          </label>

          {#if errorMsg}
            <div class="error-msg">{errorMsg}</div>
          {/if}

          <button class="accent-btn full-width" type="submit" disabled={busy || !offlineUsername.trim()}>
            {busy ? "Adding..." : "Add Account"}
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
    border-radius: var(--border-radius-xl); width: 440px; max-width: 90vw;
    box-shadow: 0 24px 64px rgba(0, 0, 0, 0.5); overflow: hidden;
    max-height: 80vh; display: flex; flex-direction: column;
  }

  .modal-header {
    display: flex; align-items: center; justify-content: space-between;
    padding: 18px 22px; border-bottom: 1px solid var(--border-color);
  }

  .modal-title { display: flex; align-items: center; gap: 10px; color: var(--text-primary); flex: 1; }
  .modal-title h3 { font-size: 16px; font-weight: 700; }

  .back-btn, .close-btn {
    width: 32px; height: 32px; padding: 0; display: flex; align-items: center; justify-content: center;
    border-radius: var(--border-radius-sm); background: transparent; color: var(--text-muted); border: none;
  }
  .back-btn:hover, .close-btn:hover { background: var(--bg-hover); color: var(--text-primary); }

  .modal-body { padding: 22px; overflow-y: auto; }

  /* ─── Account List ──────────────────────── */
  .empty-accounts {
    display: flex; flex-direction: column; align-items: center; gap: 12px;
    padding: 32px; text-align: center; color: var(--text-muted);
  }
  .empty-accounts p { font-size: 13px; }

  .account-list { display: flex; flex-direction: column; gap: 8px; margin-bottom: 16px; }

  .account-item {
    display: flex; align-items: center; gap: 12px; padding: 10px 12px;
    background: var(--bg-primary); border: 1px solid var(--border-color);
    border-radius: var(--border-radius-md); transition: all 0.15s;
  }
  .account-item.active { border-color: var(--accent-primary); background: color-mix(in srgb, var(--accent-primary) 4%, transparent); }

  .account-avatar {
    width: 36px; height: 36px; border-radius: var(--border-radius-sm);
    display: flex; align-items: center; justify-content: center;
    background: var(--bg-elevated); color: var(--text-muted);
    flex-shrink: 0;
  }
  .account-avatar.ms { background: linear-gradient(135deg, #0078d4, #00a4ef); color: #fff; }
  .account-avatar.off {
    border: 1px solid var(--badge-offline-border, rgba(245, 158, 11, 0.35));
    color: var(--badge-offline-fg, #fde68a);
  }
  .account-avatar.ygg {
    background: var(--badge-ygg-bg, rgba(168, 85, 247, 0.18));
    color: var(--badge-ygg-fg, #e9d5ff);
  }

  .account-info { flex: 1; display: flex; flex-direction: column; gap: 8px; min-width: 0; }
  .account-name { font-weight: 700; font-size: 13px; color: var(--text-primary); }
  .account-name.mc-font {
    font-family: var(--font-minecraft);
    font-size: 12px;
    letter-spacing: 0.4px;
    color: var(--mc-nick-color, var(--text-primary));
    text-shadow: var(--mc-nick-shadow-soft, 1px 1px 0 #3f3f3f);
  }
  .account-meta { font-size: 12px; color: var(--text-muted); display: flex; align-items: center; gap: 8px; flex-wrap: wrap; }

  .type-pill {
    font-size: 12px; font-weight: 800; text-transform: uppercase;
    padding: 1px 6px; border-radius: 4px;
  }
  .type-pill.mojang {
    color: var(--badge-ms-fg, #93c5fd);
    background: var(--badge-ms-bg, rgba(59, 130, 246, 0.15));
  }
  .type-pill.offline {
    color: var(--badge-offline-fg, #fde68a);
    background: var(--badge-offline-bg, rgba(245, 158, 11, 0.12));
  }
  .type-pill.ygg {
    color: var(--badge-ygg-fg, #e9d5ff);
    background: var(--badge-ygg-bg, rgba(168, 85, 247, 0.15));
  }
  .skin-src { font-size: 12px; color: var(--text-muted); }

  .active-badge {
    display: inline-block;
    line-height: 1.4;
    white-space: nowrap;
    vertical-align: baseline;
    max-width: 100%;
    overflow: hidden;
    text-overflow: ellipsis;
    font-size: 12px; font-weight: 700; color: var(--accent-primary);
    background: color-mix(in srgb, var(--accent-primary) 12%, transparent); padding: 1px 5px; border-radius: 4px;
  }

  .account-actions { display: flex; gap: 8px; }

  .icon-btn.small {
    width: 28px; height: 28px; min-width: 28px; min-height: 28px;
    padding: 0 !important;
    display: inline-flex; align-items: center; justify-content: center;
    border-radius: 6px; background: transparent !important; border: none !important;
    color: var(--text-muted) !important; cursor: pointer;
    box-shadow: none !important; transform: none !important;
  }
  .icon-btn.small :global(svg) { width: 14px; height: 14px; stroke: currentColor; }
  .icon-btn.small:hover { background: var(--bg-hover) !important; color: var(--text-primary) !important; transform: none !important; }
  .icon-btn.small.danger:hover { background: color-mix(in srgb, var(--accent-danger) 12%, transparent) !important; color: var(--accent-danger) !important; }

  /* ─── Add Options ───────────────────────── */
  .add-options { display: flex; flex-direction: column; gap: 10px; }

  .add-option {
    display: flex; align-items: center; gap: 14px; padding: 14px 16px;
    background: var(--bg-primary); border: 1px solid var(--border-color);
    border-radius: var(--border-radius-lg); cursor: pointer; text-align: left;
    transition: all 0.15s ease; width: 100%; color: var(--text-primary);
  }
  .add-option:hover { border-color: var(--accent-primary); background: color-mix(in srgb, var(--accent-primary) 4%, transparent); }
  .add-option:disabled { opacity: 0.5; cursor: not-allowed; }

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

  .ygg-presets { grid-template-columns: repeat(3, 1fr); margin-bottom: 4px; }

  .option-info { flex: 1; display: flex; flex-direction: column; gap: 8px; }
  .option-title { font-weight: 700; font-size: 14px; }
  .option-desc { font-size: 12px; color: var(--text-muted); }

  /* ─── Offline Form ──────────────────────── */
  .offline-form { display: flex; flex-direction: column; gap: 16px; }

  .field { display: flex; flex-direction: column; gap: 8px; }
  .field span { font-size: 12px; font-weight: 600; color: var(--text-secondary); }

  .field input {
    width: 100%; padding: 10px 14px; background: var(--bg-primary);
    border: 1px solid var(--border-color); border-radius: var(--border-radius-md);
    color: var(--text-primary); font-size: 14px; outline: none;
  }
  .field input:focus { border-color: var(--accent-primary); }

  .skin-source-grid { display: grid; grid-template-columns: repeat(4, 1fr); gap: 8px; }

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
    color: var(--accent-danger); font-size: 12px; background: color-mix(in srgb, var(--accent-danger) 10%, transparent);
    border: 1px solid color-mix(in srgb, var(--accent-danger) 20%, transparent); border-radius: var(--border-radius-sm);
    padding: 8px 12px; text-align: center;
  }

  /* ─── Buttons ───────────────────────────── */
  .accent-btn {
    display: flex; align-items: center; justify-content: center; gap: 8px;
    padding: 10px 16px; border-radius: var(--border-radius-md);
    background: var(--accent-primary); color: var(--on-accent, #000); border: none;
    font-size: 13px; font-weight: 700; cursor: pointer;
    transition: all 0.15s;
  }
  .accent-btn:hover { background: var(--accent-hover); }
  .accent-btn:disabled { opacity: 0.5; cursor: not-allowed; }
  .accent-btn.full-width { width: 100%; }
</style>
