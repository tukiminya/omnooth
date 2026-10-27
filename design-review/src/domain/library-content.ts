export type LibraryContent = {
  versionId:string;
  filename:string;
  state:'stored'|'ready';
  files:{id:string;path:string}[];
};

export function getLibraryContents(content:LibraryContent) {
  const paths=new Set<string>(),ids=new Set<string>();
  const files=content.files.map(file=>{
    if(!file.id||ids.has(file.id)||paths.has(file.path.toLowerCase())||!safeRelativePath(file.path))throw new Error('INVALID_MANIFEST');
    paths.add(file.path.toLowerCase());ids.add(file.id);
    const extension=file.path.split('.').at(-1)?.toLowerCase();
    return {...file,isUnitypackage:extension==='unitypackage'};
  });
  return {unitypackages:files.filter(file=>file.isUnitypackage).map(({isUnitypackage,...file})=>file)};
}

function safeRelativePath(path:string) {
  return !!path&&!/[\\:\u0000-\u001f]/.test(path)&&path.split('/').every(part=>!!part&&part!=='.'&&part!=='..');
}

export function buildContentSelection(content:LibraryContent,input:{projectId:string;fileIds:string[]}) {
  const groups=getLibraryContents(content);
  const selected=input.fileIds.map(id=>{
    const file=groups.unitypackages.find(f=>f.id===id);
    if(!file)throw new Error('INVALID_FILE_SELECTION');
    return file;
  });
  if(new Set(input.fileIds).size!==input.fileIds.length)throw new Error('DUPLICATE_SELECTION');
  const issues:string[]=[];
  if(content.state==='stored')issues.push('中身を展開してから選択してください。');
  if(!input.projectId)issues.push('適用先のプロジェクトを選択してください。');
  if(content.state==='ready'&&!groups.unitypackages.length)issues.push('適用できる .unitypackage が含まれていません。');
  else if(!selected.length)issues.push('適用する .unitypackage を選択してください。');
  const imports=selected.map((file,index)=>({fileId:file.id,path:file.path,order:index+1}));
  return {versionId:content.versionId,projectId:input.projectId,fileIds:[...input.fileIds],imports,issues};
}
