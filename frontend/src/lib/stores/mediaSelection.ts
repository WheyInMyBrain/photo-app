import { writable, derived, get } from 'svelte/store';
import type { MediaItem } from '$lib/types/media';
import { authStore } from '$lib/stores/authStore';

export function createMediaSelection(onChanged: () => void) {
  const selectedMap = writable<Record<string, boolean>>({});
  const lastSelectedId = writable<string | null>(null);
  const isActionLoading = writable(false);

  const selectedCount = derived(selectedMap, ($map) => Object.keys($map).length);

  function clearSelection() {
    selectedMap.set({});
    lastSelectedId.set(null);
  }

  function toggleSelect(
    id: string,
    e: MouseEvent,
    items: MediaItem[],
    itemIndexMap: Map<string, number>
  ) {
    e.stopPropagation();
    const currentMap = { ...get(selectedMap) };
    const lastId = get(lastSelectedId);

    if (e.shiftKey && lastId && lastId !== id) {
      const startIdx = itemIndexMap.get(lastId);
      const endIdx = itemIndexMap.get(id);

      if (startIdx !== undefined && endIdx !== undefined) {
        const [low, high] = [Math.min(startIdx, endIdx), Math.max(startIdx, endIdx)];
        for (let i = low; i <= high; i++) {
          const target = items[i];
          if (target) currentMap[target.id] = true;
        }
        selectedMap.set(currentMap);
        lastSelectedId.set(id);
        return;
      }
    }

    if (currentMap[id]) {
      delete currentMap[id];
      lastSelectedId.set(null);
    } else {
      currentMap[id] = true;
      lastSelectedId.set(id);
    }

    selectedMap.set(currentMap);
  }

  async function batchToggleDelete() {
    const ids = Object.keys(get(selectedMap));
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
    const ids = Object.keys(get(selectedMap));
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
    selectedMap,
    selectedCount,
    isActionLoading,
    toggleSelect,
    clearSelection,
    batchToggleDelete,
    batchPurge
  };
}