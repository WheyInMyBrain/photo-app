<script lang="ts">
  import { onMount, onDestroy, createEventDispatcher } from 'svelte';
  import type { FaceDetail, TagItem, SimilarItem, PersonCandidate } from '$lib/types/modal';
  import { createModalGestureController } from '$lib/utils/modalGestures';
  import PhotoDetailsSheet from './PhotoDetailsSheet.svelte';

  export let asset: {
    id: string;
    file_name: string;
    thumb_path: string;
    preview_path: string;
    mime_type: string;
    captured_at: string | null;
    is_favorite?: boolean | number;
    latitude?: number | null;
    longitude?: number | null;
  } | null = null;

  export let hasPrev = false;
  export let hasNext = false;
  export let initialRect: DOMRect | null = null;

  const dispatch = createEventDispatcher<{
    close: void;
    prev: void;
    next: void;
    toggleFavorite: { id: string; is_favorite: boolean };
    selectAsset: { id: string };
  }>();

  let isMorphing = Boolean(initialRect);
  let isHighResLoaded = false;
  let viewportEl: HTMLDivElement;

  // Sidebar Data
  let showMobileInfo = false;
  let isFavorite = false;
  let faces: FaceDetail[] = [];
  let tags: TagItem[] = [];
  let similarItems: SimilarItem[] = [];
  let knownPeople: PersonCandidate[] = [];
  let loadingDetails = true;
  let loadingSimilar = true;
  let showBoxes = true;
  let detailAbortCtrl: AbortController | null = null;

  $: isMotionMedia =
    Boolean(asset?.mime_type?.startsWith('video/')) || asset?.mime_type === 'image/gif';

  const gestures = createModalGestureController({
    onPrev: () => dispatch('prev'),
    onNext: () => dispatch('next'),
    onClose: () => dispatch('close'),
    hasPrev: () => hasPrev,
    hasNext: () => hasNext,
    isMotion: () => isMotionMedia
  });

  function resolveUrl(path: string | undefined | null): string {
    if (!path) return '';
    if (path.startsWith('http')) return path;
    const clean = path.startsWith('/') ? path.slice(1) : path;
    return clean.startsWith('users/') ? `/${clean}` : `/thumbs/${clean}`;
  }

  $: if (asset?.id) {
    isFavorite = Boolean(asset.is_favorite);
    showMobileInfo = false;
    isHighResLoaded = false;
    gestures.resetZoom();
    loadDetails(asset.id);
  }

  onMount(() => {
    const originalOverflow = document.body.style.overflow;
    document.body.style.overflow = 'hidden';

    if (isMorphing) {
      requestAnimationFrame(() => {
        isMorphing = false;
      });
    }

    fetch('/api/persons/names')
      .then((res) => (res.ok ? res.json() : []))
      .then((data) => (knownPeople = data))
      .catch(() => {});

    return () => {
      document.body.style.overflow = originalOverflow;
    };
  });

  onDestroy(() => {
    if (detailAbortCtrl) detailAbortCtrl.abort();
  });

  async function loadDetails(id: string) {
    if (detailAbortCtrl) detailAbortCtrl.abort();
    detailAbortCtrl = new AbortController();
    loadingDetails = true;
    loadingSimilar = true;

    try {
      const [fRes, tRes, sRes] = await Promise.all([
        fetch(`/api/assets/${id}/faces`, { signal: detailAbortCtrl.signal }),
        fetch(`/api/assets/${id}/tags`, { signal: detailAbortCtrl.signal }),
        fetch(`/api/assets/${id}/similar`, { signal: detailAbortCtrl.signal })
      ]);
      faces = fRes.ok ? await fRes.json() : [];
      tags = tRes.ok ? await tRes.json() : [];
      similarItems = sRes.ok ? await sRes.json() : [];
    } catch {
      faces = [];
      tags = [];
      similarItems = [];
    } finally {
      loadingDetails = false;
      loadingSimilar = false;
    }
  }

  async function toggleFavorite() {
    if (!asset) return;
    isFavorite = !isFavorite;
    try {
      const res = await fetch(`/api/assets/${asset.id}/favorite`, { method: 'POST' });
      if (res.ok) {
        const data = await res.json();
        isFavorite = Boolean(data.is_favorite);
        dispatch('toggleFavorite', { id: asset.id, is_favorite: isFavorite });
      }
    } catch {
      isFavorite = !isFavorite;
    }
  }

  async function handleSaveFaceName(e: CustomEvent<{ face: FaceDetail; cleanName: string }>) {
    const { face, cleanName } = e.detail;
    const matched = knownPeople.find((p) => p.name?.toLowerCase() === cleanName.toLowerCase());

    if (matched && matched.id !== face.person_id) {
      const res = await fetch(`/api/faces/${face.face_id}/reassign`, {
        method: 'POST',
        headers: { 'Content-Type': 'application/json' },
        body: JSON.stringify({ target_person_id: matched.id })
      });
      if (res.ok) {
        face.person_id = matched.id;
        face.person_name = matched.name;
        faces = [...faces];
      }
    } else if (face.person_id) {
      const res = await fetch(`/api/persons/${face.person_id}/name`, {
        method: 'POST',
        headers: { 'Content-Type': 'application/json' },
        body: JSON.stringify({ name: cleanName })
      });
      if (res.ok) {
        face.person_name = cleanName;
        faces = [...faces];
        if (!knownPeople.some((p) => p.id === face.person_id)) {
          knownPeople = [...knownPeople, { id: face.person_id, name: cleanName }];
        }
      }
    }
  }

  function handleKeydown(e: KeyboardEvent) {
    if ((e.target as HTMLElement)?.tagName === 'INPUT') return;
    const key = e.key.toLowerCase();
    if (key === 'escape') {
      dispatch('close');
    } else if (key === 'arrowleft' && hasPrev) {
      dispatch('prev');
    } else if (key === 'arrowright' && hasNext) {
      dispatch('next');
    } else if (key === 'f') {
      toggleFavorite();
    } else if (key === '0') {
      gestures.resetZoom();
    }
  }

  $: heroStyle = (() => {
    const scale = gestures.getScale();
    const translateX = gestures.getTranslateX();
    const translateY = gestures.getTranslateY();
    const dismissOffsetY = gestures.getDismissOffsetY();
    const dismissProgress = gestures.getDismissProgress();

    if (isMorphing && initialRect) {
      const scaleX = initialRect.width / window.innerWidth;
      const scaleY = initialRect.height / window.innerHeight;
      const x = initialRect.left + initialRect.width / 2 - window.innerWidth / 2;
      const y = initialRect.top + initialRect.height / 2 - window.innerHeight / 2;
      return `transform: translate3d(${x}px, ${y}px, 0) scale(${Math.max(scaleX, scaleY)}); border-radius: 16px;`;
    }
    if (dismissOffsetY !== 0) {
      const scaleDown = 1 - dismissProgress * 0.28;
      return `transform: translate3d(0, ${dismissOffsetY}px, 0) scale(${scaleDown}); border-radius: ${dismissProgress * 24}px;`;
    }
    return `transform: translate3d(${translateX}px, ${translateY}px, 0) scale(${scale}); border-radius: 0px;`;
  })();
