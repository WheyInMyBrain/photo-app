<script lang="ts">
  import { onMount, createEventDispatcher } from 'svelte';
  import { isBiometricAvailable, registerBiometrics, authenticateBiometrics } from '$lib/utils/webauthn';

  export let isOpen = false;

  const dispatch = createEventDispatcher<{
    success: { token: string };
    cancel: void;
  }>();

  let isInitialized = false;
  let hasPasskey = false;
  let password = '';
  let errorMsg = '';
  let isLoading = false;
  let canUseBiometrics = false;

  // Svelte action to replace HTML autofocus without accessibility warnings
  function focusInput(node: HTMLElement) {
    node.focus();
  }

  onMount(async () => {
    canUseBiometrics = await isBiometricAvailable();
    await checkStatus();
  });

  async function checkStatus() {
    try {
      const res = await fetch('/api/vault/status');
      if (res.ok) {
        const data = await res.json();
        isInitialized = data.is_initialized;
        hasPasskey = data.has_passkey;

        // Auto-trigger biometric prompt if already configured
        if (isInitialized && hasPasskey && isOpen) {
          triggerBiometricUnlock();
        }
      }
    } catch (e) {
      console.error(e);
    }
  }

  $: if (isOpen) {
    errorMsg = '';
    password = '';
    checkStatus();
  }

  async function handlePasswordSubmit() {
    if (!password.trim()) return;
    isLoading = true;
    errorMsg = '';

    const endpoint = isInitialized ? '/api/vault/unlock/password' : '/api/vault/setup';

    try {
      const res = await fetch(endpoint, {
        method: 'POST',
        headers: { 'Content-Type': 'application/json' },
        body: JSON.stringify({ password })
      });

      if (!res.ok) {
        const err = await res.text();
        throw new Error(err || 'Authentication failed');
      }

      const data = await res.json();

      // If this was first-time setup and device supports biometrics, offer enrollment
      if (!isInitialized && canUseBiometrics) {
        await registerBiometrics();
      }

      dispatch('success', { token: data.token });
    } catch (err: any) {
      errorMsg = err.message || 'Error occurred';
    } finally {
      isLoading = false;
    }
  }

  async function triggerBiometricUnlock() {
    isLoading = true;
    errorMsg = '';
    const success = await authenticateBiometrics();
    isLoading = false;
    if (success) {
      dispatch('success', { token: 'biometric-verified' });
    } else {
      errorMsg = 'Biometric scan failed. Enter password.';
    }
  }
</script>

{#if isOpen}
  <div class="fixed inset-0 z-50 flex items-center justify-center bg-black/80 backdrop-blur-sm p-4">
    <div class="bg-neutral-900 border border-neutral-800 rounded-2xl p-6 max-w-sm w-full shadow-2xl space-y-4">
      <div class="text-center space-y-1">
        <div class="w-12 h-12 rounded-full bg-purple-950/80 border border-purple-800/60 text-purple-300 flex items-center justify-center text-xl mx-auto mb-2">
          🔒
        </div>
        <h3 class="text-base font-bold text-white">
          {isInitialized ? 'Unlock Private Vault' : 'Create Vault Password'}
        </h3>
        <p class="text-xs text-neutral-400">
          {isInitialized
            ? 'Verify your identity to access secret media.'
            : 'Set a master password. You can enable fingerprint/TouchID next.'}
        </p>
      </div>

      {#if errorMsg}
        <div class="text-xs text-red-400 bg-red-950/40 border border-red-900/60 p-2 rounded-lg text-center">
          {errorMsg}
        </div>
      {/if}

      <!-- Biometric Instant Button if already configured -->
      {#if isInitialized && hasPasskey}
        <button
          type="button"
          on:click={triggerBiometricUnlock}
          disabled={isLoading}
          class="w-full py-2 bg-purple-600 hover:bg-purple-500 text-white font-medium text-xs rounded-lg flex items-center justify-center gap-2 transition-colors cursor-pointer"
        >
          <span>Use Fingerprint / FaceID</span>
        </button>

        <div class="flex items-center gap-2 my-2">
          <div class="h-px bg-neutral-800 flex-1"></div>
          <span class="text-[10px] text-neutral-500 uppercase">Or Password</span>
          <div class="h-px bg-neutral-800 flex-1"></div>
        </div>
      {/if}

      <!-- Password Input Field -->
      <form on:submit|preventDefault={handlePasswordSubmit} class="space-y-3">
        <input
          type="password"
          bind:value={password}
          placeholder={isInitialized ? 'Enter master password...' : 'Choose master password (min 6 chars)...'}
          use:focusInput
          class="w-full bg-neutral-950 border border-neutral-800 focus:border-purple-500 rounded-lg px-3 py-2 text-xs text-white placeholder-neutral-500 outline-none"
        />

        <div class="flex gap-2 pt-1">
          <button
            type="button"
            on:click={() => dispatch('cancel')}
            class="flex-1 py-1.5 rounded-lg text-xs text-neutral-400 hover:text-white border border-neutral-800 hover:bg-neutral-800 transition-colors cursor-pointer"
          >
            Cancel
          </button>
          <button
            type="submit"
            disabled={isLoading || !password}
            class="flex-1 py-1.5 rounded-lg text-xs font-semibold bg-purple-600 hover:bg-purple-500 text-white transition-colors disabled:opacity-50 cursor-pointer"
          >
            {isLoading ? 'Verifying...' : isInitialized ? 'Unlock' : 'Save & Enable'}
          </button>
        </div>
      </form>
    </div>
  </div>
{/if}