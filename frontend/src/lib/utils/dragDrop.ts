export function isValidFileDrag(e: DragEvent): boolean {
  const types = e.dataTransfer?.types;
  return Boolean(
    types &&
      types.includes('Files') &&
      !types.includes('text/plain') &&
      !types.includes('application/x-face-id')
  );
}

export function createWindowFileDrop(onFilesDropped: (files: File[]) => void) {
  let dragCounter = 0;

  return {
    handleDragEnter(e: DragEvent, setDragging: (active: boolean) => void) {
      if (!isValidFileDrag(e)) return;
      e.preventDefault();
      dragCounter++;
      setDragging(true);
    },

    handleDragOver(e: DragEvent) {
      if (!isValidFileDrag(e)) return;
      e.preventDefault();
    },

    handleDragLeave(e: DragEvent, setDragging: (active: boolean) => void) {
      if (!isValidFileDrag(e)) return;
      e.preventDefault();
      dragCounter--;
      if (dragCounter <= 0) {
        dragCounter = 0;
        setDragging(false);
      }
    },

    handleDrop(e: DragEvent, setDragging: (active: boolean) => void) {
      dragCounter = 0;
      setDragging(false);
      if (!isValidFileDrag(e)) return;

      e.preventDefault();
      if (e.dataTransfer?.files?.length) {
        onFilesDropped(Array.from(e.dataTransfer.files));
      }
    }
  };
}