// photo-app/frontend/src/lib/stores/uploadStore.ts

import { writable } from 'svelte/store';
import { authStore } from '$lib/stores/authStore';
import { CHUNK_THRESHOLD_BYTES } from '$lib/utils/uploader';
import { computeFileSHA256 } from '$lib/utils/hasher';
import {
  apiStartBatch,
  apiCheckDuplicate,
  apiUploadSingleXhr,
  apiUploadChunked,
  apiFinishBatch,
} from '$lib/api/upload';
import type { CandidateItem, InspectPreview, UploadMode } from '$lib/types/upload';

export interface ActiveFileProgress {
  fileIdx: number;
  name: string;
  size: number;
  loaded: number;
  percent: number;
  status: 'hashing' | 'uploading' | 'chunking' | 'completed' | 'failed';
}

export interface UploadStoreState {
  // Staging state (used by UploadModal)
  uploadMode: UploadMode;
  folderPath: string;
  stagedFiles: File[];
  linkUrl: string;
  linkPreview: InspectPreview | null;
  selectedLinkItems: Set<string>;
  isInspecting: boolean;

  // Active Progress state (used by Floating Toast / Widget)
  isWidgetOpen: boolean;
  isMinimized: boolean;
  isUploading: boolean;
  totalFiles: number;
  completedFiles: number;
  failedFiles: number;
  totalBytes: number;
  loadedBytes: number;
  startTime: number;
  activeUploads: ActiveFileProgress[];
  error: string | null;
}

const initialState: UploadStoreState = {
  uploadMode: 'files',
  folderPath: '',
  stagedFiles: [],
  linkUrl: '',
  linkPreview: null,
  selectedLinkItems: new Set(),
  isInspecting: false,

  isWidgetOpen: false,
  isMinimized: false,
  isUploading: false,
  totalFiles: 0,
  completedFiles: 0,
  failedFiles: 0,
  totalBytes: 0,
  loadedBytes: 0,
  startTime: 0,
  activeUploads: [],
  error: null,
};

interface IndexedFile {
  file: File;
  fileIdx: number;
  isLarge: boolean;
}

