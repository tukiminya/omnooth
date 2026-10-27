import { invoke, isTauri } from '@tauri-apps/api/core';
import { open } from '@tauri-apps/plugin-dialog';
import { listen } from '@tauri-apps/api/event';
import type { AppSettings, AppSnapshot, ArchiveBatchResult, DeepLinkStatus, ImportResult } from './contracts';

const desktop = isTauri();

function normalizeLanguage(value: string): AppSettings['language'] {
  const base = value.toLowerCase().split(/[-_]/)[0];
  return base === 'en' || base === 'ko' ? base : 'ja';
}

function requireDesktop() {
  if (!desktop) throw new Error('DESKTOP_REQUIRED');
}

async function dispatch<T>(type: string, input?: unknown): Promise<T> {
  requireDesktop();
  return invoke<T>('dispatch', { command: { type, ...(input === undefined ? {} : { input }) } });
}

async function getSnapshot(): Promise<AppSnapshot> {
  if (!desktop) {
    return {
      projects: [], items: [],
      settings: {
        language: normalizeLanguage(navigator.language), libraryPath: '',
        extractArchives: false, keepOldVersions: true,
      },
    };
  }
  return dispatch('getSnapshot');
}

export const client = {
  isDesktop: desktop,
  getSnapshot,
  async getDeepLinkStatus(): Promise<DeepLinkStatus> {
    if (!desktop) return { scheme: 'booth-library-manager', registered: false, receivedCount: 0, lastReceipt: null, lastError: null };
    return invoke('get_deep_link_status');
  },
  async registerDeepLink(): Promise<DeepLinkStatus> {
    requireDesktop();
    return invoke('register_deep_link');
  },
  async onDeepLinkStatus(callback: (status: DeepLinkStatus) => void): Promise<() => void> {
    if (!desktop) return () => {};
    return listen<DeepLinkStatus>('deep-link-status-changed', (event) => callback(event.payload));
  },
  async openManual(): Promise<void> {
    if (desktop) {
      await invoke('open_manual');
    } else {
      const tab = window.open('https://omnooth.tuki.dev/docs', '_blank', 'noopener,noreferrer');
      if (tab) tab.opener = null;
    }
  },
  async chooseProject(): Promise<AppSnapshot> {
    requireDesktop();
    const path = await open({ directory: true, multiple: false });
    if (path) await dispatch('linkProject', { directoryPath: path });
    return getSnapshot();
  },
  async chooseArchives(): Promise<ArchiveBatchResult> {
    requireDesktop();
    const paths = await open({
      directory: false, multiple: true,
      filters: [{ name: 'Unity packages / archives', extensions: ['unitypackage', 'zip', '7z', 'rar', 'tar', 'gz', 'tgz', 'bz2', 'tbz2', 'xz', 'txz'] }],
    });
    const imported: ImportResult[] = [];
    const failed: ArchiveBatchResult['failed'] = [];
    const settings = (await getSnapshot()).settings;
    for (const path of paths ?? []) {
      const originalFilename = path.split(/[\\/]/).pop() ?? path;
      try {
        imported.push(await dispatch<ImportResult>('importArchive', {
          archivePath: path, originalFilename,
          extractArchive: settings.extractArchives,
        }));
      } catch (error) {
        failed.push({ filename: originalFilename, error: String(error) });
      }
    }
    return { snapshot: await getSnapshot(), imported, failed };
  },
  async extractArchive(versionId: string): Promise<AppSnapshot> {
    await dispatch('extractArchive', { versionId });
    return getSnapshot();
  },
  async chooseLibraryPath(): Promise<string | null> {
    requireDesktop();
    return open({ directory: true, multiple: false });
  },
  async updateSettings(patch: Partial<AppSettings>): Promise<AppSnapshot> {
    const current = (await getSnapshot()).settings;
    await dispatch('updateSettings', { settings: { ...current, ...patch } });
    return getSnapshot();
  },
};
