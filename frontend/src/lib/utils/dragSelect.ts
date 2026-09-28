// photo-app/frontend/src/lib/utils/dragSelect.ts

export interface DragSelectOptions {
  isSelectionActive: () => boolean;
  toggle: (id: string, cardEl?: HTMLElement | null) => void;
  setTargetState: (id: string, cardEl: HTMLElement, state: boolean) => void;
}

export function createDragSelectHandler(options: DragSelectOptions) {
  let isDragging = false;
  let sweepTargetState: boolean | null = null;
  const processedThisStroke = new Set<string>();

  let longPressTimer: ReturnType<typeof setTimeout> | null = null;
  let startX = 0;
  let startY = 0;
  let activePointerId: number | null = null;
  let capturedTargetEl: HTMLElement | null = null;

  const LONG_PRESS_DELAY_MS = 320;
  const MOVE_SLOP_PX = 9;

  function triggerHaptic(duration = 12) {
    if (typeof navigator !== 'undefined' && 'vibrate' in navigator) {
      try {
        navigator.vibrate(duration);
      } catch {}
    }
  }

  function getCardAtPoint(clientX: number, clientY: number): HTMLElement | null {
    const el = document.elementFromPoint(clientX, clientY);
    if (!el) return null;
    return el.closest<HTMLElement>('[data-asset-id]');
  }

  function beginStroke(card: HTMLElement, pointerId: number, targetEl: HTMLElement) {
    const assetId = card.dataset.assetId!;
    isDragging = true;
    processedThisStroke.clear();

    const isCurrentlyPressed = card.getAttribute('aria-pressed') === 'true';
    sweepTargetState = !isCurrentlyPressed;

    options.setTargetState(assetId, card, sweepTargetState);
    processedThisStroke.add(assetId);
    triggerHaptic(18);

    try {
      targetEl.setPointerCapture(pointerId);
      capturedTargetEl = targetEl;
    } catch {}
  }

  function handlePointerDown(e: PointerEvent) {
    // Only primary button or touch contact
    if (e.button !== 0 && e.pointerType === 'mouse') return;

    const target = e.target as HTMLElement;
    // Don't intercept header buttons, tags, or links
    if (target.closest('button:not([data-select-btn]), a, input, select')) return;

    const card = target.closest<HTMLElement>('[data-asset-id]');
    if (!card) return;

    const isSelectBtn = Boolean(target.closest('[data-select-btn]'));
    const isModeActive = options.isSelectionActive();

    startX = e.clientX;
    startY = e.clientY;
    activePointerId = e.pointerId;

    if (isSelectBtn || isModeActive) {
      // Direct drag: when checkmark is tapped or selection mode is active
      beginStroke(card, e.pointerId, e.currentTarget as HTMLElement);
    } else {
      // Hold-to-select: start the timer for a long press
      longPressTimer = setTimeout(() => {
        longPressTimer = null;
        beginStroke(card, e.pointerId, e.currentTarget as HTMLElement);
      }, LONG_PRESS_DELAY_MS);
    }
  }

  function handlePointerMove(e: PointerEvent) {
    if (activePointerId !== e.pointerId) return;

    const deltaX = Math.abs(e.clientX - startX);
    const deltaY = Math.abs(e.clientY - startY);

    // If waiting for long press and the user moves, cancel so standard scrolling works
    if (longPressTimer) {
      if (deltaX > MOVE_SLOP_PX || deltaY > MOVE_SLOP_PX) {
        clearTimeout(longPressTimer);
        longPressTimer = null;
      }
      return;
    }

    // Active sweep drag
    if (!isDragging || sweepTargetState === null) return;

    // Prevent default scroll behavior while actively sweeping
    if (e.cancelable) {
      e.preventDefault();
    }

    const card = getCardAtPoint(e.clientX, e.clientY);
    if (!card) return;

    const assetId = card.dataset.assetId;
    if (!assetId || processedThisStroke.has(assetId)) return;

    processedThisStroke.add(assetId);
    options.setTargetState(assetId, card, sweepTargetState);
    triggerHaptic(5);
  }

  function finishStroke() {
    if (longPressTimer) {
      clearTimeout(longPressTimer);
      longPressTimer = null;
    }

    if (activePointerId !== null && capturedTargetEl) {
      try {
        capturedTargetEl.releasePointerCapture(activePointerId);
      } catch {}
    }

    activePointerId = null;
    capturedTargetEl = null;

    // Small timeout to allow the browser click event to check isDragging() before reset
    setTimeout(() => {
      isDragging = false;
      sweepTargetState = null;
      processedThisStroke.clear();
    }, 50);
  }

  function handlePointerUp(e: PointerEvent) {
    if (activePointerId === e.pointerId || activePointerId === null) {
      finishStroke();
    }
  }

  function handlePointerCancel() {
    finishStroke();
  }

  return {
    handlePointerDown,
    handlePointerMove,
    handlePointerUp,
    handlePointerCancel,
    isDragging: () => isDragging
  };
}