function createUnifiedUploadStore() {
  const { subscribe, update, set } = writable<UploadStoreState>(initialState);

  return {
    subscribe,

    // --- Modal Staging Controls ---
    setMode: (mode: UploadMode) => update((s) => ({ ...s, uploadMode: mode, error: null })),
    setFolderPath: (path: string) => update((s) => ({ ...s, folderPath: path })),
    setLinkUrl: (url: string) => update((s) => ({ ...s, linkUrl: url })),
    clearLinkPreview: () => update((s) => ({ ...s, linkPreview: null, error: null })),

    addFiles: (files: FileList | File[]) => {
      const valid = Array.from(files).filter(
        (f) =>
          f.type.startsWith('image/') ||
          f.type.startsWith('video/') ||
          /\.(heic|heif|mov|mp4|jpg|jpeg|png|webp|gif|mkv|webm)$/i.test(f.name)
      );
      update((s) => ({ ...s, stagedFiles: [...s.stagedFiles, ...valid] }));
    },

    removeStagedFile: (index: number) => {
      update((s) => ({ ...s, stagedFiles: s.stagedFiles.filter((_, i) => i !== index) }));
    },

    toggleLinkItem: (id: string) => {
      update((s) => {
        const next = new Set(s.selectedLinkItems);
        if (next.has(id)) next.delete(id);
        else next.add(id);
        return { ...s, selectedLinkItems: next };
      });
    },

    inspectLink: async () => {
      let currentUrl = '';
      update((s) => {
        currentUrl = s.linkUrl.trim();
        return { ...s, isInspecting: true, error: null };
      });

      if (!currentUrl) return;

      try {
        const res = await fetch('/api/upload/inspect', {
          method: 'POST',
          headers: { 'Content-Type': 'application/json' },
          credentials: 'include',
          body: JSON.stringify({ url: currentUrl }),
        });

        if (res.status === 401) {
          authStore.checkStatus();
          throw new Error('Session expired.');
        }

        if (!res.ok) {
          const errorText = await res.text();
          throw new Error(errorText || `Server error (${res.status})`);
        }

        const data = await res.json();
        const previewData: InspectPreview | null = data.Preview ?? (data.items ? data : null);

        if (previewData && Array.isArray(previewData.items)) {
          update((s) => ({
            ...s,
            linkPreview: previewData,
            folderPath: previewData.suggested_folder || s.folderPath,
            selectedLinkItems: new Set(previewData.items.map((i: CandidateItem) => i.id)),
            isInspecting: false,
          }));
        } else {
          throw new Error('No supported media found.');
        }
      } catch (e: any) {
        update((s) => ({ ...s, isInspecting: false, error: e.message || 'Inspection failed.' }));
      }
    },

    // --- Widget UI Controls ---
    toggleMinimize: () => update((s) => ({ ...s, isMinimized: !s.isMinimized })),
    closeWidget: () =>
      update((s) => ({
        ...s,
        isWidgetOpen: false,
        activeUploads: [],
        error: null,
      })),

    resetModal: () =>
      update((s) => ({
        ...s,
        stagedFiles: [],
        linkUrl: '',
        linkPreview: null,
        selectedLinkItems: new Set(),
      })),

    // --- Execution Core ---
    startUpload: async (onComplete?: (count: number) => void) => {
      let snapshot: UploadStoreState;
      const unsub = subscribe((s) => (snapshot = s));
      unsub();

      const files = [...snapshot!.stagedFiles];
      const folderPath = snapshot!.folderPath;

      if (files.length === 0) return;

      const totalFiles = files.length;
      const totalBytes = files.reduce((acc, f) => acc + f.size, 0);

      update((s) => ({
        ...s,
        stagedFiles: [], // clear modal staging immediately
        isWidgetOpen: true,
        isMinimized: false,
        isUploading: true,
        totalFiles,
        completedFiles: 0,
        failedFiles: 0,
        totalBytes,
        loadedBytes: 0,
        startTime: Date.now(),
        activeUploads: [],
        error: null,
      }));

      let completedCount = 0;
      let failedCount = 0;
      const fileBytesMap = new Map<number, number>();

      function recalculateLoadedBytes(): number {
        let sum = 0;
        for (const bytes of fileBytesMap.values()) sum += bytes;
        return sum;
      }

      // Step 1: Open explicit batch session
      const batchId = await apiStartBatch();

      const indexedFiles: IndexedFile[] = files.map((file, fileIdx) => ({
        file,
        fileIdx,
        isLarge: file.size > CHUNK_THRESHOLD_BYTES || file.type.startsWith('video/'),
      }));

      const smallFiles = indexedFiles.filter((item) => !item.isLarge);
      const largeFiles = indexedFiles.filter((item) => item.isLarge);

      const runFile = async (item: IndexedFile) => {
        const { file, fileIdx, isLarge } = item;
        fileBytesMap.set(fileIdx, 0);

        if (!isLarge) {
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
                status: 'hashing',
              },
            ],
          }));

          let isDuplicate = false;
          try {
            const hash = await computeFileSHA256(file);
            const dupResult = await apiCheckDuplicate(hash, file.name, folderPath);
            if (dupResult.exists) isDuplicate = true;
          } catch (hashErr) {
            console.warn('Duplicate pre-check bypassed:', hashErr);
          }

          if (isDuplicate) {
            fileBytesMap.set(fileIdx, file.size);
            completedCount++;
            const currentLoaded = recalculateLoadedBytes();

            update((s) => ({
              ...s,
              completedFiles: completedCount,
              loadedBytes: currentLoaded,
              activeUploads: s.activeUploads.filter((act) => act.fileIdx !== fileIdx),
            }));
            return;
          }
        }

        update((s) => ({
          ...s,
          activeUploads: [
            ...s.activeUploads.filter((act) => act.fileIdx !== fileIdx),
            {
              fileIdx,
              name: file.name,
              size: file.size,
              loaded: 0,
              percent: 0,
              status: isLarge ? 'chunking' : 'uploading',
            },
          ],
        }));

        try {
          if (isLarge) {
            await apiUploadChunked(file, folderPath, batchId, (part, totalParts) => {
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
                        percent: Math.min(100, Math.round((part / totalParts) * 100)),
                      }
                    : act
                ),
              }));
            });
            fileBytesMap.set(fileIdx, file.size);
          } else {
            await apiUploadSingleXhr(file, folderPath, batchId, (loaded, total) => {
              fileBytesMap.set(fileIdx, loaded);
              const currentLoaded = recalculateLoadedBytes();

              update((s) => ({
                ...s,
                loadedBytes: currentLoaded,
                activeUploads: s.activeUploads.map((act) =>
                  act.fileIdx === fileIdx
                    ? {
                        ...act,
                        loaded,
                        percent: Math.min(100, Math.round((loaded / total) * 100)),
                      }
                    : act
                ),
              }));
            });
            fileBytesMap.set(fileIdx, file.size);
          }

          completedCount++;
          const currentLoaded = recalculateLoadedBytes();

          update((s) => ({
            ...s,
            completedFiles: completedCount,
            loadedBytes: currentLoaded,
            activeUploads: s.activeUploads.filter((act) => act.fileIdx !== fileIdx),
          }));
        } catch (err: any) {
          failedCount++;
          fileBytesMap.set(fileIdx, 0);
          const currentLoaded = recalculateLoadedBytes();

          if (err.message === 'Session expired.') {
            authStore.checkStatus();
          }

          update((s) => ({
            ...s,
            failedFiles: failedCount,
            loadedBytes: currentLoaded,
            activeUploads: s.activeUploads.map((act) =>
              act.fileIdx === fileIdx ? { ...act, status: 'failed', percent: 0 } : act
            ),
            error: `Upload failed for "${file.name}": ${err.message}`,
          }));
        }
      };

      try {
        if (smallFiles.length > 0) {
          let smallCursor = 0;
          const smallWorker = async () => {
            while (smallCursor < smallFiles.length) {
              const item = smallFiles[smallCursor++];
              await runFile(item);
            }
          };
          const poolSize = Math.min(3, smallFiles.length);
          await Promise.all(Array.from({ length: poolSize }, () => smallWorker()));
        }

        for (const item of largeFiles) {
          await runFile(item);
        }
      } catch (e: any) {
        update((s) => ({ ...s, error: e.message || 'Batch upload failed.' }));
      } finally {
        update((s) => ({ ...s, isUploading: false }));

        if (batchId && completedCount > 0) {
          await apiFinishBatch(batchId);
        }

        onComplete?.(completedCount);
      }
    },
  };
}

export const uploadStore = createUnifiedUploadStore();