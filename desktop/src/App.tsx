import { useEffect, useMemo, useRef, useState } from 'react';
import { useTranslation } from 'react-i18next';
import { toast } from 'sonner';
import i18n from './i18n';
import { client } from './client';
import { Icon } from './Icon';
import { LogDialog } from './LogDialog';
import { AppToaster } from './AppToaster';
import { ArchiveContentsList } from './ArchiveContentsList';
import { formatBytes } from './formatBytes';
import type { AppSettings, AppSnapshot, ArchiveContents, DeepLinkStatus, LibraryItem, LibraryVersion, Project } from './contracts';
import './fonts.css';
import './app.css';

type Page = 'projects' | 'library';
type SettingsTab = 'general' | 'history';
const appErrorToastId = 'app-error';

function App() {
  const { t, i18n } = useTranslation();
  const [snapshot, setSnapshot] = useState<AppSnapshot | null>(null);
  const [activePage, setActivePage] = useState<Page>('projects');
  const [projectSort, setProjectSort] = useState<'lastUsed' | 'name'>('lastUsed');
  const [selectedProjectId, setSelectedProjectId] = useState<string | null>(null);
  const [selectedItemId, setSelectedItemId] = useState<string | null>(null);
  const [selectedVersionId, setSelectedVersionId] = useState<string | null>(null);
  const [chosenUnityPackages, setChosenUnityPackages] = useState<string[]>([]);
  const [archivePreview, setArchivePreview] = useState<{ versionId: string; status: 'loading' | 'ready' | 'error'; entries: ArchiveContents['entries'] } | null>(null);
  const [archivePreviewAttempt, setArchivePreviewAttempt] = useState(0);
  const [settingsOpen, setSettingsOpen] = useState(false);
  const [logsOpen, setLogsOpen] = useState(false);
  const [settingsDraft, setSettingsDraft] = useState<AppSettings | null>(null);
  const [settingsTab, setSettingsTab] = useState<SettingsTab>('general');
  const [deepLinkStatus, setDeepLinkStatus] = useState<DeepLinkStatus | null>(null);
  const [deepLinkError, setDeepLinkError] = useState('');
  const [query, setQuery] = useState('');
  const [busy, setBusy] = useState(false);
  const [isLoading, setIsLoading] = useState(true);
  const lastReceiptKey = useRef<string | null>(null);
  const latestImportStateSignature = useRef(new Map<number, string>());
  const refreshedImportJobs = useRef(new Set<number>());
  const refreshingImportJobs = useRef(new Set<number>());
  const snapshotGeneration = useRef(0);

  async function refreshForDeepLinkImport(importResult: NonNullable<DeepLinkStatus['lastImport']>, navigateToLibrary: boolean): Promise<boolean> {
    const generation = ++snapshotGeneration.current;
    try {
      const next = await client.getSnapshot();
      if (generation !== snapshotGeneration.current) return true;
      setSnapshot(next);
      toast.dismiss(appErrorToastId);
      const item = importResult.itemId ? next.items.find((candidate) => candidate.id === importResult.itemId) : null;
      if (!item) {
        if (importResult.itemId) toast.error(t('deepLinkRefreshError'), { id: appErrorToastId, duration: Infinity });
        return !importResult.itemId;
      }
      setSelectedItemId(item.id);
      setSelectedVersionId(latestVersion(item.versions)?.id ?? null);
      setChosenUnityPackages([]);
      if (navigateToLibrary) setActivePage('library');
      return true;
    } catch (cause) {
      if (generation === snapshotGeneration.current) toast.error(errorMessage(cause, t('deepLinkRefreshError')), { id: appErrorToastId, duration: Infinity });
      void client.reportError('deep-link-library-refresh', String(cause));
      return false;
    }
  }

  function handleImportStatus(status: NonNullable<DeepLinkStatus['lastImport']>, initial: boolean, activeCount: number | undefined, receivedCount: number) {
    const restoredCompletedJob = initial && receivedCount === 0;
    const signature = `${status.state}:${status.errorCode ?? ''}`;
    const isNewTransition = latestImportStateSignature.current.get(status.jobId) !== signature;
    if (isNewTransition) latestImportStateSignature.current.set(status.jobId, signature);
    if (status.state === 'succeeded' && !refreshedImportJobs.current.has(status.jobId) && !refreshingImportJobs.current.has(status.jobId)) {
      refreshingImportJobs.current.add(status.jobId);
      void refreshForDeepLinkImport(status, !restoredCompletedJob).then((refreshed) => {
        refreshingImportJobs.current.delete(status.jobId);
        if (refreshed) refreshedImportJobs.current.add(status.jobId);
      });
    }
    if (!isNewTransition) return;
    const common = { filename: status.filename, count: activeCount ?? 1 };
    const options = { id: `booth-import-${status.jobId}`, duration: Infinity, action: undefined };
    const logAction = { label: t('deepLinkOpenLogs'), onClick: () => setLogsOpen(true) };
    switch (status.state) {
      case 'queued': toast.loading(t('deepLinkQueued', common), options); break;
      case 'downloading': toast.loading(t('deepLinkDownloading', common), options); break;
      case 'importing': toast.loading(t('deepLinkImporting', common), options); break;
      case 'awaitingUrl': toast.warning(t('deepLinkAwaitingUrl', common), { ...options, action: logAction }); break;
      case 'failed': toast.error(t('deepLinkImportFailed', { ...common, reason: status.errorCode ?? t('deepLinkUnknownReason') }), { ...options, action: logAction }); break;
      case 'succeeded':
        if (!restoredCompletedJob) toast.success(t('deepLinkImportSucceeded', common), { ...options, duration: 5000 });
        break;
    }
  }

  useEffect(() => {
    const generation = snapshotGeneration.current;
    client.getSnapshot().then((next) => {
      if (generation === snapshotGeneration.current) setSnapshot(next);
      if (i18n.language !== next.settings.language) void i18n.changeLanguage(next.settings.language);
    }).catch((cause: unknown) => {
      toast.error(errorMessage(cause, i18n.t('loadError')), { id: appErrorToastId, duration: Infinity });
      void client.reportError('app-startup', String(cause));
    }).finally(() => setIsLoading(false));
  }, [i18n]);

  useEffect(() => {
    let disposed = false;
    let receivedEvent = false;
    let unsubscribe: (() => void) | undefined;
    const applyDeepLinkStatus = (status: DeepLinkStatus, initial: boolean) => {
      if (disposed) return;
      setDeepLinkStatus(status);
      setDeepLinkError(status.lastError ? (status.lastError === 'BUNDLE_REQUIRED' ? i18n.t('bundleRequired') : i18n.t('deepLinkError')) : '');
      const receipt = status.lastReceipt;
      const key = receipt ? `${status.receivedCount}\u0000${receipt.boothItemId}\u0000${receipt.variationId ?? ''}\u0000${receipt.filename}\u0000${receipt.sourceIdentity}` : null;
      if (receipt && key && key !== lastReceiptKey.current) {
        lastReceiptKey.current = key;
        if (!status.lastImport) toast.info(i18n.t('deepLinkReceived', { filename: receipt.filename }), { id: 'booth-receipt' });
      }
      if (status.lastImport) handleImportStatus(status.lastImport, initial, status.activeCount, status.receivedCount);
    };
    void (async () => {
      try {
        const stop = await client.onDeepLinkStatus((status) => {
          receivedEvent = true;
          applyDeepLinkStatus(status, false);
        });
        if (disposed) stop();
        else unsubscribe = stop;
      } catch {
        if (!disposed) setDeepLinkError(i18n.t('deepLinkError'));
        void client.reportError('deep-link-listener', 'Could not subscribe to deep-link status updates');
      }
      try {
        const status = await client.getDeepLinkStatus();
        if (!receivedEvent) applyDeepLinkStatus(status, true);
      } catch (cause) {
        if (!disposed) setDeepLinkError(deepLinkErrorMessage(cause));
        void client.reportError('deep-link-status', String(cause));
      }
    })();
    return () => { disposed = true; unsubscribe?.(); };
  }, [i18n]);

  const selectedProject = snapshot?.projects.find((project) => project.id === selectedProjectId) ?? null;
  const selectedItem = snapshot?.items.find((item) => item.id === selectedItemId) ?? null;
  const storedVersion = selectedItem?.versions.find((version) => version.id === selectedVersionId) ?? selectedItem?.versions[0] ?? null;
  const inspectVersionId = activePage === 'library' && storedVersion?.state === 'stored' && storedVersion.originalFilename.toLowerCase().endsWith('.zip') ? storedVersion.id : null;
  const currentPreview = archivePreview?.versionId === inspectVersionId ? archivePreview : null;
  const selectedVersion = storedVersion && currentPreview?.status === 'ready' ? { ...storedVersion, files: currentPreview.entries.filter((entry) => entry.kind !== 'directory') } : storedVersion;

  useEffect(() => {
    if (!inspectVersionId) { setArchivePreview(null); return; }
    let disposed = false;
    setArchivePreview({ versionId: inspectVersionId, status: 'loading', entries: [] });
    void client.inspectArchive(inspectVersionId).then((contents) => {
      if (!disposed) setArchivePreview({ versionId: inspectVersionId, status: 'ready', entries: contents.entries });
    }).catch((cause: unknown) => {
      if (!disposed) setArchivePreview({ versionId: inspectVersionId, status: 'error', entries: [] });
      void client.reportError('inspectArchive', String(cause));
    });
    return () => { disposed = true; };
  }, [inspectVersionId, archivePreviewAttempt]);
  const filteredProjects = useMemo(() => (snapshot?.projects ?? []).filter((project) => `${project.name} ${project.path}`.toLowerCase().includes(query.toLowerCase())), [snapshot, query]);
  const orderedProjects = useMemo(() => {
    const collator = new Intl.Collator(snapshot?.settings.language ?? 'ja');
    return [...filteredProjects].sort((left, right) => {
      if (projectSort === 'name') return collator.compare(left.name, right.name);
      if (left.lastUsedAt === null && right.lastUsedAt !== null) return 1;
      if (left.lastUsedAt !== null && right.lastUsedAt === null) return -1;
      const byDate = left.lastUsedAt && right.lastUsedAt ? Date.parse(right.lastUsedAt) - Date.parse(left.lastUsedAt) : 0;
      return byDate || collator.compare(left.name, right.name);
    });
  }, [filteredProjects, projectSort, snapshot?.settings.language]);
  const filteredItems = useMemo(() => (snapshot?.items ?? []).filter((item) => `${item.title} ${item.versions.map((v) => v.originalFilename).join(' ')}`.toLowerCase().includes(query.toLowerCase())), [snapshot, query]);
  const visibleSelection = chosenUnityPackages.filter((id) => selectedVersion?.files.some((file) => file.id === id && file.kind === 'unitypackage' && !file.encrypted));

  function applySnapshot(next: AppSnapshot) {
    snapshotGeneration.current += 1;
    setSnapshot(next);
    const selectedItemStillExists = next.items.some((item) => item.id === selectedItemId);
    if (!selectedItemStillExists) {
      setSelectedItemId(next.items[0]?.id ?? null);
      setSelectedVersionId(next.items[0]?.versions[0]?.id ?? null);
      setChosenUnityPackages([]);
    }
  }

  async function perform<T>(action: () => Promise<T>, success?: (value: T) => string | undefined) {
    setBusy(true); toast.dismiss(appErrorToastId);
    try {
      const value = await action();
      const message = success?.(value);
      if (message) toast.success(message);
      return value;
    } catch (cause) {
      toast.error(errorMessage(cause, t('operationError')), { id: appErrorToastId, duration: Infinity });
      void client.reportError('ui.operation', String(cause));
      return undefined;
    } finally { setBusy(false); }
  }

  async function openManual() {
    setBusy(true); toast.dismiss(appErrorToastId);
    try {
      await client.openManual();
    } catch (cause) {
      toast.error(errorMessage(cause, t('manualOpenError')), { id: appErrorToastId, duration: Infinity });
      void client.reportError('open-manual', String(cause));
    } finally {
      setBusy(false);
    }
  }

  async function registerDeepLink() {
    setBusy(true);
    toast.dismiss(appErrorToastId);
    setDeepLinkError('');
    try {
      const status = await client.registerDeepLink();
      setDeepLinkStatus(status);
      toast.success(t('deepLinkRegisteredNotice'));
    } catch (cause) {
      const code = cause instanceof Error ? cause.message : typeof cause === 'string' ? cause : '';
      setDeepLinkError(code === 'BUNDLE_REQUIRED' ? i18n.t('bundleRequired') : t('deepLinkError'));
      void client.reportError('register-deep-link', String(cause));
    } finally {
      setBusy(false);
    }
  }

  async function linkProject() {
    const previousIds = new Set(snapshot?.projects.map((project) => project.id) ?? []);
    const next = await perform(() => client.chooseProject());
    if (next) {
      applySnapshot(next);
      const linked = next.projects.find((project) => !previousIds.has(project.id));
      if (linked) {
        setSelectedProjectId(linked.id);
        toast.success(t('projectLinked'));
      }
    }
  }

  async function addArchives() {
    const result = await perform(() => client.chooseArchives());
    if (result) {
      applySnapshot(result.snapshot);
      setActivePage('library');
      const issues = [
        ...result.failed.map((failure) => ({ filename: failure.filename, kind: 'failed' as const, message: t('importFailed'), detail: failure.error })),
        ...result.imported.flatMap((entry) => entry.warnings.map((warning) => ({ filename: latestVersion(entry.item.versions)?.originalFilename ?? entry.item.title, kind: 'warning' as const, message: localizedImportWarning(warning, t) }))),
      ];
      const message = result.imported.length || result.failed.length ? t('importSummary', { imported: result.imported.length, failed: result.failed.length, warnings: issues.filter((issue) => issue.kind === 'warning').length }) : t('noFilesAdded');
      const notify = result.failed.length ? toast.error : issues.length ? toast.warning : result.imported.length ? toast.success : toast.info;
      notify(message, {
        duration: issues.length ? Infinity : 5000,
        description: issues.length ? <ul className="import-issues">{issues.map((issue, index) => <li key={`${issue.filename}-${index}`}><strong>{issue.filename}</strong>: {issue.message}{'detail' in issue && <details><summary>{t('errorDetails')}</summary><code>{issue.detail}</code></details>}</li>)}</ul> : undefined,
      });
      const first = result.imported[0]?.item;
      if (first) {
        const latest = latestVersion(first.versions);
        setSelectedItemId(first.id);
        setSelectedVersionId(latest?.id ?? null);
        setChosenUnityPackages([]);
      }
    }
  }

  async function extractSelected() {
    if (!selectedVersion) return;
    const next = await perform(() => client.extractArchive(selectedVersion.id));
    if (next) applySnapshot(next);
  }

  async function chooseLibraryPath() {
    const path = await perform(() => client.chooseLibraryPath());
    if (path) setSettingsDraft((current) => current ? { ...current, libraryPath: path } : current);
  }

  function openSettings() {
    if (!snapshot) return;
    setSettingsDraft({ ...snapshot.settings });
    setSettingsTab('general');
    setSettingsOpen(true);
  }

  async function saveSettings() {
    if (!settingsDraft) return;
    const next = await perform(() => client.updateSettings(settingsDraft));
    if (next) {
      snapshotGeneration.current += 1;
      setSnapshot(next);
      if (i18n.language !== next.settings.language) await i18n.changeLanguage(next.settings.language);
      setSettingsOpen(false);
      toast.success(i18n.t('settingsSaved'));
    }
  }

  const nav = [
    { page: 'projects' as const, label: t('projects'), icon: 'folder' as const },
    { page: 'library' as const, label: t('library'), icon: 'packages' as const },
  ];

  return (
    <div className="app-shell">
      <Header project={selectedProject} query={query} onQuery={setQuery} onSearch={() => setActivePage('library')} onProjectClick={() => setActivePage('projects')} onAdd={addArchives} busy={busy} t={t} />
      <div className="workspace">
        <aside className="side-nav">
          <nav className="nav-primary" aria-label={t('mainNavigation')}>
            {nav.map(({ page, label, icon }) => <button key={page} className={`nav-item ${activePage === page ? 'active' : ''}`} onClick={() => { setActivePage(page); setQuery(''); }}><Icon name={icon} size={20} /><span>{label}</span></button>)}
          </nav>
          <div className="nav-bottom">
            <div>
              <button className="nav-item" type="button" onClick={() => setLogsOpen(true)}><Icon name="logs" size={20} /><span>{t('logs')}</span></button>
              <button className="nav-item" onClick={openManual} disabled={busy}><Icon name="info" size={20} /><span>{t('manual')}</span><Icon name="external" size={12} /></button>
              <button className="nav-item" onClick={openSettings}><Icon name="settings" size={20} /><span>{t('settings')}</span></button>
            </div>
            <div className="app-meta"><span>Ver 0.1.0</span><p>© 2026 omnooth · {t('boothNotice')}</p></div>
          </div>
        </aside>
        <main className="page-main">
          <div className="page-scroll">
            {isLoading ? <LoadingState label={t('loading')} /> : activePage === 'projects' ? <ProjectsPage projects={orderedProjects} allCount={snapshot?.projects.length ?? 0} selectedId={selectedProjectId} selectedProject={selectedProject} sort={projectSort} onSort={setProjectSort} query={query} onQuery={setQuery} onLink={linkProject} onSelect={(project) => { setSelectedProjectId(project.id); toast.info(t('projectSelected', { name: project.name })); }} busy={busy} t={t} /> : <LibraryPage items={filteredItems} selectedItem={selectedItem} selectedVersion={selectedVersion} query={query} onQuery={setQuery} onAdd={addArchives} onSelect={(item) => { setSelectedItemId(item.id); setSelectedVersionId(latestVersion(item.versions)?.id ?? null); setChosenUnityPackages([]); }} onVersion={(id) => { setSelectedVersionId(id); setChosenUnityPackages([]); }} onExtract={extractSelected} archivePreview={currentPreview} onRetryPreview={() => setArchivePreviewAttempt((value) => value + 1)} busy={busy} selectedUnityPackages={visibleSelection} onTogglePackage={togglePackage} onMovePackage={movePackage} project={selectedProject} t={t} />}
          </div>
          {activePage === 'projects' ? <ProjectGuide t={t} /> : <PackageDetails item={selectedItem} version={selectedVersion} selectedCount={visibleSelection.length} project={selectedProject} busy={busy} onAdd={addArchives} onSettings={openSettings} t={t} />}
        </main>
      </div>
      {logsOpen && <LogDialog getLogs={client.getLogs} onClose={() => setLogsOpen(false)} t={t} />}
      {settingsOpen && settingsDraft && <SettingsDialog draft={settingsDraft} setDraft={setSettingsDraft} versions={snapshot?.items.flatMap((item) => item.versions.map((version) => ({ itemTitle: item.title, version })) ) ?? []} tab={settingsTab} setTab={setSettingsTab} busy={busy} deepLinkStatus={deepLinkStatus} deepLinkError={deepLinkError} onRegisterDeepLink={registerDeepLink} onClose={() => setSettingsOpen(false)} onSave={saveSettings} onChoosePath={chooseLibraryPath} t={t} />}
      <AppToaster container={logsOpen ? '.log-dialog' : settingsOpen ? '.settings-dialog' : null} />
    </div>
  );

  function togglePackage(id: string) {
    setChosenUnityPackages((previous) => previous.includes(id) ? previous.filter((value) => value !== id) : [...previous, id]);
  }
  function movePackage(id: string, direction: -1 | 1) {
    setChosenUnityPackages((previous) => {
      const index = previous.indexOf(id); const nextIndex = index + direction;
      if (index < 0 || nextIndex < 0 || nextIndex >= previous.length) return previous;
      const reordered = [...previous]; [reordered[index], reordered[nextIndex]] = [reordered[nextIndex], reordered[index]]; return reordered;
    });
  }
}

