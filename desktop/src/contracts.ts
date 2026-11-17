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
  contentHash?: string;
  encrypted?: boolean;
}

export interface ArchiveContents {
  versionId: string;
  entries: Array<ContentFile & { encrypted: boolean }>;
}

export interface LibraryVersion {
  id: string;
  label: string;
  versionDate: string;
  contentHash: string;
  originalFilename: string;
  state: 'stored' | 'extracted' | 'ready';
  files: ContentFile[];
  requiredVersionIds?: string[];
}

export interface LibraryItem {
  id: string;
  title: string;
  boothItemId?: string | null;
  kind?: string;
  displayParentId?: string | null;
  description?: string;
  details?: { description: string; shop: string; category: string; tags: string[]; ageRating: string; isStarred: boolean; releaseNotes: string | null };
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
  lastImport?: {
    jobId: number;
    filename: string;
    state: 'queued' | 'downloading' | 'importing' | 'awaitingUrl' | 'failed' | 'succeeded';
    errorCode: string | null;
    itemId: string | null;
  } | null;
  activeCount?: number;
}

export interface LogSnapshot {
  sessionId: string;
  entries: Array<{
    id: number;
    timestamp: string;
    level: 'info' | 'warn' | 'error';
    operation: string;
    message: string;
    correlationId: string | null;
  }>;
}

export interface InstallSelection {
  versionId: string;
  fileIds: string[];
}

export interface InstallPlan {
  id: string;
  projectId: string;
  baseStateHash: string;
  planHash: string;
  selections: InstallSelection[];
  operations: Array<{ packageId: string; title: string; kind: 'add' | 'update' | 'reselect' | 'remove'; fromVersionId: string | null; toVersionId: string | null }>;
  assetManifest: Array<{ sourceFileId: string; packageId: string; versionId: string; relativePath: string; assetGuid: string; contentHash: string | null; owned: boolean }>;
  removedPaths: string[];
  importOrder: string[];
  requiredBytes: number;
}

export interface InstallationJob {
  id: string;
  projectId: string;
  status: 'preparing' | 'waitingForUnity' | 'recovering' | 'succeeded' | 'restored';
  errorCode: string | null;
  createdAt: string;
}

export interface InstallationState {
  packages: Array<{ packageId: string; title: string; versionId: string; versionLabel: string; fileIds: string[] }>;
  jobs: InstallationJob[];
}

export interface RecoveryReport {
  restoredPaths: string[];
  conflicts: string[];
  requiresManualAction: boolean;
}

export interface SaveCustomInput {
  packageId: string | null;
  name: string;
  description: string;
  displayParentId: string;
  unitypackagePaths: string[];
  requiredVersionIds: string[];
}
