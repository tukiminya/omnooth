import { useEffect, useRef, useState } from 'react';
import { client } from './client';
import { formatBytes } from './formatBytes';
import type { CleanupPlan } from './contracts';
export function MaintenanceDialog({ onClose, onChanged, t }: { onClose: () => void; onChanged: () => Promise<void>; t: (key: string, options?: Record<string, unknown>) => string }) {
  const dialog=useRef<HTMLDialogElement>(null);
  const [plan,setPlan]=useState<CleanupPlan|null>(null);const [busy,setBusy]=useState(false);const [error,setError]=useState('');const [done,setDone]=useState(false);
  async function review(){setBusy(true);setError('');setDone(false);try{setPlan(await client.buildCleanupPlan());}catch{setError(t('cleanupUnavailable'));}finally{setBusy(false);}}
  useEffect(()=>{dialog.current?.showModal();void review();},[]);
  return <dialog ref={dialog} className="metadata-dialog" aria-label={t('cleanupTitle')} onCancel={event=>{event.preventDefault();if(!busy)onClose();}}><h2>{t('cleanupTitle')}</h2><p>{t('cleanupHint')}</p>{busy&&<p role="status">{t('working')}</p>}{error&&<p role="alert">{error}</p>}{done&&<p role="status">{t('cleanupDone')}</p>}{plan&&!done&&<><p>{t('cleanupSummary',{versions:plan.versions.length,staging:plan.stagingCount,backups:plan.backupCount,bytes:formatBytes(plan.bytes)})}</p><ul>{plan.versions.map(([id,title])=><li key={id}>{title}</li>)}</ul></>}<footer><button className="button secondary" disabled={busy} onClick={onClose}>{t('close')}</button><button className="button secondary" disabled={busy} onClick={()=>void review()}>{t('cleanupReview')}</button>{plan&&!done&&<button className="button primary" disabled={busy||plan.versions.length+plan.stagingCount+plan.backupCount===0} onClick={async()=>{setBusy(true);setError('');try{const report=await client.executeCleanup(plan.planHash);await onChanged();setDone(true);if(report.retainedPaths.length)setError(t('cleanupRetained'));}catch{setPlan(null);setError(t('cleanupStale'));}finally{setBusy(false);}}}>{t('cleanupConfirm')}</button>}</footer></dialog>;
}