function Header({ project, query, onQuery, onSearch, onProjectClick, onAdd, busy, t }: { project: Project | null; query: string; onQuery: (value: string) => void; onSearch: () => void; onProjectClick: () => void; onAdd: () => void; busy: boolean; t: (key: string, options?: Record<string, unknown>) => string }) {
  return <header className="topbar">
    <div className="brand"><Icon name="menu" size={24} className="brand-mark" /><img className="brand-logo" src="/assets/1765a.svg" alt="omnooth" /></div>
    <button type="button" className="topbar-project" onClick={onProjectClick} aria-label={t('projectPicker', { project: project?.name ?? t('chooseProject') })}><Icon name="project" size={18} /><span title={project?.path}>{project?.name ?? t('chooseProject')}</span></button>
    <form className="top-search" role="search" onSubmit={(event) => { event.preventDefault(); onSearch(); }}><Icon name="search" size={18} /><input value={query} onChange={(event) => onQuery(event.target.value)} onFocus={onSearch} placeholder={t('searchAll')} aria-label={t('searchAll')} /></form>
    <div className="topbar-spacer" />
    <button className="button primary" onClick={onAdd} disabled={busy}><Icon name="download" size={16} />{busy ? t('working') : t('addFiles')}</button>
  </header>;
}

function ProjectsPage({ projects, allCount, selectedId, selectedProject, sort, onSort, query, onQuery, onLink, onSelect, busy, t }: { projects: Project[]; allCount: number; selectedId: string | null; selectedProject: Project | null; sort: 'lastUsed' | 'name'; onSort: (value: 'lastUsed' | 'name') => void; query: string; onQuery: (value: string) => void; onLink: () => void; onSelect: (project: Project) => void; busy: boolean; t: (key: string, options?: Record<string, unknown>) => string }) {
  return <section className="projects-page">
    <div className="page-heading"><div className="heading-title-row"><h1>{t('projects')}</h1><button className="button primary" onClick={onLink} disabled={busy}><Icon name="plus" size={16} />{t('linkProject')}</button></div><p>{t('projectsIntro')}</p></div>
    {!selectedId && <div className="notice-card"><Icon name="lock" size={18} /><div><strong>{t('noProjectSelected')}</strong><p>{t('noProjectSelectedHint')}</p></div></div>}
    <div className="toolbar"><SearchField value={query} onChange={onQuery} placeholder={t('searchProjects')} ariaLabel={t('searchProjects')} /><label className="sort-field"><Icon name="sort" size={16} /><select aria-label={t('sortProjectsLabel')} value={sort} onChange={(event) => onSort(event.target.value as 'lastUsed' | 'name')}><option value="lastUsed">{t('sortLastUsed')}</option><option value="name">{t('sortName')}</option></select></label></div>
    <div className="project-list-summary"><strong>{t('linkedProjectsSummary')}</strong><span>{t('projectListStatus', { count: allCount, state: selectedProject ? t('selectedSummaryState', { name: selectedProject.name }) : t('unselectedSummaryState') })}</span></div>
    {projects.length ? <div className="project-list">{projects.map((project) => <article className={`project-card ${selectedId === project.id ? 'chosen' : ''}`} key={project.id}>
      <div className="project-symbol"><Icon name="projectFolder" size={26} /></div><div className="project-info"><div className="project-title"><h2>{project.name}</h2>{selectedId === project.id && <span className="tag">{t('selected')}</span>}</div><p className="path mono" title={project.path}>{project.path}</p><div className="metadata"><span>{project.unityVersion || t('unityVersionUnknown')}</span>{project.lastUsedAt && <span>{t('lastUsed')}: {new Date(project.lastUsedAt).toLocaleString()}</span>}</div></div>
      <button className={`button ${selectedId === project.id ? 'secondary' : 'primary'}`} onClick={() => onSelect(project)} disabled={busy}>{selectedId === project.id ? t('selected') : t('select')}<Icon name="arrow" size={16} /></button>
    </article>)}</div> : <EmptyState title={allCount ? t('noProjectsMatch') : t('emptyProjectsTitle')} body={allCount ? t('noProjectsMatchHint') : t('emptyProjectsHint')} action={!allCount ? t('linkProject') : undefined} onAction={onLink} disabled={busy} />}
    <div className="footnote">{t('projectRequirements')}</div>
  </section>;
}

