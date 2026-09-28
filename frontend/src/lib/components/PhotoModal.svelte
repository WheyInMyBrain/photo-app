<!-- photo-app/frontend/src/lib/components/PhotoModal.svelte -->
<script lang="ts">
  import { onMount, onDestroy, createEventDispatcher } from 'svelte';
  import { filterStore } from '$lib/stores/filterStore';
  import { deleteFace } from '$lib/api/people';
  import {
    fetchAssetObjects,
    fetchAssetPoses,
    fetchAssetTags,
    fetchSimilarAssets,
    toggleAssetFavorite,
    type AssetObjectDetail,
    type AssetPoseDetail,
    type SimilarMediaItem
  } from '$lib/api/assets';
  import type { FaceDetail, TagItem, PersonCandidate } from '$lib/types/modal';
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

  // Viewport & Image Sizing (for pixel-accurate stage alignment)
  let containerW = 0;
  let containerH = 0;
  let naturalW = 0;
  let naturalH = 0;

  // Layer Toggles (controlled from sidebar)
  let showFaces = true;
  let showObjects = true;
  let showPoses = true;

  // Sidebar & Overlay Data
  let showMobileInfo = false;
  let isFavorite = false;
  let faces: FaceDetail[] = [];
  let objects: AssetObjectDetail[] = [];
  let poses: AssetPoseDetail[] = [];
  let tags: TagItem[] = [];
  let similarItems: SimilarMediaItem[] = [];
  let knownPeople: PersonCandidate[] = [];
  let loadingDetails = true;
  let loadingSimilar = true;
  let detailAbortCtrl: AbortController | null = null;

  // =========================================================================
  // COCO 17 Keypoints Anatomical Color Palette
  // =========================================================================
  // Keypoint Indices:
  // 0: Nose, 1: L-Eye, 2: R-Eye, 3: L-Ear, 4: R-Ear
  // 5: L-Shoulder, 6: R-Shoulder, 7: L-Elbow, 8: R-Elbow, 9: L-Wrist, 10: R-Wrist
  // 11: L-Hip, 12: R-Hip, 13: L-Knee, 14: R-Knee, 15: L-Ankle, 16: R-Ankle

  const KEYPOINT_COLORS: Record<number, string> = {
    0: '#f59e0b', // Nose (Amber)
    1: '#fbbf24', // Left Eye (Amber Light)
    2: '#fbbf24', // Right Eye
    3: '#fde68a', // Left Ear
    4: '#fde68a', // Right Ear
    5: '#06b6d4', // Left Shoulder (Cyan)
    6: '#3b82f6', // Right Shoulder (Blue)
    7: '#22d3ee', // Left Elbow
    8: '#60a5fa', // Right Elbow
    9: '#67e8f9', // Left Wrist
    10: '#93c5fd', // Right Wrist
    11: '#f43f5e', // Left Hip (Rose)
    12: '#a855f7', // Right Hip (Purple)
    13: '#fb7185', // Left Knee
    14: '#c084fc', // Right Knee
    15: '#fda4af', // Left Ankle
    16: '#e9d5ff'  // Right Ankle
  };

  // Structured Anatomical Bones: [start_index, end_index, stroke_color]
  const SKELETON_BONES: Array<[number, number, string]> = [
    // Head / Facial Features (Amber)
    [0, 1, '#fbbf24'],
    [0, 2, '#fbbf24'],
    [1, 3, '#f59e0b'],
    [2, 4, '#f59e0b'],
    [1, 2, '#fcd34d'],

    // Torso Frame (Emerald)
    [5, 6, '#10b981'],
    [5, 11, '#10b981'],
    [6, 12, '#10b981'],
    [11, 12, '#059669'],

    // Left Arm (Cyan / Teal)
    [5, 7, '#06b6d4'],
    [7, 9, '#22d3ee'],

    // Right Arm (Electric Blue)
    [6, 8, '#3b82f6'],
    [8, 10, '#60a5fa'],

    // Left Leg (Crimson / Rose)
    [11, 13, '#f43f5e'],
    [13, 15, '#fb7185'],

    // Right Leg (Purple / Violet)
    [12, 14, '#a855f7'],
    [14, 16, '#c084fc']
  ];

  $: isMotionMedia =
    Boolean(asset?.mime_type?.startsWith('video/')) || asset?.mime_type === 'image/gif';

  // Compute the exact sub-rect where the image is drawn within the letterboxed container
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
      // Container is wider than photo: pillarbox (black bars on left & right)
      h = containerH;
      w = h * ratio;
    } else {
      // Container is taller than photo: letterbox (black bars on top & bottom)
      w = containerW;
      h = w / ratio;
    }

    const left = (containerW - w) / 2;
    const top = (containerH - h) / 2;

    return {
      width: Math.round(w),
      height: Math.round(h),
      left: Math.round(left),
      top: Math.round(top),
      ready: true
    };
  })();

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
    naturalW = 0;
    naturalH = 0;
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
      const [fRes, oData, pData, tData, sData] = await Promise.all([
        fetch(`/api/assets/${id}/faces`, { signal: detailAbortCtrl.signal, credentials: 'include' }),
        fetchAssetObjects(id, detailAbortCtrl.signal),
        fetchAssetPoses(id, detailAbortCtrl.signal),
        fetchAssetTags(id, detailAbortCtrl.signal),
        fetchSimilarAssets(id, detailAbortCtrl.signal)
      ]);

      faces = fRes.ok ? await fRes.json() : [];
      objects = oData;
      poses = pData;
      tags = tData;
      similarItems = sData;
    } catch {
      faces = [];
      objects = [];
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
      dispatch('close');
    } else if (key === 'arrowleft' && hasPrev) {
      dispatch('prev');
    } else if (key === 'arrowright' && hasNext) {
      dispatch('next');
    } else if (key === 'f') {
      handleToggleFavorite();
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
    class="fixed inset-0 z-50 flex flex-col md:flex-row select-none overflow-hidden backdrop-blur-2xl transition-colors duration-300 bg-[var(--bg-primary)]/92 text-[var(--text-main)]"
    style="opacity: {Math.max(0.15, 1 - gestures.getDismissProgress() * 0.85)};"
  >
    <!-- Top Floating Glass Navigation Header -->
    <div
      class="absolute top-0 inset-x-0 z-30 flex items-center justify-between px-4 py-3 pt-[max(0.75rem,var(--sat))] bg-gradient-to-b from-[var(--bg-primary)] via-[var(--bg-primary)]/40 to-transparent pointer-events-none transition-opacity duration-150"
      style="opacity: {1 - gestures.getDismissProgress() * 1.5};"
    >
      <button
        type="button"
        on:click={() => dispatch('close')}
        class="pointer-events-auto liquid-icon-btn w-9 h-9 rounded-full flex items-center justify-center text-white/70 hover:text-white spring-tap cursor-pointer"
        title="Close (Esc)"
        aria-label="Close preview"
      >
        <svg xmlns="http://www.w3.org/2000/svg" class="w-4 h-4" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2.2" stroke-linecap="round" stroke-linejoin="round">
          <line x1="18" y1="6" x2="6" y2="18"></line>
          <line x1="6" y1="6" x2="18" y2="18"></line>
        </svg>
      </button>

      <div class="pointer-events-auto flex items-center gap-2">
        {#if $filterStore.album_id}
          <button
            type="button"
            on:click={() => {
              if (asset) dispatch('setAsCover', { id: asset.id, thumb_path: asset.thumb_path });
            }}
            class="liquid-btn text-xs px-3 py-1.5 rounded-full text-white/80 hover:text-white transition-all spring-tap cursor-pointer"
            title="Make this photo the album cover"
          >
            Set as Cover
          </button>
        {/if}

        {#if gestures.getScale() > 1.05}
          <button
            type="button"
            on:click={() => gestures.resetZoom()}
            class="liquid-btn text-xs px-3 py-1.5 rounded-full text-white/80 font-mono cursor-pointer"
          >
            {Math.round(gestures.getScale() * 100)}%
          </button>
        {/if}

        <button
          type="button"
          on:click={handleToggleFavorite}
          class="liquid-icon-btn w-9 h-9 rounded-full flex items-center justify-center spring-tap cursor-pointer {isFavorite ? 'text-amber-300 liquid-fav-active' : 'text-white/60 hover:text-white'}"
          title="Toggle Favorite"
          aria-label="Toggle Favorite"
        >
          <svg xmlns="http://www.w3.org/2000/svg" class="w-4 h-4 {isFavorite ? 'fill-amber-300 stroke-amber-300' : 'fill-none stroke-current'}" viewBox="0 0 24 24" stroke-width="2" stroke-linecap="round" stroke-linejoin="round">
            <polygon points="12 2 15.09 8.26 22 9.27 17 14.14 18.18 21.02 12 17.77 5.82 21.02 7 14.14 2 9.27 8.91 8.26 12 2"></polygon>
          </svg>
        </button>

        <button
          type="button"
          on:click={() => (showMobileInfo = !showMobileInfo)}
          class="md:hidden liquid-icon-btn w-9 h-9 rounded-full flex items-center justify-center text-white/80 spring-tap cursor-pointer {showMobileInfo ? 'border-white/30 bg-white/15' : ''}"
          aria-label="Toggle photo details"
        >
          <svg xmlns="http://www.w3.org/2000/svg" class="w-4 h-4" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2" stroke-linecap="round" stroke-linejoin="round">
            <circle cx="12" cy="12" r="10"></circle>
            <line x1="12" y1="16" x2="12" y2="12"></line>
            <line x1="12" y1="8" x2="12.01" y2="8"></line>
          </svg>
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
      <!-- Navigation Chevrons -->
      {#if hasPrev && gestures.getDismissOffsetY() === 0}
        <button
          type="button"
          on:click|stopPropagation={() => dispatch('prev')}
          class="hidden md:flex absolute left-6 z-30 liquid-icon-btn w-11 h-11 rounded-full text-white/80 hover:text-white spring-tap cursor-pointer items-center justify-center"
          aria-label="Previous photo"
        >
          <svg xmlns="http://www.w3.org/2000/svg" class="w-5 h-5" fill="none" viewBox="0 0 24 24" stroke="currentColor" stroke-width="2.5">
            <polyline points="15 18 9 12 15 6"></polyline>
          </svg>
        </button>
      {/if}

      {#if hasNext && gestures.getDismissOffsetY() === 0}
        <button
          type="button"
          on:click|stopPropagation={() => dispatch('next')}
          class="hidden md:flex absolute right-6 z-30 liquid-icon-btn w-11 h-11 rounded-full text-white/80 hover:text-white spring-tap cursor-pointer items-center justify-center"
          aria-label="Next photo"
        >
          <svg xmlns="http://www.w3.org/2000/svg" class="w-5 h-5" fill="none" viewBox="0 0 24 24" stroke="currentColor" stroke-width="2.5">
            <polyline points="9 18 15 12 9 6"></polyline>
          </svg>
        </button>
      {/if}

      <!-- Mobile Tap Zones -->
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

      <!-- Media Canvas Container -->
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
          <!-- Letterbox Canvas Area with Client Rect Observer -->
          <div
            class="relative w-full h-full flex items-center justify-center overflow-hidden"
            bind:clientWidth={containerW}
            bind:clientHeight={containerH}
          >
            <!-- Layer 1: Instant Cached Base -->
            <img
              src={resolveUrl(asset.thumb_path)}
              alt=""
              aria-hidden="true"
              on:load={(e) => {
                if (!naturalW) {
                  naturalW = e.currentTarget.naturalWidth;
                  naturalH = e.currentTarget.naturalHeight;
                }
              }}
              class="w-full h-full object-contain block select-none pointer-events-none"
            />

            <!-- Layer 2: Full-Resolution Stream -->
            <img
              src="/api/assets/{asset.id}/stream"
              alt={asset.file_name}
              decoding="async"
              on:load={(e) => {
                isHighResLoaded = true;
                naturalW = e.currentTarget.naturalWidth;
                naturalH = e.currentTarget.naturalHeight;
              }}
              class="absolute inset-0 w-full h-full object-contain select-none pointer-events-none transition-opacity duration-300 ease-out {isHighResLoaded ? 'opacity-100' : 'opacity-0'}"
            />

            <!-- ================================================================= -->
            <!-- AI OVERLAYS: Strictly Locked to the Rendered Photo Rect           -->
            <!-- ================================================================= -->
            {#if imageStage.ready && gestures.getScale() <= 1.05 && gestures.getDismissOffsetY() === 0}
              <div
                class="absolute pointer-events-none overflow-hidden"
                style="width: {imageStage.width}px; height: {imageStage.height}px; left: {imageStage.left}px; top: {imageStage.top}px;"
              >
                <!-- 1. Detected Faces -->
                {#if showFaces}
                  {#each faces as f (f.face_id)}
                    <button
                      type="button"
                      class="absolute pointer-events-auto border border-purple-400/90 bg-purple-500/15 rounded-lg cursor-pointer group z-10 hover:border-purple-300 hover:bg-purple-500/25 transition-all shadow-sm"
                      style="left: {f.bbox_x * 100}%; top: {f.bbox_y * 100}%; width: {f.bbox_w * 100}%; height: {f.bbox_h * 100}%;"
                      on:click|stopPropagation={() => (showMobileInfo = true)}
                      aria-label="View face details for {f.person_name || 'Unnamed'}"
                    >
                      <span
                        class="absolute -bottom-6 left-1/2 -translate-x-1/2 liquid-tag text-[10px] text-purple-200 px-2 py-0.5 rounded-full shadow-lg whitespace-nowrap opacity-90 group-hover:opacity-100 font-medium pointer-events-none font-mono"
                      >
                        👤 {f.person_name || 'Unnamed'}
                      </span>
                    </button>
                  {/each}
                {/if}

                <!-- 2. YOLO Detected Objects (Segmentation Boxes) -->
                {#if showObjects}
                  {#each objects as obj (obj.id)}
                    <div
                      class="absolute border border-cyan-400/80 bg-cyan-500/15 rounded-lg pointer-events-none z-10 shadow-sm"
                      style="left: {obj.bbox_x * 100}%; top: {obj.bbox_y * 100}%; width: {obj.bbox_w * 100}%; height: {obj.bbox_h * 100}%;"
                    >
                      <span
                        class="absolute -top-5 left-0 liquid-tag text-[9px] text-cyan-200 px-1.5 py-0.5 rounded-md shadow-md whitespace-nowrap font-mono tracking-tight"
                      >
                        {obj.label} {Math.round(obj.score * 100)}%
                      </span>
                    </div>
                  {/each}
                {/if}

                <!-- 3. YOLO Human Pose Estimation (Anatomically Colored Bones & Joints) -->
                {#if showPoses && poses.length > 0}
                  <svg
                    class="absolute inset-0 w-full h-full pointer-events-none z-20"
                    viewBox="0 0 100 100"
                    preserveAspectRatio="none"
                  >
                    <defs>
                      <filter id="bone-glow" x="-20%" y="-20%" width="140%" height="140%">
                        <feDropShadow dx="0" dy="0" stdDeviation="0.25" flood-color="#000" flood-opacity="0.8" />
                      </filter>
                    </defs>

                    {#each poses as pose (pose.id)}
                      <!-- Anatomically Grouped Bone Connections -->
                      {#each SKELETON_BONES as [i, j, boneColor]}
                        {#if pose.keypoints[i] && pose.keypoints[j] && pose.keypoints[i].score > 0.35 && pose.keypoints[j].score > 0.35}
                          <!-- Dark drop-shadow bone backing for high contrast on any background -->
                          <line
                            x1={pose.keypoints[i].x * 100}
                            y1={pose.keypoints[i].y * 100}
                            x2={pose.keypoints[j].x * 100}
                            y2={pose.keypoints[j].y * 100}
                            stroke="rgba(0, 0, 0, 0.65)"
                            stroke-width="0.8"
                            stroke-linecap="round"
                          />
                          <!-- Foreground Color Bone -->
                          <line
                            x1={pose.keypoints[i].x * 100}
                            y1={pose.keypoints[i].y * 100}
                            x2={pose.keypoints[j].x * 100}
                            y2={pose.keypoints[j].y * 100}
                            stroke={boneColor}
                            stroke-width="0.45"
                            stroke-linecap="round"
                            filter="url(#bone-glow)"
                          />
                        {/if}
                      {/each}

                      <!-- Anatomically Grouped Joint Keypoints -->
                      {#each pose.keypoints as kp, idx}
                        {#if kp.score > 0.35}
                          <!-- Joint Outer Ring -->
                          <circle
                            cx={kp.x * 100}
                            cy={kp.y * 100}
                            r="0.75"
                            fill={KEYPOINT_COLORS[idx] || '#ffffff'}
                            stroke="rgba(0, 0, 0, 0.75)"
                            stroke-width="0.18"
                          />
                          <!-- Joint Core Highlight -->
                          <circle
                            cx={kp.x * 100}
                            cy={kp.y * 100}
                            r="0.25"
                            fill="#ffffff"
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
      {objects}
      {poses}
      {tags}
      {similarItems}
      {knownPeople}
      {loadingDetails}
      {loadingSimilar}
      {showFaces}
      {showObjects}
      {showPoses}
      {showMobileInfo}
      on:toggleFavorite={handleToggleFavorite}
      on:toggleFaces={() => (showFaces = !showFaces)}
      on:toggleObjects={() => (showObjects = !showObjects)}
      on:togglePoses={() => (showPoses = !showPoses)}
      on:closeMobile={() => (showMobileInfo = false)}
      on:selectAsset={(e) => dispatch('selectAsset', e.detail)}
      on:saveFaceName={handleSaveFaceName}
      on:deleteFace={handleDeleteFace}
    />
  </div>
{/if}

<style>
  .liquid-btn {
    background: rgba(255, 255, 255, 0.08);
    border: 1px solid rgba(255, 255, 255, 0.14);
    box-shadow: 0 4px 14px rgba(0, 0, 0, 0.3), inset 0 1px 0 rgba(255, 255, 255, 0.25);
  }

  .liquid-btn:hover {
    background: rgba(255, 255, 255, 0.14);
    border-color: rgba(255, 255, 255, 0.24);
  }

  .liquid-icon-btn {
    background: rgba(20, 20, 24, 0.6);
    border: 1px solid rgba(255, 255, 255, 0.14);
    backdrop-filter: blur(20px) saturate(180%);
    box-shadow: 0 4px 14px rgba(0, 0, 0, 0.35), inset 0 1px 0 rgba(255, 255, 255, 0.25);
  }

  .liquid-icon-btn:hover {
    background: rgba(255, 255, 255, 0.12);
    border-color: rgba(255, 255, 255, 0.25);
  }

  .liquid-fav-active {
    background: rgba(245, 158, 11, 0.15);
    border-color: rgba(245, 158, 11, 0.35);
    box-shadow: 0 4px 14px rgba(245, 158, 11, 0.2), inset 0 1px 0 rgba(255, 255, 255, 0.3);
  }

  .liquid-tag {
    background: rgba(15, 15, 20, 0.88);
    border: 1px solid rgba(255, 255, 255, 0.18);
    backdrop-filter: blur(14px);
  }
</style>