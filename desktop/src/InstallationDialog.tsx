import { useEffect, useRef, useState } from 'react';
import { toast } from 'sonner';
import { client } from './client';
import { Icon } from './Icon';
import { formatBytes } from './formatBytes';
import type { Project, InstallSelection, InstallPlan, InstallationJob, InstallationState, RecoveryReport } from './contracts';

type Translate = (key: string, options?: Record<string, unknown>) => string;

export function InstallationDialog({ project, initialSelections, onClose, t }: {
  project: Project; initialSelections: InstallSelection[]; onClose: () => void; t: Translate;
}) {
  const dialog = useRef<HTMLDialogElement>(null);
  const [state, setState] = useState<InstallationState | null>(null);
  const [plan, setPlan] = useState<InstallPlan | null>(null);
  const [job, setJob] = useState<InstallationJob | null>(null);
  const [report, setReport] = useState<RecoveryReport | null>(null);
  const [busy, setBusy] = useState(false);
  const [error, setError] = useState('');
  const [notice, setNotice] = useState('');
  const idempotencyKey = useRef(crypto.randomUUID());
  const succeeded = useRef(new Set<string>());
  const activeJob = (next: InstallationState) => next.jobs.find((j) => ['preparing', 'waitingForUnity', 'recovering'].includes(j.status)) ?? null;

  useEffect(() => {
    dialog.current?.showModal();
    return () => dialog.current?.close();
  }, []);

  useEffect(() => {
    let disposed = false;
    setBusy(true);
    void (async () => {
      try {
        const next = await client.getInstallations(project.id);
        if (disposed) return;
        setState(next);
        const pending = activeJob(next);
        setJob(pending);
        if (!pending && initialSelections.length) {
          const result = await client.buildInstallPlan(project.id, initialSelections);
          if (!disposed) setPlan(result);
        }
      } catch (cause) { if (!disposed) setError(installError(cause, t)); }
      finally { if (!disposed) setBusy(false); }
    })();
    return () => { disposed = true; };
  }, [project.id, initialSelections, t]);

  useEffect(() => {
    if (job?.status !== 'waitingForUnity') return;
    let disposed = false;
    let timer: ReturnType<typeof setTimeout>;
    async function poll() {
      try {
        const next = await client.pollInstallation(job!.id);
        if (disposed) return;
        setJob(next);
        setError('');
        if (next.status === 'succeeded') {
          if (!succeeded.current.has(next.id)) { succeeded.current.add(next.id); toast.success(t('installSucceeded')); }
          setState(await client.getInstallations(project.id));
          setPlan(null);
        }
        if (next.status !== 'waitingForUnity') return;
      } catch (cause) { if (!disposed) setError(installError(cause, t)); }
      if (!disposed) timer = setTimeout(() => void poll(), 1500);
    }
    void poll();
    return () => { disposed = true; clearTimeout(timer); };
  }, [job?.id, job?.status, project.id, t]);

  async function perform(action: () => Promise<void>) {
    setBusy(true); setError(''); setNotice('');
    try { await action(); }
    catch (cause) { setError(installError(cause, t)); void client.reportError('unity.installation', String(cause)); }
    finally { setBusy(false); }
  }
  async function review(selections = initialSelections, remove: string[] = []) {
    await perform(async () => {
      setPlan(null); setReport(null); setJob(null);
      const next = await client.buildInstallPlan(project.id, selections, remove);
      idempotencyKey.current = crypto.randomUUID();
      setPlan(next);
    });
  }
  async function restore(mode: 'inspect' | 'restore') {
    if (!job) return;
    await perform(async () => {
      const result = await client.recoverInstallation(job.id, mode);
      setReport(result);
      if (mode === 'restore' && !result.requiresManualAction) {
        const next = await client.getInstallations(project.id); setState(next); setJob(activeJob(next)); setPlan(null);
        setNotice(t('installRestored'));
      }
    });
  }
  const pending = job && ['preparing', 'waitingForUnity', 'recovering'].includes(job.status);
  return <dialog ref={dialog} className="settings-dialog installation-dialog" aria-labelledby="installation-title" onCancel={(event) => { if (busy) event.preventDefault(); else onClose(); }}>
    <header className="dialog-heading"><h2 id="installation-title">{t('unityImport')}</h2><button className="icon-button" aria-label={t('close')} disabled={busy} onClick={onClose}><Icon name="close" /></button></header>
    <div className="installation-body">
      <p><strong>{project.name}</strong> · {project.unityVersion}</p><p className="mono path">{project.path}</p>
      <p>{t('installEditorHint')}</p>
      <button className="button secondary" disabled={busy || Boolean(pending)} onClick={() => void perform(async () => { await client.setupUnityAdapter(project.id); setNotice(t('installAdapterReady')); })}>{t('setupUnityAdapter')}</button>
      {busy && <p role="status">{t('working')}</p>}
      {error && <p className="deep-link-error" role="alert">{error}</p>}
      {notice && <p role="status">{notice}</p>}
      {job && <section className="installation-status" aria-live="polite"><strong>{t(`installStatus_${job.status}`)}</strong>{job.errorCode && <p>{installError(job.errorCode, t)}</p>}{job.status === 'waitingForUnity' && <p>{t('installWaitingHint')}</p>}</section>}
      {job && ['preparing', 'recovering'].includes(job.status) && <section>
        <p>{t('installRecoveryHint')}</p><div className="installation-actions"><button className="button secondary" disabled={busy} onClick={() => void restore('inspect')}>{t('inspectRecovery')}</button><button className="button secondary" disabled={busy} onClick={() => void restore('restore')}>{t('restoreInstallation')}</button></div>
        {report && <div role="status"><p>{report.requiresManualAction ? t('installRecoveryConflict') : t('installRecoverySafe')}</p>{report.conflicts.map((path) => <p className="mono" key={path}>{path}</p>)}</div>}
      </section>}
      {plan && !pending && job?.status !== 'succeeded' && <InstallPlanReview plan={plan} t={t} />}
      {!plan && !pending && initialSelections.length > 0 && <button className="button secondary" disabled={busy} onClick={() => void review()}>{t('reviewInstall')}</button>}
      {!pending && state && <section><h3>{t('installedPackages')}</h3>{state.packages.length ? state.packages.map((installed) => <div className="installed-package" key={installed.packageId}><div><strong>{installed.title}</strong><p className="mono">{installed.versionLabel}</p></div><button className="button secondary" disabled={busy} onClick={() => void review([], [installed.packageId])}>{t('reviewRemoval')}</button></div>) : <p>{t('noInstalledPackages')}</p>}</section>}
    </div>
    <footer className="dialog-footer"><p>{t('installFooter')}</p><div><button className="button secondary" disabled={busy} onClick={onClose}>{t('close')}</button>{plan && !pending && job?.status !== 'succeeded' && <button className="button primary" disabled={busy} onClick={() => void perform(async () => { const result = await client.installItem(plan, idempotencyKey.current); setJob(result); })}>{t('confirmInstall')}</button>}</div></footer>
  </dialog>;
}

