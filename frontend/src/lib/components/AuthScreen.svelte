<script lang="ts">
  import { authStore } from '$lib/stores/authStore';

  let mode: 'login' | 'register' = 'login';
  let username = '';
  let password = '';
  let displayName = '';
  let errorMsg = '';
  let isSubmitting = false;

  async function handleSubmit() {
    errorMsg = '';
    isSubmitting = true;

    const endpoint = mode === 'login' ? '/api/auth/login' : '/api/auth/register';
    const body = mode === 'login' 
      ? { username, password } 
      : { username, password, display_name: displayName.trim() || undefined };

    try {
      const res = await fetch(endpoint, {
        method: 'POST',
        headers: { 'Content-Type': 'application/json' },
        body: JSON.stringify(body)
      });

      const data = await res.json();

      if (!res.ok) {
        errorMsg = data.error || 'Authentication failed';
        return;
      }

      authStore.loginSuccess(data);
    } catch (err: any) {
      errorMsg = err.message || 'Network error';
    } finally {
      isSubmitting = false;
    }
  }
</script>

<div class="fixed inset-0 z-50 flex items-center justify-center p-4 select-none bg-[var(--bg-primary)] text-[var(--text-main)]">
  <!-- Specular Ambient Background Glow -->
  <div class="absolute w-80 h-80 rounded-full bg-purple-600/15 blur-[100px] pointer-events-none"></div>

  <div class="relative w-full max-w-sm p-7 glass-panel rounded-3xl shadow-2xl space-y-6">
    <!-- App Header & Icon Badge -->
    <div class="text-center space-y-2">
      <div class="w-12 h-12 rounded-2xl bg-purple-600/10 border border-purple-500/20 text-purple-400 mx-auto flex items-center justify-center text-xl font-bold shadow-inner">
        🔒
      </div>
      <div>
        <h1 class="text-xl font-bold tracking-tight text-[var(--text-main)]">Vault</h1>
        <p class="text-xs text-[var(--text-muted)] mt-0.5">Encrypted personal photo cloud</p>
      </div>
    </div>

    <!-- Segmented Mode Selector Switch -->
    <div class="flex p-1 rounded-xl glass-panel text-xs">
      <button
        type="button"
        on:click={() => { mode = 'login'; errorMsg = ''; }}
        class="flex-1 py-1.5 rounded-lg text-center transition-all cursor-pointer font-medium {mode === 'login' ? 'bg-purple-600 text-white shadow-md' : 'text-[var(--text-muted)] hover:text-[var(--text-main)]'}"
      >
        Sign In
      </button>
      <button
        type="button"
        on:click={() => { mode = 'register'; errorMsg = ''; }}
        class="flex-1 py-1.5 rounded-lg text-center transition-all cursor-pointer font-medium {mode === 'register' ? 'bg-purple-600 text-white shadow-md' : 'text-[var(--text-muted)] hover:text-[var(--text-main)]'}"
      >
        Create Account
      </button>
    </div>

    {#if errorMsg}
      <div class="p-3 bg-red-500/10 border border-red-500/30 rounded-xl text-xs text-red-400 text-center font-medium">
        {errorMsg}
      </div>
    {/if}

    <!-- Input Form -->
    <form on:submit|preventDefault={handleSubmit} class="space-y-4">
      {#if mode === 'register'}
        <div>
          <label for="display_name" class="block text-[11px] font-medium text-[var(--text-muted)] mb-1 pl-1">Display Name</label>
          <input
            id="display_name"
            type="text"
            bind:value={displayName}
            placeholder="Aseem"
            autocomplete="name"
            class="w-full bg-[var(--bg-surface-elevated)] border border-[var(--border-glass)] rounded-xl px-3.5 py-2 text-xs text-[var(--text-main)] placeholder-[var(--text-muted)] focus:outline-none focus:ring-2 focus:ring-purple-500/50 transition-all"
          />
        </div>
      {/if}

      <div>
        <label for="username" class="block text-[11px] font-medium text-[var(--text-muted)] mb-1 pl-1">Username</label>
        <input
          id="username"
          type="text"
          required
          bind:value={username}
          autocomplete="username"
          autocapitalize="none"
          placeholder="username"
          class="w-full bg-[var(--bg-surface-elevated)] border border-[var(--border-glass)] rounded-xl px-3.5 py-2 text-xs text-[var(--text-main)] placeholder-[var(--text-muted)] focus:outline-none focus:ring-2 focus:ring-purple-500/50 transition-all"
        />
      </div>

      <div>
        <label for="password" class="block text-[11px] font-medium text-[var(--text-muted)] mb-1 pl-1">Password</label>
        <input
          id="password"
          type="password"
          required
          minlength="6"
          bind:value={password}
          autocomplete={mode === 'login' ? 'current-password' : 'new-password'}
          placeholder="••••••••"
          class="w-full bg-[var(--bg-surface-elevated)] border border-[var(--border-glass)] rounded-xl px-3.5 py-2 text-xs text-[var(--text-main)] placeholder-[var(--text-muted)] focus:outline-none focus:ring-2 focus:ring-purple-500/50 transition-all"
        />
      </div>

      <button
        type="submit"
        disabled={isSubmitting}
        class="w-full mt-3 py-2.5 bg-purple-600 hover:bg-purple-500 active:scale-[0.98] text-white font-semibold text-xs rounded-xl shadow-lg shadow-purple-600/20 transition-all spring-tap cursor-pointer disabled:opacity-50"
      >
        {#if isSubmitting}
          <span class="inline-flex items-center gap-1.5">
            <span class="w-3 h-3 border-2 border-white/30 border-t-white rounded-full animate-spin"></span>
            <span>Verifying...</span>
          </span>
        {:else}
          {mode === 'login' ? 'Sign In to Vault' : 'Create Vault Account'}
        {/if}
      </button>
    </form>
  </div>
</div>