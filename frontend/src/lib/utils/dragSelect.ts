export interface DragSelectOptions {
  isSelectionActive: () => boolean;
  toggle: (id: string, cardEl?: HTMLElement | null) => void;
  setTargetState: (id: string, cardEl: HTMLElement, state: boolean) => void;
}

export function createDragSelectHandler(options: DragSelectOptions) {
  let isDragging = false;
  let sweepTargetState: boolean | null = null;
  const processedThisStroke = new Set<string>();

  function handlePointerDown(e: PointerEvent) {
    // Only engage if selection mode is already active or initiated from a select button
    const target = e.target as HTMLElement;
    const isSelectBtn = Boolean(target.closest('[data-select-btn]'));
    const selectionActive = options.isSelectionActive();

    if (!isSelectBtn && !selectionActive) return;

    const card = target.closest<HTMLElement>('[data-asset-id]');
    if (!card) return;

    const assetId = card.dataset.assetId!;
    isDragging = true;
    processedThisStroke.clear();

    // The state of the initial item sets the stroke direction (select or deselect)
    const isCurrentlyPressed = card.getAttribute('aria-pressed') === 'true';
    sweepTargetState = !isCurrentlyPressed;

    options.setTargetState(assetId, card, sweepTargetState);
    processedThisStroke.add(assetId);

    // Optional subtle haptic click
    if (typeof navigator !== 'undefined' && 'vibrate' in navigator) {
      navigator.vibrate(8);
    }

    try {
      (e.currentTarget as HTMLElement).setPointerCapture(e.pointerId);
    } catch {
      // Fallback for browsers that don't support pointer capture on containers
    }
  }

  function handlePointerMove(e: PointerEvent) {
    if (!isDragging || sweepTargetState === null) return;

    const el = document.elementFromPoint(e.clientX, e.clientY) as HTMLElement | null;
    if (!el) return;

    const card = el.closest<HTMLElement>('[data-asset-id]');
    if (!card) return;

    const assetId = card.dataset.assetId;
    if (!assetId || processedThisStroke.has(assetId)) return;

    processedThisStroke.add(assetId);
    options.setTargetState(assetId, card, sweepTargetState);

    if (typeof navigator !== 'undefined' && 'vibrate' in navigator) {
      navigator.vibrate(4);
    }
  }

  function handlePointerUp(e: PointerEvent) {
    if (!isDragging) return;
    isDragging = false;
    sweepTargetState = null;
    processedThisStroke.clear();

    try {
      (e.currentTarget as HTMLElement).releasePointerCapture(e.pointerId);
    } catch {}
  }

  function handlePointerCancel() {
    isDragging = false;
    sweepTargetState = null;
    processedThisStroke.clear();
  }

  return {
    handlePointerDown,
    handlePointerMove,
    handlePointerUp,
    handlePointerCancel,
    isDragging: () => isDragging
  };
}