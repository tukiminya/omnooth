import {test} from 'node:test';
import assert from 'node:assert/strict';
import {catalog} from '../data/catalog.mjs';
import {createDatabaseModel,databaseViews} from '../src/pages/database-model.ts';

test('ER 図は複数テーブルのモデルを分割し、設計中の外部キーをすべて実在する列につなぐ',()=>{
  const model=createDatabaseModel(catalog.specs);
  assert.equal(model.tables.length,17);
  assert.equal(model.relations.length,29);
  const tables=new Map(model.tables.map(t=>[t.name,t]));
  const fkCount=catalog.specs.filter(s=>s.category==='database').flatMap(s=>s.fields).filter(f=>f.constraint.includes('FK →')).length;
  assert.equal(model.relations.length,fkCount);
  for(const relation of model.relations) {
    assert.ok(tables.get(relation.parent)?.fields.some(f=>f.name===relation.parentField),relation.id);
    assert.ok(tables.get(relation.child)?.fields.some(f=>f.name===relation.childField&&f.foreign),relation.id);
  }
  assert.equal(tables.get('tags').specId,tables.get('item_tags').specId);
  assert.deepEqual(tables.get('item_tags').fields.filter(f=>f.primary).map(f=>f.name),['item_id','tag_id']);
});
test('収録ファイルの選択実績と、Unity 内に展開したファイルの出所を区別する',()=>{
  const {tables,relations}=createDatabaseModel(catalog.specs);
  const selections=tables.find(t=>t.name==='project_selections');
  assert.deepEqual(selections.fields.filter(f=>f.primary).map(f=>f.name),['project_id','file_id']);
  assert.equal(relations.find(r=>r.id==='project_selections.file_id').parent,'package_files');
  assert.equal(relations.find(r=>r.id==='project_selections.version_id').parent,'package_versions');
  assert.equal(relations.find(r=>r.id==='project_files.source_file_id').parent,'package_files');
});
test('NULL 可の参照・自己参照・依存の二つの役割を取り違えない',()=>{
  const {relations}=createDatabaseModel(catalog.specs);
  const relation=id=>relations.find(r=>r.id===id);
  assert.equal(relation('packages.item_id').optional,true);
  assert.equal(relation('items.shop_id').optional,false);
  assert.equal(relation('install_operations.from_version_id').optional,true);
  assert.equal(relation('install_operations.to_version_id').optional,true);
  assert.equal(relation('packages.display_parent_id').parent,'packages');
  assert.equal(relation('packages.display_parent_id').child,'packages');
  const dependencies=relations.filter(r=>r.child==='package_dependencies');
  assert.equal(dependencies.length,2);
  assert.ok(dependencies.every(r=>r.parent==='package_versions'&&!r.optional));
  assert.equal(new Set(dependencies.map(r=>r.label)).size,2);
});
test('ファイルのキューは任意の商品と複数の結果を参照し、Unity の適用ジョブから独立する',()=>{
  const {tables,relations}=createDatabaseModel(catalog.specs);
  const jobs=tables.find(t=>t.name==='library_import_jobs');
  const results=tables.find(t=>t.name==='library_import_results');
  assert.ok(jobs.fields.some(f=>f.name==='extract_archive'));
  assert.ok(jobs.fields.some(f=>f.name==='queue_seq'));
  assert.ok(!jobs.fields.some(f=>f.name==='project_id'));
  assert.equal(relations.find(r=>r.id==='library_import_jobs.item_id').optional,true);
  assert.deepEqual(results.fields.filter(f=>f.primary).map(f=>f.name),['job_id','version_id']);
  assert.equal(relations.find(r=>r.id==='library_import_results.job_id').parent,'library_import_jobs');
  assert.equal(relations.find(r=>r.id==='library_import_results.version_id').parent,'package_versions');
  const view=databaseViews.find(v=>v.id==='import');
  assert.ok(view.rows.flat().includes('library_import_jobs'));
  assert.ok(view.rows.flat().includes('library_import_results'));
});
test('分割ビューは全モデルを網羅し、全体図にすべてのテーブルを一度だけ配置する',()=>{
  const {tables}=createDatabaseModel(catalog.specs);
  const names=new Set(tables.map(t=>t.name));
  for(const view of databaseViews) {
    const shown=view.rows.flat().filter(Boolean);
    assert.equal(new Set(shown).size,shown.length,view.id);
    for(const name of shown)assert.ok(names.has(name),view.id+' / '+name);
  }
  const all=databaseViews.find(v=>v.id==='all').rows.flat().filter(Boolean);
  assert.deepEqual([...all].sort(),[...names].sort());
  const separated=new Set(databaseViews.filter(v=>v.id!=='all').flatMap(v=>v.rows.flat()).filter(Boolean));
  assert.deepEqual([...separated].sort(),[...names].sort());
});
test('適用台帳はパッケージ定義のパスを持ち、配置先変更のスキーマを持たない',()=>{
  const {tables}=createDatabaseModel(catalog.specs);
  assert.ok(!tables.some(t=>t.name==='project_package_paths'));
  const files=tables.find(t=>t.name==='project_files');
  assert.ok(files.fields.some(f=>f.name==='relative_path'&&f.primary));
  assert.ok(files.fields.some(f=>f.name==='asset_guid'));
  assert.ok(!files.fields.some(f=>f.name==='source_relative_path'));
  assert.ok(!tables.find(t=>t.name==='project_selections').fields.some(f=>f.name==='destination_path'));
});