</script>

<svelte:window on:keydown={handleKeydown} />

{#if asset}
  <div
    class="fixed inset-0 z-50 flex flex-col md:flex-row select-none overflow-hidden backdrop-blur-2xl transition-colors duration-300 bg-[var(--bg-primary)]/90 text-[var(--text-main)]"
    style="opacity: {Math.max(0.15, 1 - gestures.getDismissProgress() * 0.85)};"
  >
    <!-- Top Action Bar -->
    <div
      class="absolute top-0 inset-x-0 z-30 flex items-center justify-between px-4 py-3 pt-[max(0.75rem,var(--sat))] bg-gradient-to-b from-[var(--bg-primary)] via-[var(--bg-primary)]/40 to-transparent pointer-events-none transition-opacity duration-150"
      style="opacity: {1 - gestures.getDismissProgress() * 1.5};"
    >
      <button
        type="button"
        on:click={() => dispatch('close')}
        class="pointer-events-auto glass-panel text-[var(--text-main)] p-2.5 rounded-full spring-tap cursor-pointer shadow-lg"
        title="Close (Esc)"
        aria-label="Close photo preview"
      >
        ✕
      </button>

      <div class="pointer-events-auto flex items-center gap-2">
        {#if gestures.getScale() > 1.05}
          <button
            type="button"
            on:click={() => gestures.resetZoom()}
            class="glass-panel text-xs px-3 py-1.5 rounded-full text-[var(--text-main)] font-mono shadow-lg cursor-pointer"
          >
            {Math.round(gestures.getScale() * 100)}%
          </button>
        {/if}

        <button
          type="button"
          on:click={toggleFavorite}
          class="glass-panel text-sm p-2.5 rounded-full spring-tap cursor-pointer shadow-lg {isFavorite ? 'text-amber-400' : 'text-[var(--text-muted)] hover:text-[var(--text-main)]'}"
          title="Toggle Favorite"
          aria-label="Toggle Favorite"
        >
          ★
        </button>

        <button
          type="button"
          on:click={() => (showMobileInfo = !showMobileInfo)}
          class="md:hidden glass-panel text-xs font-serif font-bold p-2.5 rounded-full text-[var(--text-main)] spring-tap cursor-pointer w-9 h-9 flex items-center justify-center shadow-lg {showMobileInfo ? 'text-purple-500 border-purple-500/80' : ''}"
          aria-label="Toggle photo details"
        >
          ℹ
        </button>
      </div>
    </div>

    <!-- Edge-to-Edge True Viewport Canvas -->
    <!-- svelte-ignore a11y_no_noninteractive_element_interactions -->
    <div
      bind:this={viewportEl}
      role="region"
      aria-label="Media preview viewport"
      class="flex-1 relative flex items-center justify-center overflow-hidden w-full h-full p-0 touch-none"
      on:wheel={gestures.handleWheel}
      on:touchstart={(e) => gestures.handleTouchStart(e, viewportEl)}
      on:touchmove={gestures.handleTouchMove}
      on:touchend={gestures.handleTouchEnd}
      on:dblclick={(e) => gestures.handleDoubleTap(e.clientX, e.clientY, viewportEl.getBoundingClientRect())}
    >
      <!-- Desktop & Tablet Chevron Buttons -->
      {#if hasPrev && gestures.getDismissOffsetY() === 0}
        <button
          type="button"
          on:click|stopPropagation={() => dispatch('prev')}
          class="hidden md:flex absolute left-6 z-30 text-[var(--text-main)] glass-panel p-3.5 rounded-full spring-tap cursor-pointer shadow-xl items-center justify-center"
          aria-label="Previous photo"
        >
          ‹
        </button>
      {/if}

      {#if hasNext && gestures.getDismissOffsetY() === 0}
        <button
          type="button"
          on:click|stopPropagation={() => dispatch('next')}
          class="hidden md:flex absolute right-6 z-30 text-[var(--text-main)] glass-panel p-3.5 rounded-full spring-tap cursor-pointer shadow-xl items-center justify-center"
          aria-label="Next photo"
        >
          ›
        </button>
      {/if}

      <!-- Mobile Tap Zones (Left 20% / Right 20%) -->
      {#if gestures.getScale() <= 1.05 && gestures.getDismissOffsetY() === 0}
        {#if hasPrev}
          <div
            class="md:hidden absolute inset-y-0 left-0 w-[20%] z-20 cursor-pointer"
            role="button"
            tabindex="-1"
            aria-label="Previous image"
            on:click|stopPropagation={() => dispatch('prev')}
            on:keydown={() => {}}
          ></div>
        {/if}

        {#if hasNext}
          <div
            class="md:hidden absolute inset-y-0 right-0 w-[20%] z-20 cursor-pointer"
            role="button"
            tabindex="-1"
            aria-label="Next image"
            on:click|stopPropagation={() => dispatch('next')}
            on:keydown={() => {}}
          ></div>
        {/if}
      {/if}

      <!-- Full-Screen Media Canvas Container -->
      <div
        class="w-full h-full flex items-center justify-center {isMorphing ? 'transition-all duration-300 ease-[cubic-bezier(0.32,0.72,0,1)]' : ''}"
        style={heroStyle}
      >
        {#if isMotionMedia}
          <video
            src={resolveUrl(asset.preview_path)}
            poster={resolveUrl(asset.thumb_path)}
            controls={asset.mime_type !== 'image/gif'}
            autoplay
            loop={asset.mime_type === 'image/gif'}
            muted={asset.mime_type === 'image/gif'}
            playsinline
            class="w-full h-full object-contain"
          >
            <track kind="captions" />
          </video>
        {:else}
          <div class="relative w-full h-full flex items-center justify-center">
            <!-- Layer 1: Instant Cached Thumbnail Base -->
            <img
              src={resolveUrl(asset.thumb_path)}
              alt=""
              aria-hidden="true"
              class="w-full h-full object-contain block select-none pointer-events-none"
            />

            <!-- Layer 2: Full-Resolution Stream -->
            <img
              src="/api/assets/{asset.id}/stream"
              alt={asset.file_name}
              decoding="async"
              on:load={() => (isHighResLoaded = true)}
              class="absolute inset-0 w-full h-full object-contain select-none pointer-events-none transition-opacity duration-300 ease-out {isHighResLoaded ? 'opacity-100' : 'opacity-0'}"
            />

            {#if showBoxes && gestures.getScale() <= 1.05 && gestures.getDismissOffsetY() === 0}
              {#each faces as f (f.face_id)}
                <button
                  type="button"
                  class="absolute border border-purple-400/90 bg-purple-400/15 rounded-md cursor-pointer group z-10 hover:border-purple-300 hover:bg-purple-400/25 transition-colors shadow-sm"
                  style="left: {f.bbox_x * 100}%; top: {f.bbox_y * 100}%; width: {f.bbox_w * 100}%; height: {f.bbox_h * 100}%;"
                  on:click|stopPropagation={() => (showMobileInfo = true)}
                  aria-label="View face details for {f.person_name || 'Unnamed'}"
                >
                  <span
                    class="absolute -bottom-6 left-1/2 -translate-x-1/2 glass-panel text-[10px] text-purple-400 px-2 py-0.5 rounded-full shadow-lg whitespace-nowrap opacity-90 group-hover:opacity-100 font-medium pointer-events-none"
                  >
                    {f.person_name || 'Unnamed'}
                  </span>
                </button>
              {/each}
            {/if}
          </div>
        {/if}
      </div>
    </div>

    <!-- Metadata Details Sheet -->
    <PhotoDetailsSheet
      {asset}
      {isFavorite}
      {faces}
      {tags}
      {similarItems}
      {knownPeople}
      {loadingDetails}
      {loadingSimilar}
      {showBoxes}
      {showMobileInfo}
      on:toggleFavorite={toggleFavorite}
      on:toggleBoxes={() => (showBoxes = !showBoxes)}
      on:closeMobile={() => (showMobileInfo = false)}
      on:selectAsset={(e) => dispatch('selectAsset', e.detail)}
      on:saveFaceName={handleSaveFaceName}
    />
  </div>
{/if}