// photo-app/frontend/src/lib/stores/uploadProgressStore.ts
import { writable } from 'svelte/store';
import { authStore } from '$lib/stores/authStore';
import { CHUNK_THRESHOLD_BYTES, uploadChunked, checkDuplicate } from '$lib/utils/uploader';
import { computeFileSHA256 } from '$lib/utils/hasher';

export interface ActiveFileProgress {
  fileIdx: number;
  name: string;
  size: number;
  loaded: number;
  percent: number;
  status: 'hashing' | 'uploading' | 'chunking' | 'completed' | 'failed';
}

export interface UploadProgressState {
  isOpen: boolean;
  isMinimized: boolean;
  totalFiles: number;
  completedFiles: number;
  failedFiles: number;
  totalBytes: number;
  loadedBytes: number;
  startTime: number;
  activeUploads: ActiveFileProgress[];
  error: string | null;
}

const initialState: UploadProgressState = {
  isOpen: false,
  isMinimized: false,
  totalFiles: 0,
  completedFiles: 0,
  failedFiles: 0,
  totalBytes: 0,
  loadedBytes: 0,
  startTime: 0,
  activeUploads: [],
  error: null
};

interface IndexedFile {
  file: File;
  fileIdx: number;
  isLarge: boolean;
}

function createUploadProgressStore() {
  const { subscribe, update, set } = writable<UploadProgressState>(initialState);

  return {
    subscribe,
    toggleMinimize: () => update((s) => ({ ...s, isMinimized: !s.isMinimized })),
    close: () => set(initialState),

    startBatch: async (
      files: File[],
      folderPath: string,
      onDone?: (summary: { completed: number; failed: number }) => void
    ) => {
      const totalFiles = files.length;
      if (totalFiles === 0) {
        onDone?.({ completed: 0, failed: 0 });
        return;
      }

      const totalBytes = files.reduce((acc, f) => acc + f.size, 0);

      set({
        isOpen: true,
        isMinimized: false,
        totalFiles,
        completedFiles: 0,
        failedFiles: 0,
        totalBytes,
        loadedBytes: 0,
        startTime: Date.now(),
        activeUploads: [],
        error: null
      });

      let completedCount = 0;
      let failedCount = 0;

      // Track per-file byte progress for accurate aggregated progress
      const fileBytesMap = new Map<number, number>();

      function recalculateLoadedBytes(): number {
        let sum = 0;
        for (const bytes of fileBytesMap.values()) {
          sum += bytes;
        }
        return sum;
      }

      // XHR for small files
      const uploadSingle = (file: File, folder: string, fileIdx: number): Promise<void> => {
        return new Promise((resolve, reject) => {
          const xhr = new XMLHttpRequest();
          const formData = new FormData();
          formData.append('file', file);
          if (folder.trim()) formData.append('folder', folder.trim());

          xhr.upload.onprogress = (e) => {
            if (e.lengthComputable) {
              fileBytesMap.set(fileIdx, e.loaded);
              const currentLoaded = recalculateLoadedBytes();

              update((s) => ({
                ...s,
                loadedBytes: currentLoaded,
                activeUploads: s.activeUploads.map((item) =>
                  item.fileIdx === fileIdx
                    ? {
                        ...item,
                        loaded: e.loaded,
                        percent: Math.min(100, Math.round((e.loaded / e.total) * 100))
                      }
                    : item
                )
              }));
            }
          };

          xhr.onload = () => {
            if (xhr.status >= 200 && xhr.status < 300) {
              fileBytesMap.set(fileIdx, file.size);
              resolve();
            } else if (xhr.status === 401) {
              authStore.checkStatus();
              reject(new Error('Session expired.'));
            } else {
              reject(new Error(xhr.responseText || `Status ${xhr.status}`));
            }
          };

          xhr.onerror = () => reject(new Error('Network connection error'));
          xhr.open('POST', '/api/upload');
          xhr.withCredentials = true;
          xhr.send(formData);
        });
      };

      // Segregate files into Small (concurrency 2-3) vs Large (strict sequential 1-by-1)
      const indexedFiles: IndexedFile[] = files.map((file, fileIdx) => ({
        file,
        fileIdx,
        isLarge: file.size > CHUNK_THRESHOLD_BYTES || file.type.startsWith('video/')
      }));

      const smallFiles = indexedFiles.filter((item) => !item.isLarge);
      const largeFiles = indexedFiles.filter((item) => item.isLarge);

      const runFile = async (item: IndexedFile) => {
        const { file, fileIdx, isLarge } = item;
        fileBytesMap.set(fileIdx, 0);

        // Step 1: Register as hashing in progress UI
        update((s) => ({
          ...s,
          activeUploads: [
            ...s.activeUploads,
            {
              fileIdx,
              name: file.name,
              size: file.size,
              loaded: 0,
              percent: 0,
              status: 'hashing'
            }
          ]
        }));

        try {
          // Compute client-side SHA-256 for duplicate check
          let isDuplicate = false;
          try {
            const hash = await computeFileSHA256(file);
            const dupResult = await checkDuplicate(hash, file.name, folderPath);
            if (dupResult.exists) {
              isDuplicate = true;
            }
          } catch (hashErr) {
            console.warn('Fast hashing bypassed:', hashErr);
          }

          // If duplicate exists on server, skip uploading completely!
          if (isDuplicate) {
            fileBytesMap.set(fileIdx, file.size);
            completedCount++;
            const currentLoaded = recalculateLoadedBytes();

            update((s) => ({
              ...s,
              completedFiles: completedCount,
              loadedBytes: currentLoaded,
              activeUploads: s.activeUploads.filter((act) => act.fileIdx !== fileIdx)
            }));
            return;
          }

          // Step 2: Proceed with upload if unique
          update((s) => ({
            ...s,
            activeUploads: s.activeUploads.map((act) =>
              act.fileIdx === fileIdx
                ? { ...act, status: isLarge ? 'chunking' : 'uploading' }
                : act
            )
          }));

          if (isLarge) {
            await uploadChunked(file, folderPath, (part, totalParts) => {
              const approxLoaded = Math.min(file.size, (part / totalParts) * file.size);
              fileBytesMap.set(fileIdx, approxLoaded);
              const currentLoaded = recalculateLoadedBytes();

              update((s) => ({
                ...s,
                loadedBytes: currentLoaded,
                activeUploads: s.activeUploads.map((act) =>
                  act.fileIdx === fileIdx
                    ? {
                        ...act,
                        loaded: approxLoaded,
                        percent: Math.min(100, Math.round((part / totalParts) * 100))
                      }
                    : act
                )
              }));
            });
            fileBytesMap.set(fileIdx, file.size);
          } else {
            await uploadSingle(file, folderPath, fileIdx);
          }

          completedCount++;
          const currentLoaded = recalculateLoadedBytes();

          update((s) => ({
            ...s,
            completedFiles: completedCount,
            loadedBytes: currentLoaded,
            activeUploads: s.activeUploads.filter((act) => act.fileIdx !== fileIdx)
          }));
        } catch (err: any) {
          failedCount++;
          fileBytesMap.set(fileIdx, 0); // Rollback partial bytes
          const currentLoaded = recalculateLoadedBytes();

          update((s) => ({
            ...s,
            failedFiles: failedCount,
            loadedBytes: currentLoaded,
            activeUploads: s.activeUploads.map((act) =>
              act.fileIdx === fileIdx ? { ...act, status: 'failed', percent: 0 } : act
            ),
            error: `Upload failed for "${file.name}": ${err.message}`
          }));
        }
      };

      try {
        // Phase 1: Small files in parallel
        if (smallFiles.length > 0) {
          const SMALL_CONCURRENCY = 3;
          let smallCursor = 0;

          const smallWorker = async () => {
            while (smallCursor < smallFiles.length) {
              const item = smallFiles[smallCursor++];
              await runFile(item);
            }
          };

          const poolSize = Math.min(SMALL_CONCURRENCY, smallFiles.length);
          await Promise.all(Array.from({ length: poolSize }, () => smallWorker()));
        }

        // Phase 2: Large files strictly in series
        for (const item of largeFiles) {
          await runFile(item);
        }
      } catch (e: any) {
        update((s) => ({ ...s, error: e.message || 'Batch upload failed.' }));
      } finally {
        onDone?.({ completed: completedCount, failed: failedCount });
      }
    }
  };
}

export const uploadProgressStore = createUploadProgressStore();