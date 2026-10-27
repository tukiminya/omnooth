export function identifyPackageVersion(input:{filename:string;contentHash:string;sourceUpdatedAt?:string;importedAt:string}) {
  if(!input.filename||/[\\/\u0000-\u001f]/.test(input.filename))throw new Error('INVALID_FILENAME');
  if(!/^[\da-f]{64}$/i.test(input.contentHash))throw new Error('INVALID_HASH');
  const imported=new Date(input.importedAt);
  if(!Number.isFinite(imported.getTime()))throw new Error('INVALID_IMPORTED_AT');
  const updated=input.sourceUpdatedAt?new Date(input.sourceUpdatedAt):null;
  const date=updated&&Number.isFinite(updated.getTime())?updated:imported;
  const versionDate=date.toISOString().slice(0,10),contentHash=input.contentHash.toLowerCase();
  const packageName=input.filename.replace(/(?:\.tar\.(?:gz|bz2|xz)|\.(?:zip|7z|rar|tar|tgz|tbz2|txz|unitypackage))$/i,'');
  return {packageName,versionDate,contentHash,label:versionDate+'@'+contentHash.slice(0,8),originalFilename:input.filename};
}
