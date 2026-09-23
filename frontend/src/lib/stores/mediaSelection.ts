import { writable, get } from 'svelte/store';
import { authStore } from '$lib/stores/authStore';

export function createMediaSelection(onChanged: () => void) {
  const selectedSet = new Set<string>();
  const selectedCount = writable<number>(0);
  const isSelectionActive = writable<boolean>(false);
  const isActionLoading = writable<boolean>(false);

  function clearSelection() {
    selectedSet.clear();
    selectedCount.set(0);
    isSelectionActive.set(false);

    if (typeof document !== 'undefined') {
      const selectedEls = document.querySelectorAll('[data-asset-id][aria-pressed="true"]');
      for (let i = 0; i < selectedEls.length; i++) {
        selectedEls[i].setAttribute('aria-pressed', 'false');
      }
    }
  }

  function toggle(id: string, cardEl?: HTMLElement | null) {
    const nextState = !selectedSet.has(id);
    setTargetState(id, cardEl, nextState);
  }

  function setTargetState(id: string, cardEl: HTMLElement | null | undefined, state: boolean) {
    if (state) {
      selectedSet.add(id);
    } else {
      selectedSet.delete(id);
    }

    const el = cardEl ?? document.querySelector(`[data-asset-id="${id}"]`);
    if (el) {
      el.setAttribute('aria-pressed', state ? 'true' : 'false');
    }

    const count = selectedSet.size;
    selectedCount.set(count);
    isSelectionActive.set(count > 0);
  }

  function getSelectedIds(): string[] {
    return Array.from(selectedSet);
  }

  async function batchToggleDelete() {
    const ids = getSelectedIds();
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
    const ids = getSelectedIds();
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
    selectedCount,
    isSelectionActive,
    isActionLoading,
    toggle,
    setTargetState,
    clearSelection,
    getSelectedIds,
    batchToggleDelete,
    batchPurge
  };
}