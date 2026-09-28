<!-- photo-app/frontend/src/lib/components/AuthScreen.svelte -->
<script lang="ts">
  import { authStore } from '$lib/stores/authStore';

  let mode: 'login' | 'register' = 'login';
  let username = '';
  let password = '';
  let displayName = '';
  let errorMsg = '';
  let isSubmitting = false;
  let showPassword = false;

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

<div class="fixed inset-0 z-50 flex items-center justify-center p-4 select-none bg-[var(--bg-primary)] text-[var(--text-main)] overflow-hidden">
  <!-- Subtle Ambient Glow -->
  <div class="absolute w-96 h-96 rounded-full bg-purple-600/10 dark:bg-purple-600/15 blur-[120px] pointer-events-none -top-12 -left-12"></div>
  <div class="absolute w-80 h-80 rounded-full bg-blue-600/5 dark:bg-blue-600/10 blur-[100px] pointer-events-none -bottom-10 -right-10"></div>

  <div class="relative w-full max-w-sm p-6 sm:p-8 auth-card rounded-3xl space-y-6">
    <!-- Header & Specular Icon Badge -->
    <div class="text-center space-y-3">
      <div class="w-13 h-13 rounded-2xl auth-icon-plate mx-auto flex items-center justify-center text-purple-600 dark:text-purple-400">
        <svg xmlns="http://www.w3.org/2000/svg" class="w-6 h-6" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2" stroke-linecap="round" stroke-linejoin="round">
          <rect x="3" y="11" width="18" height="11" rx="2" ry="2"></rect>
          <path d="M7 11V7a5 5 0 0 1 10 0v4"></path>
        </svg>
      </div>
      <div>
        <h1 class="text-xl font-bold tracking-tight text-[var(--text-main)]">Vault</h1>
        <p class="text-xs text-[var(--text-muted)] mt-0.5 tracking-tight">Encrypted personal media archive</p>
      </div>
    </div>

    <!-- Segmented Mode Switcher -->
    <div class="auth-segmented p-1 rounded-2xl flex text-xs">
      <button
        type="button"
        on:click={() => { mode = 'login'; errorMsg = ''; }}
        class="flex-1 py-2 rounded-xl text-center transition-all cursor-pointer font-medium spring-tap {mode === 'login' ? 'auth-seg-active text-[var(--text-main)] font-semibold' : 'text-[var(--text-muted)] hover:text-[var(--text-main)]'}"
      >
        Sign In
      </button>
      <button
        type="button"
        on:click={() => { mode = 'register'; errorMsg = ''; }}
        class="flex-1 py-2 rounded-xl text-center transition-all cursor-pointer font-medium spring-tap {mode === 'register' ? 'auth-seg-active text-[var(--text-main)] font-semibold' : 'text-[var(--text-muted)] hover:text-[var(--text-main)]'}"
      >
        Create Account
      </button>
    </div>

    {#if errorMsg}
      <div class="p-3 bg-rose-500/10 border border-rose-500/25 rounded-2xl text-xs text-rose-600 dark:text-rose-300 text-center font-medium flex items-center justify-center gap-1.5">
        <svg xmlns="http://www.w3.org/2000/svg" class="w-3.5 h-3.5 flex-shrink-0" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2" stroke-linecap="round" stroke-linejoin="round">
          <circle cx="12" cy="12" r="10"></circle>
          <line x1="12" y1="8" x2="12" y2="12"></line>
          <line x1="12" y1="16" x2="12.01" y2="16"></line>
        </svg>
        <span>{errorMsg}</span>
      </div>
    {/if}

    <!-- Input Form -->
    <form on:submit|preventDefault={handleSubmit} class="space-y-3.5">
      {#if mode === 'register'}
        <div class="space-y-1">
          <label for="display_name" class="block text-[11px] font-medium text-[var(--text-muted)] pl-1">
            Display Name
          </label>
          <div class="relative">
            <input
              id="display_name"
              type="text"
              bind:value={displayName}
              placeholder="e.g. Alex"
              autocomplete="name"
              class="auth-input w-full rounded-xl px-3.5 py-2.5 text-xs text-[var(--text-main)] placeholder-[var(--text-muted)] outline-none transition-all"
            />
          </div>
        </div>
      {/if}

      <div class="space-y-1">
        <label for="username" class="block text-[11px] font-medium text-[var(--text-muted)] pl-1">
          Username
        </label>
        <div class="relative">
          <input
            id="username"
            type="text"
            required
            bind:value={username}
            autocomplete="username"
            autocapitalize="none"
            placeholder="username"
            class="auth-input w-full rounded-xl px-3.5 py-2.5 text-xs text-[var(--text-main)] placeholder-[var(--text-muted)] outline-none transition-all"
          />
        </div>
      </div>

      <div class="space-y-1">
        <label for="password" class="block text-[11px] font-medium text-[var(--text-muted)] pl-1">
          Password
        </label>
        <div class="relative flex items-center">
          <input
            id="password"
            type={showPassword ? 'text' : 'password'}
            required
            minlength="6"
            bind:value={password}
            autocomplete={mode === 'login' ? 'current-password' : 'new-password'}
            placeholder="••••••••"
            class="auth-input w-full rounded-xl pl-3.5 pr-10 py-2.5 text-xs text-[var(--text-main)] placeholder-[var(--text-muted)] outline-none transition-all"
          />
          <button
            type="button"
            on:click={() => (showPassword = !showPassword)}
            class="absolute right-3 text-[var(--text-muted)] hover:text-[var(--text-main)] cursor-pointer p-0.5 transition-colors"
            title={showPassword ? 'Hide password' : 'Show password'}
            aria-label="Toggle password visibility"
          >
            {#if showPassword}
              <svg xmlns="http://www.w3.org/2000/svg" class="w-3.5 h-3.5" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2" stroke-linecap="round" stroke-linejoin="round">
                <path d="M17.94 17.94A10.07 10.07 0 0 1 12 20c-7 0-11-8-11-8a18.45 18.45 0 0 1 5.06-5.94M9.9 4.24A9.12 9.12 0 0 1 12 4c7 0 11 8 11 8a18.5 18.5 0 0 1-2.16 3.19m-6.72-1.07a3 3 0 1 1-4.24-4.24"></path>
                <line x1="1" y1="1" x2="23" y2="23"></line>
              </svg>
            {:else}
              <svg xmlns="http://www.w3.org/2000/svg" class="w-3.5 h-3.5" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2" stroke-linecap="round" stroke-linejoin="round">
                <path d="M1 12s4-8 11-8 11 8 11 8-4 8-11 8-11-8-11-8z"></path>
                <circle cx="12" cy="12" r="3"></circle>
              </svg>
            {/if}
          </button>
        </div>
      </div>

      <button
        type="submit"
        disabled={isSubmitting}
        class="w-full mt-2 py-2.5 bg-purple-600 hover:bg-purple-500 text-white font-semibold text-xs rounded-xl shadow-lg shadow-purple-600/25 transition-all spring-tap cursor-pointer disabled:opacity-50 flex items-center justify-center gap-2"
      >
        {#if isSubmitting}
          <span class="w-3.5 h-3.5 border-2 border-white/20 border-t-white rounded-full animate-spin"></span>
          <span>Verifying...</span>
        {:else}
          <span>{mode === 'login' ? 'Sign In to Vault' : 'Create Vault Account'}</span>
        {/if}
      </button>
    </form>
  </div>
</div>

<style>
  .auth-card {
    background: var(--bg-surface-elevated);
    border: 1px solid var(--border-glass);
    backdrop-filter: blur(40px) saturate(190%);
    -webkit-backdrop-filter: blur(40px) saturate(190%);
    box-shadow:
      0 30px 70px var(--dock-shadow),
      inset 0 1px 0 0 var(--border-specular);
  }

  .auth-icon-plate {
    background: var(--pill-bg);
    border: 1px solid var(--border-glass);
    box-shadow: inset 0 1px 0 var(--border-specular);
  }

  .auth-segmented {
    background: var(--pill-bg);
    border: 1px solid var(--border-glass);
  }

  .auth-seg-active {
    background: var(--dock-bg);
    border: 1px solid var(--dock-border);
    box-shadow:
      0 2px 6px var(--dock-shadow),
      inset 0 1px 0 var(--dock-highlight);
  }

  .auth-input {
    background: var(--card-bg);
    border: 1px solid var(--border-glass);
    box-shadow: inset 0 1px 2px rgba(0, 0, 0, 0.08);
  }

  .auth-input:focus {
    border-color: var(--border-subtle);
    box-shadow: 0 0 0 1px var(--border-subtle);
  }
</style>