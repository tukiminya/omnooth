export interface AppSettings {
  language: 'ja' | 'en' | 'ko';
  libraryPath: string;
  extractArchives: boolean;
  keepOldVersions: boolean;
}

export interface Project {
  id: string;
  name: string;
  path: string;
  unityVersion: string;
  lastUsedAt: string | null;
}

export interface ContentFile {
  id: string;
  relativePath: string;
  kind: string;
  sizeBytes: number;
  contentHash: string;
}

export interface LibraryVersion {
  id: string;
  label: string;
  versionDate: string;
  contentHash: string;
  originalFilename: string;
  state: 'stored' | 'extracted' | 'ready';
  files: ContentFile[];
}

export interface LibraryItem {
  id: string;
  title: string;
  versions: LibraryVersion[];
}

export interface AppSnapshot {
  projects: Project[];
  items: LibraryItem[];
  settings: AppSettings;
}

export interface ImportResult {
  item: LibraryItem;
  reused: boolean;
  warnings: string[];
}

export interface ArchiveBatchResult {
  snapshot: AppSnapshot;
  imported: ImportResult[];
  failed: { filename: string; error: string }[];
}

export interface DeepLinkStatus {
  scheme: string;
  registered: boolean;
  receivedCount: number;
  lastReceipt: {
    boothItemId: string;
    variationId: string | null;
    filename: string;
    sourceIdentity: string;
  } | null;
  lastError: string | null;
}
