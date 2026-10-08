// photo-app/frontend/src/lib/utils/mediaEvents.ts

export interface AssetReadyData {
  asset_id: string;
  thumb_path: string;
  folder_path: string;
}

export interface AssetFailedData {
  asset_id: string;
  error: string;
}

export interface AlbumUpdatedData {
  album_id: string | null;
  folder_path: string;
  asset_id: string;
}

export interface PeopleUpdatedData {
  user_id: string;
  new_people_count: number;
  affected_person_ids: string[];
}

export interface AiCompletedData {
  asset_id: string;
  faces_detected: number;
  tags_count: number;
}

export interface MediaEventHandlers {
  onAssetReady?: (data: AssetReadyData) => void;
  onBatchAssetsReady?: (batch: AssetReadyData[]) => void;
  onAssetFailed?: (data: AssetFailedData) => void;
  onAlbumUpdated?: (data: AlbumUpdatedData) => void;
  onPeopleUpdated?: (data: PeopleUpdatedData) => void;
  onAiCompleted?: (data: AiCompletedData) => void;
}

export function initMediaEvents(handlers: MediaEventHandlers) {
  const source = new EventSource('/api/events');

  let readyBatch: AssetReadyData[] = [];
  let batchTimer: ReturnType<typeof setTimeout> | null = null;

  function flushBatch() {
    if (readyBatch.length > 0) {
      if (handlers.onBatchAssetsReady) {
        handlers.onBatchAssetsReady([...readyBatch]);
      }
      readyBatch = [];
    }
    batchTimer = null;
  }

  function parseEvent<T>(e: MessageEvent): T | null {
    try {
      const parsed = JSON.parse(e.data);
      return (parsed.data ?? parsed) as T;
    } catch (err) {
      console.error('[SSE] Failed parsing event payload', err, e.data);
      return null;
    }
  }

  // 1. Batch AssetReady events to avoid rapid UI refetches
  source.addEventListener('asset_ready', (event) => {
    const data = parseEvent<AssetReadyData>(event as MessageEvent);
    if (!data) return;

    if (handlers.onAssetReady) {
      handlers.onAssetReady(data);
    }

    if (handlers.onBatchAssetsReady) {
      readyBatch.push(data);
      if (!batchTimer) {
        batchTimer = setTimeout(flushBatch, 250);
      }
    }
  });

  // 2. Asset processing failed
  source.addEventListener('asset_failed', (event) => {
    const data = parseEvent<AssetFailedData>(event as MessageEvent);
    if (data && handlers.onAssetFailed) {
      handlers.onAssetFailed(data);
    }
  });

  // 3. Album or Folder item count/cover changed
  source.addEventListener('album_updated', (event) => {
    const data = parseEvent<AlbumUpdatedData>(event as MessageEvent);
    if (data && handlers.onAlbumUpdated) {
      handlers.onAlbumUpdated(data);
    }
  });

  // 4. Person cluster updated
  source.addEventListener('people_updated', (event) => {
    const data = parseEvent<PeopleUpdatedData>(event as MessageEvent);
    if (data && handlers.onPeopleUpdated) {
      handlers.onPeopleUpdated(data);
    }
  });

  // 5. AI vectors, tags, and poses completed
  source.addEventListener('ai_completed', (event) => {
    const data = parseEvent<AiCompletedData>(event as MessageEvent);
    if (data && handlers.onAiCompleted) {
      handlers.onAiCompleted(data);
    }
  });

  source.onerror = (err) => {
    console.warn('[SSE] EventSource connection dropped or reconnecting', err);
  };

  return {
    close: () => {
      if (batchTimer) clearTimeout(batchTimer);
      source.close();
    }
  };
}