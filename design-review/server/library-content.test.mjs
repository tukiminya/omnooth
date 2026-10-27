import {test} from 'node:test';
import assert from 'node:assert/strict';
import {getLibraryContents,buildContentSelection} from '../src/domain/library-content.ts';
const content={versionId:'version',filename:'Avatar_v1_01.zip',state:'ready',files:[
  {id:'pc',path:'Packages/Avatar_PC.unitypackage'},
  {id:'quest',path:'Packages/Avatar_Quest.UNITYPACKAGE'},
  {id:'outfit',path:'Extras/Outfit.unitypackage'},
  {id:'png',path:'Textures/Body.png'},{id:'psd',path:'Source/Body.psd'},
  {id:'meta',path:'Textures/Body.png.meta'},{id:'readme',path:'README.txt'},
]};
test('同じ原本の複数 unitypackage だけを、拡張子の大文字小文字によらず列挙する',()=>{
  assert.deepEqual(getLibraryContents(content).unitypackages.map(file=>file.id),['pc','quest','outfit']);
  assert.equal(getLibraryContents({...content,files:[content.files[0]]}).unitypackages.length,1);
});
test('指定順の unitypackage だけを返し、未選択の兄弟や直接コピーを追加しない',()=>{
  const plan=buildContentSelection(content,{projectId:'project',fileIds:['outfit','pc']});
  assert.deepEqual(plan.imports.map(file=>file.fileId),['outfit','pc']);
  assert.deepEqual(plan.imports.map(file=>file.order),[1,2]);
  assert.deepEqual(plan.issues,[]);
  assert.equal('copies' in plan,false);
  assert.equal('destination' in plan,false);
  assert.equal('fileMappings' in plan,false);
});
test('PNG / PSD・.meta・README は選択できず、unitypackage の無い原本は適用できない',()=>{
  for(const fileId of ['png','psd','meta','readme'])assert.throws(()=>buildContentSelection(content,{projectId:'project',fileIds:[fileId]}),/INVALID_FILE_SELECTION/);
  const unsupported={...content,files:content.files.slice(3)};
  assert.deepEqual(getLibraryContents(unsupported).unitypackages,[]);
  assert.ok(buildContentSelection(unsupported,{projectId:'project',fileIds:[]}).issues.some(issue=>issue.includes('含まれていません')));
});
test('未展開・未選択・プロジェクト不足・重複選択を拒否し、危険なマニフェストを通さない',()=>{
  assert.ok(buildContentSelection({...content,state:'stored',files:[]},{projectId:'project',fileIds:[]}).issues.some(issue=>issue.includes('展開')));
  assert.ok(buildContentSelection(content,{projectId:'project',fileIds:[]}).issues.length);
  assert.ok(buildContentSelection(content,{projectId:'',fileIds:['pc']}).issues.length);
  assert.throws(()=>buildContentSelection(content,{projectId:'project',fileIds:['pc','pc']}),/DUPLICATE_SELECTION/);
  for(const files of [
    [{id:'bad',path:'../Avatar.unitypackage'}],
    [{id:'same',path:'a.unitypackage'},{id:'same',path:'b.unitypackage'}],
    [{id:'a',path:'a.unitypackage'},{id:'b',path:'A.unitypackage'}],
  ])assert.throws(()=>getLibraryContents({...content,files}),/INVALID_MANIFEST/);
});