function LibraryPage({ items, selectedItem, selectedVersion, query, onQuery, onAdd, onSelect, onVersion, onExtract, archivePreview, onRetryPreview, busy, selectedUnityPackages, onTogglePackage, onMovePackage, project, t }: { items: LibraryItem[]; selectedItem: LibraryItem | null; selectedVersion: LibraryVersion | null; query: string; onQuery: (value: string) => void; onAdd: () => void; onSelect: (item: LibraryItem) => void; onVersion: (id: string) => void; onExtract: () => void; archivePreview: { status: 'loading' | 'ready' | 'error'; entries: ArchiveContents['entries'] } | null; onRetryPreview: () => void; busy: boolean; selectedUnityPackages: string[]; onTogglePackage: (id: string) => void; onMovePackage: (id: string, direction: -1 | 1) => void; project: Project | null; t: (key: string, options?: Record<string, unknown>) => string }) {
  const unityFiles = selectedVersion?.files.filter((file) => file.kind === 'unitypackage') ?? [];
  const selectedFiles = selectedUnityPackages.map((id) => unityFiles.find((file) => file.id === id)).filter((file): file is NonNullable<typeof file> => Boolean(file));
  return <section className="library-page">
    <div className="page-heading"><div className="heading-title-row"><h1>{t('library')}</h1><button className="button primary" onClick={onAdd} disabled={busy}><Icon name="plus" size={16} />{t('addFiles')}</button></div><p>{t('libraryIntro')}</p></div>
    <div className="notice-card compact"><Icon name="warning" size={18} /><div><strong>{t('libraryBehaviorTitle')}</strong><p>{t('libraryBehavior')}</p></div></div>
    <div className="toolbar"><SearchField value={query} onChange={onQuery} placeholder={t('searchLibrary')} ariaLabel={t('searchLibrary')} /><span className="subtle">{t('itemsCount', { count: items.length })}</span></div>
    {!items.length ? <EmptyState title={t('emptyLibraryTitle')} body={t('emptyLibraryHint')} action={t('addFiles')} onAction={onAdd} disabled={busy} /> : <div className="library-layout">
      <div className="item-list">{items.map((item) => <button key={item.id} className={`item-row ${selectedItem?.id === item.id ? 'selected' : ''}`} onClick={() => onSelect(item)} disabled={busy}><span className="file-badge">ZIP</span><span className="item-row-copy"><strong>{item.title}</strong><small>{item.versions.length} {t('versions')}</small></span><Icon name="chevron" size={12} /></button>)}</div>
      <div className="item-content">{selectedItem && selectedVersion ? <>
        <div className="item-titlebar"><div><h2>{selectedItem.title}</h2><p className="subtle mono">{selectedVersion.originalFilename}</p></div><label className="version-select-label"><span>{t('version')}</span><select value={selectedVersion.id} onChange={(event) => onVersion(event.target.value)} disabled={busy}>{selectedItem.versions.map((version) => <option key={version.id} value={version.id}>{version.label}</option>)}</select></label></div>
        <div className="version-summary"><span className={`status-dot ${selectedVersion.state}`} />{selectedVersion.state === 'extracted' ? t('extracted') : selectedVersion.state === 'ready' ? t('ready') : t('storedOnly')}<span className="mono">{selectedVersion.versionDate}</span><span className="mono">SHA-256 {selectedVersion.contentHash.slice(0, 8)}</span></div>
        <h3>{t('unityPackages')}</h3>
        {archivePreview && <div className="inline-empty"><strong>{t('archiveNotExtracted')}</strong><p>{t('zipBrowseHint')}</p><button className="button secondary" onClick={onExtract} disabled={busy}>{t('extractArchive')}</button></div>}
        {archivePreview?.status === 'loading' ? <p role="status">{t('archiveLoading')}</p> : archivePreview?.status === 'error' ? <div className="inline-empty" role="alert"><p>{t('archiveReadError')}</p><button className="button secondary" onClick={onRetryPreview} disabled={busy}>{t('archiveRetry')}</button></div> : unityFiles.length ? <div className="unity-file-list">{unityFiles.map((file) => <label className="unity-file" key={file.id}><input type="checkbox" checked={selectedUnityPackages.includes(file.id)} disabled={busy || file.encrypted} onChange={() => onTogglePackage(file.id)} /><span className="package-icon">UP</span><span className="unity-file-name mono" title={file.relativePath}>{file.relativePath}</span><span className="file-size">{file.encrypted ? t('archiveEncrypted') : formatBytes(file.sizeBytes)}</span></label>)}</div> : selectedVersion.state === 'stored' && !archivePreview ? <div className="inline-empty"><strong>{t('archiveNotExtracted')}</strong><p>{t('archiveNotExtractedHint')}</p><button className="button secondary" onClick={onExtract} disabled={busy}>{t('extractArchive')}</button></div> : <div className="inline-empty">{t('noUnityPackages')}</div>}
        {archivePreview?.status === 'ready' && <ArchiveContentsList key={selectedVersion.id} entries={archivePreview.entries} />}
        {selectedFiles.length > 0 && <div className="selection-box"><div className="selection-heading"><strong>{t('selectedImportOrder')}</strong><span>{t('selectedCount', { count: selectedFiles.length })}</span></div>{selectedFiles.map((file, index) => <div className="selection-item" key={file.id}><Icon name="grip" size={16} /><span className="order-number">{index + 1}</span><span className="mono">{file.relativePath.split('/').at(-1)}</span><span className="order-controls"><button onClick={() => onMovePackage(file.id, -1)} disabled={busy || index === 0} aria-label={t('moveUp')}>↑</button><button onClick={() => onMovePackage(file.id, 1)} disabled={busy || index === selectedFiles.length - 1} aria-label={t('moveDown')}>↓</button></span></div>)}<p>{t('selectionPending')}</p></div>}
      </> : <EmptyState title={t('selectLibraryItem')} body={t('selectLibraryItemHint')} />}</div>
    </div>}
    {project && <p className="context-line">{t('currentProject')}: <strong>{project.name}</strong></p>}
  </section>;
}

