export function createPinchZoomHandler(
  zoomIn: () => void,
  zoomOut: () => void,
  canZoom: () => boolean
) {
  let initialPinchDist = 0;
  let pinchThrottle = false;

  return {
    handleTouchStart(e: TouchEvent) {
      if (!canZoom()) return;
      if (e.touches.length === 2) {
        initialPinchDist = Math.hypot(
          e.touches[0].clientX - e.touches[1].clientX,
          e.touches[0].clientY - e.touches[1].clientY
        );
      }
    },

    handleTouchMove(e: TouchEvent) {
      if (!canZoom() || e.touches.length !== 2 || pinchThrottle || initialPinchDist === 0) return;

      const currentDist = Math.hypot(
        e.touches[0].clientX - e.touches[1].clientX,
        e.touches[0].clientY - e.touches[1].clientY
      );
      const diff = currentDist - initialPinchDist;

      if (diff > 55) {
        zoomIn();
        pinchThrottle = true;
        setTimeout(() => (pinchThrottle = false), 350);
        initialPinchDist = currentDist;
      } else if (diff < -55) {
        zoomOut();
        pinchThrottle = true;
        setTimeout(() => (pinchThrottle = false), 350);
        initialPinchDist = currentDist;
      }
    },

    handleTouchEnd(e: TouchEvent) {
      if (e.touches.length < 2) {
        initialPinchDist = 0;
      }
    },

    handleWheel(e: WheelEvent) {
      if (!canZoom() || !e.ctrlKey || pinchThrottle) return;
      e.preventDefault();

      if (e.deltaY < -15) {
        zoomIn();
        pinchThrottle = true;
        setTimeout(() => (pinchThrottle = false), 320);
      } else if (e.deltaY > 15) {
        zoomOut();
        pinchThrottle = true;
        setTimeout(() => (pinchThrottle = false), 320);
      }
    }
  };
}