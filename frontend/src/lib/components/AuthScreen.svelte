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

<div class="fixed inset-0 z-50 flex items-center justify-center bg-neutral-950 text-neutral-100 p-4 font-sans">
  <div class="w-full max-w-sm p-6 bg-neutral-900 border border-neutral-800 rounded-2xl shadow-2xl space-y-5">
    
    <!-- Title -->
    <div class="text-center space-y-1">
      <h1 class="text-xl font-bold tracking-tight text-white">Vault Media</h1>
      <p class="text-xs text-neutral-400">Isolated personal photo & media cloud</p>
    </div>

    <!-- Mode Selector Tabs -->
    <div class="flex bg-neutral-950 p-0.5 rounded-lg border border-neutral-800 text-xs">
      <button
        type="button"
        on:click={() => { mode = 'login'; errorMsg = ''; }}
        class="flex-1 py-1.5 rounded-md text-center transition-all cursor-pointer {mode === 'login' ? 'bg-neutral-800 text-white font-medium shadow-sm' : 'text-neutral-400 hover:text-white'}"
      >
        Sign In
      </button>
      <button
        type="button"
        on:click={() => { mode = 'register'; errorMsg = ''; }}
        class="flex-1 py-1.5 rounded-md text-center transition-all cursor-pointer {mode === 'register' ? 'bg-neutral-800 text-white font-medium shadow-sm' : 'text-neutral-400 hover:text-white'}"
      >
        Create Account
      </button>
    </div>

    {#if errorMsg}
      <div class="p-2.5 bg-red-950/40 border border-red-800/80 rounded-lg text-xs text-red-300">
        {errorMsg}
      </div>
    {/if}

    <!-- Auth Form -->
    <form on:submit|preventDefault={handleSubmit} class="space-y-3">
      {#if mode === 'register'}
        <div>
          <label for="display_name" class="block text-[11px] text-neutral-400 mb-1">Display Name</label>
          <input
            id="display_name"
            type="text"
            bind:value={displayName}
            placeholder="Aseem"
            class="w-full bg-neutral-950 border border-neutral-800 rounded-lg px-3 py-2 text-xs text-white placeholder-neutral-600 focus:outline-none focus:border-neutral-600"
          />
        </div>
      {/if}

      <div>
        <label for="username" class="block text-[11px] text-neutral-400 mb-1">Username</label>
        <input
          id="username"
          type="text"
          required
          bind:value={username}
          autocomplete="username"
          placeholder="username"
          class="w-full bg-neutral-950 border border-neutral-800 rounded-lg px-3 py-2 text-xs text-white placeholder-neutral-600 focus:outline-none focus:border-neutral-600"
        />
      </div>

      <div>
        <label for="password" class="block text-[11px] text-neutral-400 mb-1">Password</label>
        <input
          id="password"
          type="password"
          required
          minlength="6"
          bind:value={password}
          autocomplete={mode === 'login' ? 'current-password' : 'new-password'}
          placeholder="••••••••"
          class="w-full bg-neutral-950 border border-neutral-800 rounded-lg px-3 py-2 text-xs text-white placeholder-neutral-600 focus:outline-none focus:border-neutral-600"
        />
      </div>

      <button
        type="submit"
        disabled={isSubmitting}
        class="w-full mt-2 py-2 bg-white hover:bg-neutral-200 text-neutral-950 font-medium text-xs rounded-lg transition-colors cursor-pointer disabled:opacity-50"
      >
        {isSubmitting ? 'Verifying...' : (mode === 'login' ? 'Sign In' : 'Register')}
      </button>
    </form>
  </div>
</div>