function PackageDetails({ item, version, selectedCount, project, busy, onAdd, onSettings, t }: { item: LibraryItem | null; version: LibraryVersion | null; selectedCount: number; project: Project | null; busy: boolean; onAdd: () => void; onSettings: () => void; t: (key: string, options?: Record<string, unknown>) => string }) {
  return <aside className="right-rail">
    <h2>{t('packageDetails')}</h2>
    {item && version ? <>
      <div className="rail-identity"><Icon name="disk" size={18} /><strong>{item.title}</strong></div><p className="rail-description">{t('originalFilename')}: <span className="mono">{version.originalFilename}</span></p>
      <hr /><div className="rail-label-row"><strong>{t('includedFiles')}</strong><span>{version.files.length}</span></div>
      {version.files.slice(0, 6).map((file) => <div className="rail-file" key={file.id}><span className="file-type">{file.relativePath.split('.').at(-1)?.toUpperCase() || 'FILE'}</span><div><span className="file-name mono">{file.relativePath}</span><small>{formatBytes(file.sizeBytes)} · {file.contentHash ? `SHA-256 ${file.contentHash.slice(0, 8)}` : t('hashOnImport')}</small></div></div>)}
      {version.files.length > 6 && <p className="subtle">{t('moreFiles', { count: version.files.length - 6 })}</p>}
      <hr /><div className="rail-label-row"><strong>{t('version')}</strong><span className="mono">{version.label}</span></div><p className="subtle mono">SHA-256 {version.contentHash}</p>
      <div className="rail-state"><strong>{version.state === 'extracted' ? t('extracted') : version.state === 'ready' ? t('ready') : t('storedOnly')}</strong><p>{version.state === 'ready' ? t('readyHint') : t('storedStatusHint')}</p></div>
      <hr /><h3>{t('unityImport')}</h3><p className="rail-description">{project ? t('selectionForProject', { project: project.name, count: selectedCount }) : t('selectProjectBeforeInstall')}</p><button className="button primary full" disabled>{t('applyUnavailable')}</button><p className="rail-caption">{t('applyPending')}</p>
    </> : <div className="rail-empty"><p>{t('emptyDetail')}</p><button className="text-button" onClick={onAdd} disabled={busy}>{t('addFiles')}</button></div>}
    <button className="rail-setting" onClick={onSettings}><Icon name="settings" size={16} />{t('librarySettings')}</button>
  </aside>;
}

