export interface GestureCallbacks {
  onPrev: () => void;
  onNext: () => void;
  onClose: () => void;
  hasPrev: () => boolean;
  hasNext: () => boolean;
  isMotion: () => boolean;
}

export function createModalGestureController(callbacks: GestureCallbacks) {
  let scale = 1;
  let translateX = 0;
  let translateY = 0;
  let dismissOffsetY = 0;
  let dismissProgress = 0;
  let isPanning = false;
  let isDismissDragging = false;
  let gestureDirection: 'horizontal' | 'vertical' | null = null;

  let initialPinchDistance = 0;
  let initialPinchScale = 1;
  let lastTapTime = 0;
  let touchStartX = 0;
  let touchStartY = 0;
  let panStartX = 0;
  let panStartY = 0;

  function resetZoom() {
    scale = 1;
    translateX = 0;
    translateY = 0;
    dismissOffsetY = 0;
    dismissProgress = 0;
    isPanning = false;
    isDismissDragging = false;
    gestureDirection = null;
  }

  function handleWheel(e: WheelEvent) {
    if (callbacks.isMotion()) return;
    e.preventDefault();
    const zoomDelta = -e.deltaY * 0.003;
    const newScale = Math.min(Math.max(1, scale + zoomDelta), 4.5);
    if (newScale <= 1.02) resetZoom();
    else scale = newScale;
  }

  function handleDoubleTap(clientX: number, clientY: number, targetRect: DOMRect) {
    if (callbacks.isMotion()) return;
    if (scale > 1.05) {
      resetZoom();
    } else {
      scale = 2.5;
      translateX = (targetRect.width / 2 - (clientX - targetRect.left)) * 1.3;
      translateY = (targetRect.height / 2 - (clientY - targetRect.top)) * 1.3;
    }
  }

  function handleTouchStart(e: TouchEvent, targetEl: HTMLElement) {
    if (e.touches.length === 2) {
      initialPinchDistance = Math.hypot(
        e.touches[0].clientX - e.touches[1].clientX,
        e.touches[0].clientY - e.touches[1].clientY
      );
      initialPinchScale = scale;
      isDismissDragging = false;
      gestureDirection = null;
    } else if (e.touches.length === 1) {
      const now = Date.now();
      if (now - lastTapTime < 280) {
        handleDoubleTap(e.touches[0].clientX, e.touches[0].clientY, targetEl.getBoundingClientRect());
      }
      lastTapTime = now;

      touchStartX = e.touches[0].clientX;
      touchStartY = e.touches[0].clientY;
      panStartX = e.touches[0].clientX - translateX;
      panStartY = e.touches[0].clientY - translateY;
      isPanning = scale > 1;
      gestureDirection = null;
      isDismissDragging = false;
    }
  }

  function handleTouchMove(e: TouchEvent) {
    if (e.touches.length === 2 && initialPinchDistance > 0) {
      const dist = Math.hypot(
        e.touches[0].clientX - e.touches[1].clientX,
        e.touches[0].clientY - e.touches[1].clientY
      );
      scale = Math.min(Math.max(1, initialPinchScale * (dist / initialPinchDistance)), 4.5);
      if (scale <= 1.02) {
        translateX = 0;
        translateY = 0;
      }
    } else if (e.touches.length === 1) {
      const deltaX = e.touches[0].clientX - touchStartX;
      const deltaY = e.touches[0].clientY - touchStartY;

      if (scale > 1 && isPanning) {
        translateX = e.touches[0].clientX - panStartX;
        translateY = e.touches[0].clientY - panStartY;
      } else if (scale <= 1.05) {
        // Lock intent: vertical = rubber-band pull down, horizontal = carousel navigation
        if (!gestureDirection && (Math.abs(deltaX) > 10 || Math.abs(deltaY) > 10)) {
          gestureDirection = Math.abs(deltaX) > Math.abs(deltaY) ? 'horizontal' : 'vertical';
          if (gestureDirection === 'vertical') {
            isDismissDragging = true;
          }
        }

        if (gestureDirection === 'vertical' && isDismissDragging) {
          if (deltaY > 0) {
            dismissOffsetY = deltaY;
            dismissProgress = Math.min(deltaY / 320, 1);
          } else {
            dismissOffsetY = deltaY * 0.25;
            dismissProgress = 0;
          }
        }
      }
    }
  }

  function handleTouchEnd(e: TouchEvent) {
    if (isDismissDragging) {
      if (dismissOffsetY > 110) {
        callbacks.onClose();
        return;
      }
      dismissOffsetY = 0;
      dismissProgress = 0;
      isDismissDragging = false;
    }

    if (scale <= 1.05 && gestureDirection === 'horizontal' && e.changedTouches.length === 1) {
      const diffX = e.changedTouches[0].clientX - touchStartX;
      if (Math.abs(diffX) > 40) {
        if (diffX > 0 && callbacks.hasPrev()) {
          callbacks.onPrev();
        } else if (diffX < 0 && callbacks.hasNext()) {
          callbacks.onNext();
        }
      }
    }

    gestureDirection = null;
    isPanning = false;
  }

  return {
    getScale: () => scale,
    getTranslateX: () => translateX,
    getTranslateY: () => translateY,
    getDismissOffsetY: () => dismissOffsetY,
    getDismissProgress: () => dismissProgress,
    handleWheel,
    handleTouchStart,
    handleTouchMove,
    handleTouchEnd,
    handleDoubleTap,
    resetZoom
  };
}