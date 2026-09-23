<script lang="ts">
  import { onMount, onDestroy, createEventDispatcher } from 'svelte';
  import { browser } from '$app/environment';
  import { authStore } from '$lib/stores/authStore';

  export let isOpen = false;

  const dispatch = createEventDispatcher<{
    close: void;
    selectPhoto: { id: string };
  }>();

  interface MapPoint {
    id: string;
    lat: number;
    lng: number;
    thumb_path: string;
  }

  let mapContainer: HTMLDivElement;
  let map: any = null;
  let clusterGroup: any = null;
  let points: MapPoint[] = [];
  let isLoading = true;

  function resolveUrl(path: string | undefined | null): string {
    if (!path) return '';
    if (path.startsWith('http')) return path;
    const clean = path.startsWith('/') ? path.slice(1) : path;
    return clean.startsWith('users/') ? `/${clean}` : `/thumbs/${clean}`;
  }

  async function loadLocationData() {
    isLoading = true;
    try {
      const res = await fetch('/api/media/locations');
      if (res.status === 401) {
        authStore.checkStatus();
        return;
      }
      if (res.ok) {
        points = await res.json();
      }
    } catch (e) {
      console.error('Failed to load map points', e);
    } finally {
      isLoading = false;
    }
  }

  async function initMap() {
    if (!browser || !mapContainer) return;

    const L = (await import('leaflet')).default;
    await import('leaflet.markercluster');

    if (map) {
      map.remove();
      map = null;
    }

    const defaultCenter: [number, number] = points.length > 0
      ? [points[0].lat, points[0].lng]
      : [20, 0];
    const defaultZoom = points.length > 0 ? 4 : 2;

    map = L.map(mapContainer, {
      zoomControl: false,
      attributionControl: false,
      maxZoom: 18,
      minZoom: 2
    }).setView(defaultCenter, defaultZoom);

    L.tileLayer('https://{s}.tile.openstreetmap.org/{z}/{x}/{y}.png', {
      maxZoom: 19
    }).addTo(map);

    clusterGroup = (L as any).markerClusterGroup({
      showCoverageOnHover: false,
      maxClusterRadius: 55,
      spiderfyOnMaxZoom: true,
      zoomToBoundsOnClick: true,
      iconCreateFunction: (cluster: any) => {
        const markers = cluster.getAllChildMarkers();
        const count = cluster.getChildCount();
        const primaryThumb = markers[0]?.options?.thumbUrl || '';

        return L.divIcon({
          html: `
            <div class="apple-photo-bubble">
              <img src="${primaryThumb}" alt="" loading="lazy" />
              <div class="apple-photo-badge">${count}</div>
            </div>
          `,
          className: 'apple-map-pin',
          iconSize: [48, 48],
          iconAnchor: [24, 48]
        });
      }
    });

    const bounds = L.latLngBounds([]);

    for (const pt of points) {
      const thumb = resolveUrl(pt.thumb_path);
      const icon = L.divIcon({
        html: `
          <div class="apple-photo-bubble">
            <img src="${thumb}" alt="" loading="lazy" />
          </div>
        `,
        className: 'apple-map-pin',
        iconSize: [48, 48],
        iconAnchor: [24, 48]
      });

      const marker = L.marker([pt.lat, pt.lng], {
        icon,
        thumbUrl: thumb,
        assetId: pt.id
      } as any);

      marker.on('click', () => {
        dispatch('selectPhoto', { id: pt.id });
      });

      clusterGroup.addLayer(marker);
      bounds.extend([pt.lat, pt.lng]);
    }

    map.addLayer(clusterGroup);

    if (points.length > 0) {
      map.fitBounds(bounds, { padding: [40, 40], maxZoom: 14 });
    }
  }

  $: if (isOpen && browser) {
    (async () => {
      await loadLocationData();
      setTimeout(() => initMap(), 80);
    })();
  }

  onDestroy(() => {
    if (map) {
      map.remove();
      map = null;
    }
  });

  function handleKeydown(e: KeyboardEvent) {
    if (e.key === 'Escape') dispatch('close');
  }
</script>

<svelte:window on:keydown={handleKeydown} />

{#if isOpen}
  <!-- Centered Modal Backdrop -->
  <!-- svelte-ignore a11y_click_events_have_key_events -->
  <!-- svelte-ignore a11y_no_noninteractive_element_interactions -->
  <div
    class="fixed inset-0 z-50 flex items-center justify-center bg-black/60 backdrop-blur-md p-3 sm:p-6 select-none"
    role="dialog"
    aria-modal="true"
    aria-label="Places Map"
    tabindex="-1"
    on:click|self={() => dispatch('close')}
  >
    <!-- Floating Modal Card Shell -->
    <div
      class="glass-panel rounded-3xl w-full max-w-4xl h-[78vh] shadow-2xl flex flex-col overflow-hidden border border-[var(--border-glass)] bg-[var(--bg-surface-elevated)] relative"
    >
      <!-- Header Bar -->
      <div class="px-5 py-3.5 border-b border-[var(--border-glass)] flex items-center justify-between z-20 bg-[var(--bg-surface)]">
        <div class="flex items-center gap-2">
          <span class="text-sm">🗺️</span>
          <h2 class="text-sm font-bold tracking-tight text-[var(--text-main)]">Places</h2>
          <span class="text-[10px] text-[var(--text-muted)] font-mono">({points.length} photos)</span>
        </div>

        <button
          type="button"
          on:click={() => dispatch('close')}
          class="w-7 h-7 flex items-center justify-center rounded-full glass-panel text-[var(--text-muted)] hover:text-[var(--text-main)] transition-colors spring-tap cursor-pointer text-xs"
          title="Close (Esc)"
          aria-label="Close Map"
        >
          ✕
        </button>
      </div>

      <!-- Map Display Viewport -->
      <div class="relative flex-1 w-full h-full overflow-hidden bg-[var(--bg-primary)]">
        <div bind:this={mapContainer} class="w-full h-full z-10"></div>

        {#if isLoading}
          <div class="absolute inset-0 z-40 bg-[var(--bg-primary)]/70 backdrop-blur-md flex flex-col items-center justify-center gap-2">
            <div class="w-6 h-6 border-2 border-purple-500/20 border-t-purple-500 rounded-full animate-spin"></div>
            <span class="text-xs text-[var(--text-muted)] font-mono">Plotting photo coordinates...</span>
          </div>
        {/if}
      </div>
    </div>
  </div>
{/if}