function ProjectGuide({ t }: { t: (key: string, options?: Record<string, unknown>) => string }) {
  return <aside className="right-rail guide-rail"><h2><Icon name="link" size={20} />{t('projectGuide')}</h2><p>{t('projectGuideIntro')}</p><hr /><div className="guide-step"><strong>{t('guideStepOne')}</strong><p>{t('guideStepOneHint')}</p></div><div className="guide-step"><strong>{t('guideStepTwo')}</strong><p>{t('guideStepTwoHint')}</p></div><div className="guide-step"><strong>{t('guideStepThree')}</strong><p>{t('guideStepThreeHint')}</p></div><div className="guide-callout"><strong>{t('projectSafetyTitle')}</strong><p>{t('projectSafety')}</p></div></aside>;
}

function SettingsDialog({ draft, setDraft, versions, tab, setTab, busy, deepLinkStatus, deepLinkError, onRegisterDeepLink, onClose, onSave, onChoosePath, t }: { draft: AppSettings; setDraft: (value: AppSettings) => void; versions: Array<{ itemTitle: string; version: LibraryVersion }>; tab: SettingsTab; setTab: (value: SettingsTab) => void; busy: boolean; deepLinkStatus: DeepLinkStatus | null; deepLinkError: string; onRegisterDeepLink: () => void; onClose: () => void; onSave: () => void; onChoosePath: () => void; t: (key: string, options?: Record<string, unknown>) => string }) {
  const dialogRef = useRef<HTMLDialogElement>(null);
  const patch = <K extends keyof AppSettings>(key: K, value: AppSettings[K]) => setDraft({ ...draft, [key]: value });
  useEffect(() => {
    const dialog = dialogRef.current;
    if (dialog && !dialog.open) dialog.showModal();
    return () => { if (dialog?.open) dialog.close(); };
  }, []);
  return <dialog ref={dialogRef} className="settings-dialog" aria-labelledby="settings-title" onCancel={(event) => { event.preventDefault(); onClose(); }} onClick={(event) => { if (event.target === event.currentTarget) onClose(); }}>
    <header className="dialog-heading"><h2 id="settings-title">{t('settings')}</h2><button className="icon-button" onClick={onClose} aria-label={t('close')}><Icon name="close" size={20} /></button></header>
    <div className="settings-body"><nav className="settings-categories" role="tablist" aria-label={t('settingsCategories')}>
      <button className={`settings-category ${tab === 'general' ? 'active' : ''}`} type="button" role="tab" id="settings-tab-general" aria-selected={tab === 'general'} aria-controls="settings-panel" onClick={() => setTab('general')}><Icon name="settings" /><span>{t('general')}</span></button>
      <button className={`settings-category ${tab === 'history' ? 'active' : ''}`} type="button" role="tab" id="settings-tab-history" aria-selected={tab === 'history'} aria-controls="settings-panel" onClick={() => setTab('history')}><Icon name="history" /><span>{t('versionHistory')}</span></button>
    </nav>
      <div className="settings-content" role="tabpanel" id="settings-panel" aria-labelledby={tab === 'general' ? 'settings-tab-general' : 'settings-tab-history'}>{tab === 'history' ? <div className="settings-section"><h3>{t('versionHistory')}</h3><p>{t('versionHistoryHint')}</p>{versions.length ? <div className="history-list">{versions.map(({ itemTitle, version }) => <div className="history-row" key={version.id}><span><strong>{itemTitle}</strong><small className="history-label">{version.label} · {version.versionDate}</small></span><small className="mono">SHA-256 {version.contentHash.slice(0, 8)}</small></div>)}</div> : <div className="inline-empty">{t('emptyHistory')}</div>}</div> : <>
        <div className="settings-section"><h3>{t('general')}</h3><p>{t('generalHint')}</p><label className="field-label" htmlFor="language">{t('language')}</label><select id="language" value={draft.language} disabled={busy} onChange={(event) => patch('language', event.target.value as AppSettings['language'])}><option value="ja">日本語</option><option value="en">English</option><option value="ko">한국어</option></select></div>
        <div className="settings-divider" />
        <div className="settings-section"><h3>{t('deepLinkSection')}</h3><p>{t('deepLinkHint')}</p><div className="deep-link-status" role="status"><span className={`status-dot ${deepLinkStatus?.registered ? 'extracted' : ''}`} /><span>{deepLinkStatus?.registered ? t('deepLinkRegistered') : t('deepLinkNotRegistered')}</span></div>{deepLinkStatus?.lastReceipt && <p className="deep-link-receipt">{t('deepLinkLastReceived', { filename: deepLinkStatus.lastReceipt.filename })}</p>}<p className="deep-link-immediate">{t('deepLinkImmediate')}</p>{deepLinkError && <p className="deep-link-error" role="alert">{deepLinkError}</p>}<button className="button secondary" type="button" onClick={onRegisterDeepLink} disabled={busy}>{t('registerDeepLink')}</button><p className="settings-help">{t('deepLinkSwitchHint')}</p></div>
        <div className="settings-divider" />
        <div className="settings-section"><h3>{t('storage')}</h3><p>{t('storageHint')}</p><label className="field-label" htmlFor="library-path">{t('libraryPath')}</label><div className="path-input-row"><input id="library-path" className="mono" value={draft.libraryPath} readOnly /><button className="button secondary" onClick={onChoosePath} disabled={busy}>{t('chooseFolder')}</button></div></div>
        <div className="settings-divider" />
        <div className="settings-section"><h3>{t('import')}</h3><p>{t('importHint')}</p><label className="check-setting"><input type="checkbox" checked={draft.extractArchives} disabled={busy} onChange={(event) => patch('extractArchives', event.target.checked)} /><span><strong>{t('extractIncoming')}</strong><small>{t('extractIncomingHint')}</small></span></label><label className="check-setting"><input type="checkbox" checked={draft.keepOldVersions} disabled onChange={(event) => patch('keepOldVersions', event.target.checked)} /><span><strong>{t('keepVersions')}</strong><small>{t('keepVersionsHint')}</small></span></label></div>
      </>}</div>
    </div>
    <footer className="dialog-footer"><p>{t('settingsFooter')}</p><div><button className="button secondary" onClick={onClose}>{t('cancel')}</button><button className="button primary" onClick={onSave} disabled={busy}>{busy ? t('working') : t('save')}</button></div></footer>
  </dialog>;
}