function InstallPlanReview({ plan, t }: { plan: InstallPlan; t: Translate }) {
  const files = plan.assetManifest.filter((a) => a.contentHash !== null);
  return <section className="install-plan">
    <h3>{t('installPlanTitle')}</h3>
    {plan.operations.map((operation) => <p key={operation.packageId}><strong>{t(`installOperation_${operation.kind}`)}</strong> · {operation.title}</p>)}
    <p>{t('installPlanSummary', { count: files.length, removed: plan.removedPaths.length, bytes: formatBytes(plan.requiredBytes) })}</p>
    {plan.importOrder.length > 0 && <><h4>{t('selectedImportOrder')}</h4><ol>{plan.importOrder.map((path, index) => <li className="mono" key={`${index}:${path}`}>{path}</li>)}</ol></>}
    <details><summary>{t('installAffectedPaths')}</summary><ul className="install-paths">{files.slice(0, 200).map((file) => <li className="mono" key={file.relativePath}>{file.relativePath}</li>)}{plan.removedPaths.slice(0, 200).map((path) => <li className="mono" key={path}>{t('installOperation_remove')}: {path}</li>)}</ul>{files.length + plan.removedPaths.length > 200 && <p>{t('installPathsLimited')}</p>}</details>
  </section>;
}

function installError(cause: unknown, t: Translate): string {
  const code = String(cause).match(/\b[A-Z][A-Z_]{3,}\b/)?.[0] ?? 'UNKNOWN_ERROR';
  const groups: Record<string, string> = {
    UNITY_UNAVAILABLE: 'installUnityUnavailable', UNITY_BUSY: 'installUnityBusy', PROJECT_BUSY: 'installProjectBusy',
    PLAN_STALE: 'installPlanStale', NO_INSTALL_CHANGES: 'installNoChanges', USER_FILE_MODIFIED: 'installUserModified',
    ASSET_PATH_CONFLICT: 'installPathConflict', ASSET_GUID_CONFLICT: 'installGuidConflict', PROJECT_GUID_COLLISION: 'installGuidConflict',
    UNITY_ADAPTER_ALREADY_EXISTS: 'installAdapterExists', DEPENDENCY_SELECTION_REQUIRED: 'installDependencyRequired',
    PARENT_FOLDER_REQUIRED: 'installParentRequired', INSUFFICIENT_SPACE: 'installSpaceRequired',
  };
  const message = groups[code] ? t(groups[code]) : t('installError', { code });
  const path = String(cause).match(/Assets\/[^\r\n]+/)?.[0];
  return path ? `${message} ${path}` : message;
}
