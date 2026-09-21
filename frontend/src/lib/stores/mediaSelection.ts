import { writable, derived, get } from 'svelte/store';
import { authStore } from '$lib/stores/authStore';

export function createMediaSelection(onChanged: () => void) {
  // Native Set for true O(1) membership checks and zero object-spread churn
  const selectedIds = writable<Set<string>>(new Set());
  const isActionLoading = writable<boolean>(false);

  // Instant O(1) size property — no Object.keys() array allocation
  const selectedCount = derived(selectedIds, ($set) =>$set.size);

  function clearSelection() {
    selectedIds.set(new Set());
  }

  // Fast O(1) toggle
  function toggle(id: string) {
    selectedIds.update((set) => {
      const next = new Set(set);
      if (next.has(id)) {
        next.delete(id);
      } else {
        next.add(id);
      }
      return next;
    });
  }

  // Explicit batch setter (for future drag-to-select or select-all)
  function selectAll(ids: string[]) {
    selectedIds.set(new Set(ids));
  }

  async function batchToggleDelete() {
    const ids = Array.from(get(selectedIds));
    if (ids.length === 0 || get(isActionLoading)) return;
    isActionLoading.set(true);

    try {
      const res = await fetch('/api/assets/batch/delete', {
        method: 'POST',
        headers: { 'Content-Type': 'application/json' },
        body: JSON.stringify({ ids })
      });

      if (res.status === 401) {
        authStore.checkStatus();
        return;
      }

      if (!res.ok) throw new Error(`HTTP ${res.status}`);

      clearSelection();
      onChanged();
    } catch (e) {
      console.error('Batch delete error:', e);
    } finally {
      isActionLoading.set(false);
    }
  }

  async function batchPurge() {
    const ids = Array.from(get(selectedIds));
    if (ids.length === 0 || get(isActionLoading)) return;
    if (!confirm(`Permanently delete ${ids.length} item(s)? This cannot be undone.`)) return;

    isActionLoading.set(true);
    try {
      const res = await fetch('/api/assets/batch/purge', {
        method: 'POST',
        headers: { 'Content-Type': 'application/json' },
        body: JSON.stringify({ ids })
      });

      if (res.status === 401) {
        authStore.checkStatus();
        return;
      }

      if (!res.ok) throw new Error(`HTTP ${res.status}`);

      clearSelection();
      onChanged();
    } catch (e) {
      console.error('Batch purge error:', e);
    } finally {
      isActionLoading.set(false);
    }
  }

  return {
    selectedIds,
    selectedCount,
    isActionLoading,
    toggle,
    selectAll,
    clearSelection,
    batchToggleDelete,
    batchPurge
  };
}