function SearchField({ value, onChange, placeholder, ariaLabel }: { value: string; onChange: (value: string) => void; placeholder: string; ariaLabel: string }) {
  return <label className="search-field"><Icon name="search" size={16} /><input aria-label={ariaLabel} value={value} onChange={(event) => onChange(event.target.value)} placeholder={placeholder} /></label>;
}

function EmptyState({ title, body, action, onAction, disabled = false }: { title: string; body: string; action?: string; onAction?: () => void; disabled?: boolean }) {
  return <div className="empty-state"><div className="empty-symbol"><Icon name="folder" size={24} /></div><strong>{title}</strong><p>{body}</p>{action && onAction && <button className="button primary" onClick={onAction} disabled={disabled}><Icon name="plus" size={16} />{action}</button>}</div>;
}

function LoadingState({ label }: { label: string }) { return <div className="loading-state" role="status"><span className="spinner" />{label}</div>; }
function errorMessage(error: unknown, fallback: string) {
  const value = error instanceof Error ? error.message : typeof error === 'string' ? error : '';
  if (value === 'DESKTOP_REQUIRED') return i18n.t('desktopOnly');
  if (value === 'BUNDLE_REQUIRED') return i18n.t('bundleRequired');
  return value || fallback;
}
function deepLinkErrorMessage(error: unknown) {
  const value = error instanceof Error ? error.message : typeof error === 'string' ? error : '';
  if (value === 'DESKTOP_REQUIRED') return i18n.t('desktopOnly');
  if (value === 'BUNDLE_REQUIRED') return i18n.t('bundleRequired');
  return i18n.t('deepLinkError');
}
function localizedImportWarning(warning: string, t: (key: string) => string) {
  const key = warning === 'NO_UNITYPACKAGE_FOUND' ? 'warningNoUnitypackage' : warning === 'EXTRACTION_UNSUPPORTED_STORED_ONLY' ? 'warningExtractionUnsupported' : '';
  return key ? t(key) : warning;
}
function latestVersion(versions: LibraryVersion[]) {
  return versions.reduce<LibraryVersion | null>((latest, version) => !latest || version.versionDate > latest.versionDate ? version : latest, null);
}

export default App;
