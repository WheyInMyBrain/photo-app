/**
 * Triggers a direct browser file download for any asset URL
 */
export async function downloadAsset(url: string, fileName: string) {
  try {
    // Fetch blob to ensure browser forces download even across cross-origin/inline MIME types
    const response = await fetch(url);
    if (!response.ok) throw new Error(`HTTP ${response.status}`);
    const blob = await response.blob();

    const blobUrl = URL.createObjectURL(blob);
    const link = document.createElement('a');
    link.href = blobUrl;
    link.download = fileName || 'download';
    document.body.appendChild(link);
    link.click();
    document.body.removeChild(link);
    URL.revokeObjectURL(blobUrl);
  } catch (err) {
    console.error('Download failed, falling back to direct link:', err);
    // Fallback: direct anchor trigger
    const link = document.createElement('a');
    link.href = url;
    link.download = fileName || 'download';
    link.target = '_blank';
    link.click();
  }
}

/**
 * Triggers the native OS share sheet (mobile share tray, macOS AirDrop/Messages, etc.)
 */
export async function shareAsset(url: string, fileName: string, mimeType: string = 'image/jpeg') {
  const shareData: ShareData = {
    title: fileName,
  };

  // Try sharing the actual binary file first (Native OS photo tray)
  if (navigator.canShare && typeof navigator.share === 'function') {
    try {
      const response = await fetch(url);
      const blob = await response.blob();
      const file = new File([blob], fileName, { type: blob.type || mimeType });

      if (navigator.canShare({ files: [file] })) {
        await navigator.share({
          files: [file],
          title: fileName,
        });
        return;
      }
    } catch (err: any) {
      // User cancelled share or file sharing not supported
      if (err.name === 'AbortError') return;
      console.warn('Native file share failed, falling back to URL share:', err);
    }

    // Fallback 1: Share as URL
    try {
      await navigator.share({
        title: fileName,
        url: window.location.origin + (url.startsWith('/') ? url : `/${url}`)
      });
      return;
    } catch (err: any) {
      if (err.name === 'AbortError') return;
    }
  }

  // Fallback 2: Desktop browsers lacking Web Share API -> Copy link to clipboard
  try {
    await navigator.clipboard.writeText(window.location.origin + (url.startsWith('/') ? url : `/${url}`));
    alert('Link copied to clipboard!');
  } catch (err) {
    console.error('Clipboard copy failed:', err);
  }
}