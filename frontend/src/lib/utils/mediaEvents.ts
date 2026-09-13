export interface MediaEventPayload {
  event_type: 'asset_ready' | 'asset_failed';
  asset_id: string;
  thumb_path: string;
}

export function initMediaEvents(onAssetReady: (assetId: string) => void) {
  const source = new EventSource('/api/events');

  source.addEventListener('media_update', (event) => {
    try {
      const payload: MediaEventPayload = JSON.parse(event.data);
      if (payload.event_type === 'asset_ready') {
        onAssetReady(payload.asset_id);
      }
    } catch (err) {
      console.error('[SSE] Failed parsing event payload', err);
    }
  });

  source.onerror = (err) => {
    console.warn('[SSE] EventSource encountered error / reconnecting', err);
  };

  return {
    close: () => source.close()
  };
}