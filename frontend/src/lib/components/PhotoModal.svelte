<!-- photo-app/frontend/src/lib/components/PhotoModal.svelte -->
<script lang="ts">
  import { onMount, onDestroy, createEventDispatcher } from 'svelte';
  import { deleteFace } from '$lib/api/people';
  import {
    fetchAssetPoses,
    fetchAssetTags,
    fetchSimilarAssets,
    toggleAssetFavorite,
    type AssetPoseDetail,
    type SimilarMediaItem
  } from '$lib/api/assets';
  import type { FaceDetail, TagItem, PersonCandidate } from '$lib/types/modal';
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
    aspect_ratio?: number | null;
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
    setAsCover: { id: string; thumb_path: string };
  }>();

  let isMorphing = Boolean(initialRect);
  let isHighResLoaded = false;
  let viewportEl: HTMLDivElement;

  // Viewport & Image Sizing
  let containerW = 0;
  let containerH = 0;
  let naturalW = 0;
  let naturalH = 0;

  // Layer Toggles
  let showFaces = false;
  let showPoses = false;

  // Sidebar & Overlay Data
  let showMobileInfo = false;
  let isFavorite = false;
  let faces: FaceDetail[] = [];
  let poses: AssetPoseDetail[] = [];
  let tags: TagItem[] = [];
  let similarItems: SimilarMediaItem[] = [];
  let knownPeople: PersonCandidate[] = [];
  let loadingDetails = true;
  let loadingSimilar = true;
  let detailAbortCtrl: AbortController | null = null;

  // =========================================================================
  // Native-Feeling Kinetic Gesture & Swipe Engine
  // =========================================================================
  let scale = 1.0;
  let translateX = 0;
  let translateY = 0;
  let isDragging = false;
  let dragStartX = 0;
  let dragStartY = 0;
  let dragStartTime = 0;
  let startTranslateX = 0;
  let startTranslateY = 0;

  // Gesture tracking modes: 'none' | 'pan' | 'dismiss' | 'swipe'
  let gestureMode: 'none' | 'pan' | 'dismiss' | 'swipe' = 'none';
  let horizontalSwipeOffset = 0;

  // Dismiss Swipe Tracking (Swipe Down to Exit)
  let dismissOffsetY = 0;
  let dismissProgress = 0;

  const MIN_SCALE = 1.0;
  const MAX_SCALE = 5.0;

  function resetZoom(_animated = true) {
    scale = 1.0;
    translateX = 0;
    translateY = 0;
    isDragging = false;
    gestureMode = 'none';
    horizontalSwipeOffset = 0;
    dismissOffsetY = 0;
    dismissProgress = 0;
  }

  function handleWheel(e: WheelEvent) {
    if (isMotionMedia) return;
    e.preventDefault();

    const rect = viewportEl.getBoundingClientRect();
    const mouseX = e.clientX - rect.left - rect.width / 2;
    const mouseY = e.clientY - rect.top - rect.height / 2;

    const zoomFactor = e.deltaY < 0 ? 1.15 : 0.87;
    const newScale = Math.min(MAX_SCALE, Math.max(MIN_SCALE, scale * zoomFactor));

    if (newScale === MIN_SCALE) {
      resetZoom();
      return;
    }

    const ratio = newScale / scale;
    translateX = mouseX - (mouseX - translateX) * ratio;
    translateY = mouseY - (mouseY - translateY) * ratio;
    scale = newScale;
    clampPan();
  }

  function handleDoubleTap(clientX: number, clientY: number) {
    if (scale > 1.05) {
      resetZoom();
    } else {
      const rect = viewportEl.getBoundingClientRect();
      const mouseX = clientX - rect.left - rect.width / 2;
      const mouseY = clientY - rect.top - rect.height / 2;
      scale = 2.5;
      translateX = -mouseX * 1.5;
      translateY = -mouseY * 1.5;
      clampPan();
    }
  }

  function onPointerDown(e: PointerEvent) {
    if (e.button !== 0) return;
    isDragging = true;
    dragStartTime = Date.now();
    dragStartX = e.clientX;
    dragStartY = e.clientY;
    startTranslateX = translateX;
    startTranslateY = translateY;
    horizontalSwipeOffset = 0;
    dismissOffsetY = 0;
    dismissProgress = 0;
    gestureMode = scale > 1.05 ? 'pan' : 'none';

    (e.currentTarget as HTMLElement)?.setPointerCapture(e.pointerId);
  }

  function onPointerMove(e: PointerEvent) {
    if (!isDragging) return;
    const deltaX = e.clientX - dragStartX;
    const deltaY = e.clientY - dragStartY;

    if (scale > 1.05) {
      // Freeform pan when zoomed in
      translateX = startTranslateX + deltaX;
      translateY = startTranslateY + deltaY;
      clampPan();
    } else {
      // 1. Directional locking threshold (10px)
      if (gestureMode === 'none') {
        if (Math.abs(deltaY) > Math.abs(deltaX) && Math.abs(deltaY) > 10) {
          if (deltaY > 0) gestureMode = 'dismiss';
        } else if (Math.abs(deltaX) > Math.abs(deltaY) && Math.abs(deltaX) > 10) {
          gestureMode = 'swipe';
        }
      }

      // 2. Continuous gesture execution
      if (gestureMode === 'dismiss') {
        dismissOffsetY = Math.max(0, deltaY);
        dismissProgress = Math.min(1.0, dismissOffsetY / 380);
        horizontalSwipeOffset = 0;
      } else if (gestureMode === 'swipe') {
        dismissOffsetY = 0;
        dismissProgress = 0;

        // Fluid rubber-banding if swiping beyond boundaries
        if ((deltaX > 0 && !hasPrev) || (deltaX < 0 && !hasNext)) {
          horizontalSwipeOffset = deltaX * 0.22;
        } else {
          horizontalSwipeOffset = deltaX;
        }
      }
    }
  }

  function onPointerUp(e: PointerEvent) {
    if (!isDragging) return;
    isDragging = false;

    const deltaX = e.clientX - dragStartX;
    const deltaY = e.clientY - dragStartY;
    const duration = Math.max(1, Date.now() - dragStartTime);
    const velocityX = Math.abs(deltaX) / duration; // px per ms

    // A. Clean tap detection (< 10px drift within 250ms)
    if (Math.abs(deltaX) < 10 && Math.abs(deltaY) < 10 && duration < 250) {
      dismissOffsetY = 0;
      dismissProgress = 0;
      horizontalSwipeOffset = 0;
      gestureMode = 'none';

      const screenW = window.innerWidth;
      // Tap left 20% of screen -> Prev
      if (e.clientX < screenW * 0.20 && hasPrev) {
        dispatch('prev');
        return;
      }
      // Tap right 20% of screen -> Next
      else if (e.clientX > screenW * 0.80 && hasNext) {
        dispatch('next');
        return;
      }
      return;
    }

    // B. Dismiss swipe threshold (> 120px)
    if (gestureMode === 'dismiss' && dismissOffsetY > 120) {
      dispatch('close');
      return;
    }

    // C. Natural swipe threshold: distance > 60px OR fast flick (velocity > 0.45 px/ms)
    if (gestureMode === 'swipe' || Math.abs(deltaX) > 60) {
      if ((deltaX > 60 || (deltaX > 25 && velocityX > 0.45)) && hasPrev) {
        dispatch('prev');
      } else if ((deltaX < -60 || (deltaX < -25 && velocityX > 0.45)) && hasNext) {
        dispatch('next');
      }
    }

    // Reset offsets smoothly
    dismissOffsetY = 0;
    dismissProgress = 0;
    horizontalSwipeOffset = 0;
    gestureMode = 'none';
  }

  function clampPan() {
    if (!containerW || !containerH) return;
    const maxTx = (containerW * (scale - 1)) / 2;
    const maxTy = (containerH * (scale - 1)) / 2;
    translateX = Math.max(-maxTx, Math.min(maxTx, translateX));
    translateY = Math.max(-maxTy, Math.min(maxTy, translateY));
  }

  function zoomToFace(f: FaceDetail) {
    if (!imageStage.ready) return;
    scale = 2.8;
    const faceCenterX = (f.bbox_x + f.bbox_w / 2) * imageStage.width;
    const faceCenterY = (f.bbox_y + f.bbox_h / 2) * imageStage.height;

    const stageCenterX = imageStage.width / 2;
    const stageCenterY = imageStage.height / 2;

    translateX = (stageCenterX - faceCenterX) * 2.8;
    translateY = (stageCenterY - faceCenterY) * 2.8;
    clampPan();
  }

  const KEYPOINT_COLORS: Record<number, string> = {
    0: '#f59e0b', 1: '#fbbf24', 2: '#fbbf24', 3: '#fde68a', 4: '#fde68a',
    5: '#06b6d4', 6: '#3b82f6', 7: '#22d3ee', 8: '#60a5fa', 9: '#67e8f9',
    10: '#93c5fd', 11: '#f43f5e', 12: '#a855f7', 13: '#fb7185', 14: '#c084fc',
    15: '#fda4af', 16: '#e9d5ff'
  };

  const SKELETON_BONES: Array<[number, number, string]> = [
    [0, 1, '#fbbf24'], [0, 2, '#fbbf24'], [1, 3, '#f59e0b'], [2, 4, '#f59e0b'], [1, 2, '#fcd34d'],
    [5, 6, '#10b981'], [5, 11, '#10b981'], [6, 12, '#10b981'], [11, 12, '#059669'],
    [5, 7, '#06b6d4'], [7, 9, '#22d3ee'], [6, 8, '#3b82f6'], [8, 10, '#60a5fa'],
    [11, 13, '#f43f5e'], [13, 15, '#fb7185'], [12, 14, '#a855f7'], [14, 16, '#c084fc']
  ];

  $: isMotionMedia =
    Boolean(asset?.mime_type?.startsWith('video/')) || asset?.mime_type === 'image/gif';

  $: imageStage = (() => {
    if (!containerW || !containerH) {
      return { width: 0, height: 0, left: 0, top: 0, ready: false };
    }

    const ratio = naturalW > 0 && naturalH > 0
      ? naturalW / naturalH
      : (asset?.aspect_ratio && asset.aspect_ratio > 0 ? asset.aspect_ratio : 1.0);

    const cRatio = containerW / containerH;
    let w: number;
    let h: number;

    if (cRatio > ratio) {
      h = containerH;
      w = h * ratio;
    } else {
      w = containerW;
      h = w / ratio;
    }

    return {
      width: Math.round(w),
      height: Math.round(h),
      left: Math.round((containerW - w) / 2),
      top: Math.round((containerH - h) / 2),
      ready: true
    };
  })();

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
    naturalW = 0;
    naturalH = 0;
    resetZoom(false);
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

    fetch('/api/persons/names', { credentials: 'include' })
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
      const [fRes, pData, tData, sData] = await Promise.all([
        fetch(`/api/assets/${id}/faces`, { signal: detailAbortCtrl.signal, credentials: 'include' }),
        fetchAssetPoses(id, detailAbortCtrl.signal),
        fetchAssetTags(id, detailAbortCtrl.signal),
        fetchSimilarAssets(id, detailAbortCtrl.signal)
      ]);

      faces = fRes.ok ? await fRes.json() : [];
      poses = pData;
      tags = tData;
      similarItems = sData;
    } catch {
      faces = [];
      poses = [];
      tags = [];
      similarItems = [];
    } finally {
      loadingDetails = false;
      loadingSimilar = false;
    }
  }

  async function handleToggleFavorite() {
    if (!asset) return;
    const prev = isFavorite;
    isFavorite = !isFavorite;
    try {
      const data = await toggleAssetFavorite(asset.id);
      isFavorite = Boolean(data.is_favorite);
      dispatch('toggleFavorite', { id: asset.id, is_favorite: isFavorite });
    } catch {
      isFavorite = prev;
    }
  }

  async function handleSaveFaceName(e: CustomEvent<{ face: FaceDetail; cleanName: string }>) {
    const { face, cleanName } = e.detail;
    const matched = knownPeople.find((p) => p.name?.toLowerCase() === cleanName.toLowerCase());

    if (matched && matched.id !== face.person_id) {
      const res = await fetch(`/api/faces/${face.face_id}/reassign`, {
        method: 'POST',
        headers: { 'Content-Type': 'application/json' },
        credentials: 'include',
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
        credentials: 'include',
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

  async function handleDeleteFace(e: CustomEvent<{ faceId: string }>) {
    const { faceId } = e.detail;
    try {
      await deleteFace(faceId);
      faces = faces.filter((f) => f.face_id !== faceId);
    } catch (err) {
      console.error('Failed to delete face:', err);
    }
  }

  function handleKeydown(e: KeyboardEvent) {
    if ((e.target as HTMLElement)?.tagName === 'INPUT') return;
    const key = e.key.toLowerCase();
    if (key === 'escape') {
      if (scale > 1.05) resetZoom();
      else dispatch('close');
    } else if (key === 'arrowleft' && hasPrev && scale <= 1.05) {
      dispatch('prev');
    } else if (key === 'arrowright' && hasNext && scale <= 1.05) {
      dispatch('next');
    } else if (key === 'f') {
      handleToggleFavorite();
    } else if (key === '0') {
      resetZoom();
    } else if (key === '+' || key === '=') {
      scale = Math.min(MAX_SCALE, scale * 1.25);
      clampPan();
    } else if (key === '-' || key === '_') {
      scale = Math.max(MIN_SCALE, scale / 1.25);
      if (scale === MIN_SCALE) resetZoom();
      else clampPan();
    }
  }

  function handleThumbLoad(e: Event) {
    const img = e.currentTarget as HTMLImageElement;
    if (!naturalW) {
      naturalW = img.naturalWidth;
      naturalH = img.naturalHeight;
    }
  }

  function handleHighResLoad(e: Event) {
    const img = e.currentTarget as HTMLImageElement;
    isHighResLoaded = true;
    naturalW = img.naturalWidth;
    naturalH = img.naturalHeight;
  }

  $: heroStyle = (() => {
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
    if (scale <= 1.05 && horizontalSwipeOffset !== 0) {
      // Fluid swipe translation with subtle tilt/scale for native momentum feel
      const tilt = (horizontalSwipeOffset / window.innerWidth) * 4;
      return `transform: translate3d(${horizontalSwipeOffset}px, 0, 0) rotate(${tilt}deg);`;
    }
    return `transform: translate3d(${translateX}px, ${translateY}px, 0) scale(${scale}); cursor: ${scale > 1.05 ? (isDragging ? 'grabbing' : 'grab') : 'default'};`;
  })();
</script>

<svelte:window on:keydown={handleKeydown} />

{#if asset}
  <div
    class="fixed inset-0 z-50 flex flex-col md:flex-row select-none overflow-hidden backdrop-blur-2xl transition-colors duration-300 bg-[var(--bg-primary)]/95 text-[var(--text-main)]"
    style="opacity: {Math.max(0.15, 1 - dismissProgress * 0.85)};"
  >
    <!-- Top Floating Navigation -->
    <div
      class="absolute top-0 inset-x-0 z-30 flex items-center justify-between px-4 py-3 pt-[max(0.75rem,var(--sat))] bg-gradient-to-b from-[var(--bg-primary)] via-[var(--bg-primary)]/40 to-transparent pointer-events-none transition-opacity duration-150"
      style="opacity: {1 - dismissProgress * 1.5};"
    >
      <button
        type="button"
        on:click={() => dispatch('close')}
        class="pointer-events-auto liquid-icon-btn w-9 h-9 rounded-full flex items-center justify-center text-[var(--text-main)] spring-tap cursor-pointer"
        title="Close (Esc)"
        aria-label="Close preview"
      >
        <svg xmlns="http://www.w3.org/2000/svg" class="w-4 h-4" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2.2">
          <line x1="18" y1="6" x2="6" y2="18"></line>
          <line x1="6" y1="6" x2="18" y2="18"></line>
        </svg>
      </button>

      <div class="pointer-events-auto flex items-center gap-2">
        {#if scale > 1.05}
          <button
            type="button"
            on:click={() => resetZoom()}
            class="liquid-btn text-xs px-3 py-1.5 rounded-full text-amber-400 font-mono cursor-pointer font-medium hover:bg-amber-500/10 transition-colors"
            title="Reset Zoom (0)"
            aria-label="Reset zoom to 100%"
          >
            {Math.round(scale * 100)}% (Reset)
          </button>
        {/if}

        <button
          type="button"
          on:click={handleToggleFavorite}
          class="liquid-icon-btn w-9 h-9 rounded-full flex items-center justify-center spring-tap cursor-pointer {isFavorite ? 'text-amber-500 liquid-fav-active' : 'text-[var(--text-muted)] hover:text-[var(--text-main)]'}"
          title="Toggle Favorite (F)"
          aria-label={isFavorite ? 'Remove from favorites' : 'Add to favorites'}
        >
          <svg xmlns="http://www.w3.org/2000/svg" class="w-4 h-4 {isFavorite ? 'fill-amber-500 stroke-amber-500' : 'fill-none stroke-current'}" viewBox="0 0 24 24" stroke-width="2">
            <polygon points="12 2 15.09 8.26 22 9.27 17 14.14 18.18 21.02 12 17.77 5.82 21.02 7 14.14 2 9.27 8.91 8.26 12 2"></polygon>
          </svg>
        </button>

        <button
          type="button"
          on:click={() => (showMobileInfo = !showMobileInfo)}
          class="md:hidden liquid-icon-btn w-9 h-9 rounded-full flex items-center justify-center text-[var(--text-main)] spring-tap cursor-pointer"
          title="Toggle Details"
          aria-label="Toggle photo details"
        >
          <svg xmlns="http://www.w3.org/2000/svg" class="w-4 h-4" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2">
            <circle cx="12" cy="12" r="10"></circle>
            <line x1="12" y1="16" x2="12" y2="12"></line>
            <line x1="12" y1="8" x2="12.01" y2="8"></line>
          </svg>
        </button>
      </div>
    </div>

    <!-- Active Viewport Canvas -->
    <!-- svelte-ignore a11y_no_noninteractive_element_interactions -->
    <div
      bind:this={viewportEl}
      role="region"
      aria-label="Media preview viewport"
      class="flex-1 relative flex items-center justify-center overflow-hidden w-full h-full p-0 touch-none"
      on:wheel|nonpassive={handleWheel}
      on:pointerdown={onPointerDown}
      on:pointermove={onPointerMove}
      on:pointerup={onPointerUp}
      on:pointercancel={onPointerUp}
      on:dblclick={(e) => handleDoubleTap(e.clientX, e.clientY)}
    >
      <!-- Edge Ambient Backlight Glow -->
      <div
        class="absolute inset-0 pointer-events-none opacity-25 filter blur-3xl scale-110 -z-10 transition-opacity duration-700"
        style="background: radial-gradient(circle, var(--accent-primary, #6366f1) 0%, transparent 70%);"
      ></div>

      <!-- Navigation Arrows (Desktop) -->
      {#if hasPrev && scale <= 1.05 && dismissOffsetY === 0}
        <button
          type="button"
          on:click|stopPropagation={() => dispatch('prev')}
          class="hidden md:flex absolute left-6 z-30 liquid-icon-btn w-11 h-11 rounded-full text-[var(--text-main)] spring-tap cursor-pointer items-center justify-center"
          title="Previous photo (Left Arrow)"
          aria-label="Previous photo"
        >
          <svg xmlns="http://www.w3.org/2000/svg" class="w-5 h-5" fill="none" viewBox="0 0 24 24" stroke="currentColor" stroke-width="2.5">
            <polyline points="15 18 9 12 15 6"></polyline>
          </svg>
        </button>
      {/if}

      {#if hasNext && scale <= 1.05 && dismissOffsetY === 0}
        <button
          type="button"
          on:click|stopPropagation={() => dispatch('next')}
          class="hidden md:flex absolute right-6 z-30 liquid-icon-btn w-11 h-11 rounded-full text-[var(--text-main)] spring-tap cursor-pointer items-center justify-center"
          title="Next photo (Right Arrow)"
          aria-label="Next photo"
        >
          <svg xmlns="http://www.w3.org/2000/svg" class="w-5 h-5" fill="none" viewBox="0 0 24 24" stroke="currentColor" stroke-width="2.5">
            <polyline points="9 18 15 12 9 6"></polyline>
          </svg>
        </button>
      {/if}

      <!-- Media Canvas -->
      <div
        class="w-full h-full flex items-center justify-center {isMorphing || (!isDragging && (scale > 1.0 || horizontalSwipeOffset === 0)) ? 'transition-transform duration-250 cubic-bezier(0.16, 1, 0.3, 1)' : ''}"
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
          <div
            class="relative w-full h-full flex items-center justify-center overflow-hidden"
            bind:clientWidth={containerW}
            bind:clientHeight={containerH}
          >
            <!-- Layer 1: Thumbnail -->
            <img
              src={resolveUrl(asset.thumb_path)}
              alt=""
              aria-hidden="true"
              on:load={handleThumbLoad}
              class="w-full h-full object-contain block select-none pointer-events-none"
            />

            <!-- Layer 2: Full-Res Stream -->
            <img
              src="/api/assets/{asset.id}/stream"
              alt={asset.file_name}
              decoding="async"
              on:load={handleHighResLoad}
              class="absolute inset-0 w-full h-full object-contain select-none pointer-events-none transition-opacity duration-300 ease-out {isHighResLoaded ? 'opacity-100' : 'opacity-0'}"
            />

            <!-- AI OVERLAYS -->
            {#if imageStage.ready && dismissOffsetY === 0}
              <div
                class="absolute pointer-events-none overflow-hidden"
                style="width: {imageStage.width}px; height: {imageStage.height}px; left: {imageStage.left}px; top: {imageStage.top}px;"
              >
                <!-- Faces with 1-Click Portrait Framing -->
                {#if showFaces}
                  {#each faces as f (f.face_id)}
                    <button
                      type="button"
                      class="absolute pointer-events-auto border border-purple-500/90 bg-purple-500/15 rounded-lg cursor-pointer group z-10 hover:border-purple-400 hover:bg-purple-500/30 transition-all shadow-sm"
                      style="left: {f.bbox_x * 100}%; top: {f.bbox_y * 100}%; width: {f.bbox_w * 100}%; height: {f.bbox_h * 100}%;"
                      on:click|stopPropagation={() => zoomToFace(f)}
                      title="Click to zoom in on {f.person_name || 'Unnamed'}"
                      aria-label="Zoom to face of {f.person_name || 'Unnamed'}"
                    >
                      <span
                        class="absolute -bottom-6 left-1/2 -translate-x-1/2 liquid-tag text-[10px] text-[var(--text-main)] px-2 py-0.5 rounded-full shadow-lg whitespace-nowrap opacity-90 group-hover:opacity-100 font-medium font-mono flex items-center gap-1"
                      >
                        <span>{f.person_name || 'Unnamed'}</span>
                      </span>
                    </button>
                  {/each}
                {/if}

                <!-- YOLO Pose Skeleton -->
                {#if showPoses && poses.length > 0}
                  <svg
                    class="absolute inset-0 w-full h-full pointer-events-none z-20"
                    viewBox="0 0 100 100"
                    preserveAspectRatio="none"
                  >
                    {#each poses as pose (pose.id)}
                      {#each SKELETON_BONES as [i, j, boneColor]}
                        {#if pose.keypoints[i] && pose.keypoints[j] && pose.keypoints[i].score > 0.35 && pose.keypoints[j].score > 0.35}
                          <line
                            x1={pose.keypoints[i].x * 100}
                            y1={pose.keypoints[i].y * 100}
                            x2={pose.keypoints[j].x * 100}
                            y2={pose.keypoints[j].y * 100}
                            stroke={boneColor}
                            stroke-width="0.5"
                            stroke-linecap="round"
                          />
                        {/if}
                      {/each}

                      {#each pose.keypoints as kp, idx}
                        {#if kp.score > 0.35}
                          <circle
                            cx={kp.x * 100}
                            cy={kp.y * 100}
                            r="0.75"
                            fill={KEYPOINT_COLORS[idx] || '#ffffff'}
                            stroke="rgba(0, 0, 0, 0.75)"
                            stroke-width="0.2"
                          />
                        {/if}
                      {/each}
                    {/each}
                  </svg>
                {/if}
              </div>
            {/if}
          </div>
        {/if}
      </div>
    </div>

    <!-- Metadata Details Sheet / Sidebar -->
    <PhotoDetailsSheet
      {asset}
      {isFavorite}
      {faces}
      {poses}
      {tags}
      {similarItems}
      {knownPeople}
      {loadingDetails}
      {loadingSimilar}
      {showFaces}
      {showPoses}
      {showMobileInfo}
      on:toggleFavorite={handleToggleFavorite}
      on:toggleFaces={() => (showFaces = !showFaces)}
      on:togglePoses={() => (showPoses = !showPoses)}
      on:closeMobile={() => (showMobileInfo = false)}
      on:selectAsset={(e) => dispatch('selectAsset', e.detail)}
      on:saveFaceName={handleSaveFaceName}
      on:deleteFace={handleDeleteFace}
      on:reassignFace={async (e) => {
        const { faceId, targetPersonId } = e.detail;
        await fetch(`/api/faces/${faceId}/reassign`, {
          method: 'POST',
          headers: { 'Content-Type': 'application/json' },
          body: JSON.stringify({ target_person_id: targetPersonId })
        });
        loadDetails(asset.id);
      }}
      on:splitFace={async (e) => {
        const { faceId } = e.detail;
        await fetch(`/api/faces/${faceId}/split-new`, { method: 'POST' });
        loadDetails(asset.id);
      }}
      on:unlinkFace={async (e) => {
        const { faceId } = e.detail;
        await fetch(`/api/faces/${faceId}/unlink`, { method: 'POST' });
        loadDetails(asset.id);
      }}
    />
  </div>
{/if}

<style>
  .liquid-btn {
    background: var(--dock-bg, rgba(255, 255, 255, 0.08));
    border: 1px solid var(--dock-border, rgba(255, 255, 255, 0.12));
    backdrop-filter: blur(20px) saturate(180%);
    -webkit-backdrop-filter: blur(20px) saturate(180%);
    box-shadow: 0 4px 14px var(--dock-shadow, rgba(0, 0, 0, 0.25)), inset 0 1px 0 var(--dock-highlight, rgba(255, 255, 255, 0.15));
    transition: all 0.2s cubic-bezier(0.16, 1, 0.3, 1);
  }

  .liquid-btn:hover {
    background: var(--dock-bg-hover, rgba(255, 255, 255, 0.14));
    border-color: var(--border-subtle, rgba(255, 255, 255, 0.22));
    transform: translateY(-1px);
  }

  .liquid-icon-btn {
    background: var(--dock-bg, rgba(255, 255, 255, 0.08));
    border: 1px solid var(--dock-border, rgba(255, 255, 255, 0.12));
    backdrop-filter: blur(20px) saturate(180%);
    -webkit-backdrop-filter: blur(20px) saturate(180%);
    box-shadow: 0 4px 14px var(--dock-shadow, rgba(0, 0, 0, 0.25)), inset 0 1px 0 var(--dock-highlight, rgba(255, 255, 255, 0.15));
    transition: all 0.2s cubic-bezier(0.16, 1, 0.3, 1);
  }

  .liquid-icon-btn:hover {
    background: var(--dock-bg-hover, rgba(255, 255, 255, 0.14));
    border-color: var(--border-subtle, rgba(255, 255, 255, 0.22));
    transform: translateY(-1px);
  }

  .liquid-fav-active {
    background: rgba(245, 158, 11, 0.18) !important;
    border-color: rgba(245, 158, 11, 0.45) !important;
    box-shadow: 0 4px 18px rgba(245, 158, 11, 0.3), inset 0 1px 0 rgba(255, 255, 255, 0.35) !important;
  }

  .liquid-tag {
    background: var(--bg-surface-elevated, rgba(17, 24, 39, 0.75));
    border: 1px solid var(--border-glass, rgba(255, 255, 255, 0.14));
    backdrop-filter: blur(16px) saturate(160%);
    -webkit-backdrop-filter: blur(16px) saturate(160%);
    box-shadow: 0 4px 14px rgba(0, 0, 0, 0.35);
  }

  .spring-tap {
    transition: transform 0.15s cubic-bezier(0.34, 1.56, 0.64, 1);
  }
  .spring-tap:active {
    transform: scale(0.92);
  }
</style>