import { writable } from 'svelte/store';
import { browser } from '$app/environment';

export interface UserProfile {
  id: string;
  username: string;
  displayName: string | null;
  hasPasskey: boolean;
}

interface AuthState {
  isAuthenticated: boolean;
  isLoading: boolean;
  user: UserProfile | null;
}

const initialState: AuthState = {
  isAuthenticated: false,
  isLoading: true,
  user: null
};

function createAuthStore() {
  const { subscribe, set, update } = writable<AuthState>(initialState);

  return {
    subscribe,
    checkStatus: async () => {
      if (!browser) return;
      try {
        const res = await fetch('/api/auth/status');
        if (res.ok) {
          const data = await res.json();
          if (data.is_authenticated && data.user_id) {
            set({
              isAuthenticated: true,
              isLoading: false,
              user: {
                id: data.user_id,
                username: data.username,
                displayName: data.display_name,
                hasPasskey: data.has_passkey
              }
            });
            return;
          }
        }
        set({ isAuthenticated: false, isLoading: false, user: null });
      } catch (err) {
        console.error('Failed to check auth status', err);
        set({ isAuthenticated: false, isLoading: false, user: null });
      }
    },
    loginSuccess: (data: { user_id: string; username: string; display_name: string | null }) => {
      set({
        isAuthenticated: true,
        isLoading: false,
        user: {
          id: data.user_id,
          username: data.username,
          displayName: data.display_name,
          hasPasskey: false
        }
      });
    },
    logout: async () => {
      try {
        await fetch('/api/auth/logout', { method: 'POST' });
      } finally {
        set({ isAuthenticated: false, isLoading: false, user: null });
        window.location.reload();
      }
    }
  };
}

export const authStore = createAuthStore();