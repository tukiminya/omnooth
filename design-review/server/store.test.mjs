import { test } from 'node:test';
import assert from 'node:assert/strict';
import { mkdtempSync, rmSync, readFileSync } from 'node:fs';
import { tmpdir } from 'node:os';
import { join } from 'node:path';
import { createStore } from './store.mjs';
import { catalog } from '../data/catalog.mjs';

function fixture(t, inputCatalog=catalog) {
  const dir=mkdtempSync(join(tmpdir(),'omnooth-review-test-'));
  let store=createStore(dir,inputCatalog);
  t.after(()=>{store.close();rmSync(dir,{recursive:true,force:true});});
  return {
    dir, get store(){return store;},
    reopen(nextCatalog=inputCatalog){store.close();store=createStore(dir,nextCatalog);return store;},
    input(extra={}) {
      const spec=store.catalog.specs[0];
      return {targetId:spec.id,revision:spec.revision,author:'reviewer',body:'Assets がない時のエラー文言を確認したい',kind:'question',...extra};
    },
  };
}
test('コメント・返信・解決・承認を再起動後も保持し、人が読める JSON へ書き出す',t=>{
  const f=fixture(t);
  f.store.addComment(f.input());
  const root=f.store.snapshot().comments[0];
  f.store.addComment(f.input({parentId:root.id,body:'この項目の完了条件に反映します',author:'Codex',kind:'note'}));
  f.store.updateComment(root.id,{status:'resolved'});
  f.store.setDecision(root.targetId,{revision:f.input().revision,status:'approved'});
  f.reopen();
  const state=f.store.snapshot();
  assert.equal(state.comments.length,2);
  assert.ok(state.comments.every(c=>c.status==='resolved'));
  assert.equal(state.comments[1].parentId,root.id);
  assert.equal(state.decisions[0].status,'approved');
  const exported=JSON.parse(readFileSync(join(f.dir,'review-snapshot.json'),'utf8'));
  assert.equal(exported.comments[0].targetTitle,'既存 Unity プロジェクトを紐づける');
  assert.equal(exported.comments[1].author,'Codex');
  assert.equal(exported.decisions[0].status,'approved');
});
test('変更依頼は承認を取り消し、未解決の間は再承認を拒否する',t=>{
  const f=fixture(t),input=f.input({kind:'change'});
  f.store.setDecision(input.targetId,{revision:input.revision,status:'approved'});
  f.store.addComment(input);
  assert.equal(f.store.snapshot().decisions[0].status,'changes');
  assert.throws(()=>f.store.setDecision(input.targetId,{revision:input.revision,status:'approved'}),{status:409});
  const root=f.store.snapshot().comments[0];
  f.store.updateComment(root.id,{status:'resolved'});
  f.store.setDecision(input.targetId,{revision:input.revision,status:'approved'});
  f.store.updateComment(root.id,{status:'open'});
  assert.equal(f.store.snapshot().decisions[0].status,'changes');
});
test('解決したスレッドに返信したら親も再オープンし、変更依頼を見落とさない',t=>{
  const f=fixture(t);
  f.store.addComment(f.input());
  const root=f.store.snapshot().comments[0];
  f.store.updateComment(root.id,{status:'resolved'});
  f.store.addComment(f.input({parentId:root.id,kind:'change',body:'もう一つ修正をお願いします'}));
  assert.equal(f.store.snapshot().comments[0].status,'open');
  assert.equal(f.store.snapshot().decisions[0].status,'changes');
});
test('本文が変わった設計は再レビューとし、古い画面の承認・投稿を拒否する',t=>{
  const f=fixture(t),old=f.input();
  f.store.setDecision(old.targetId,{revision:old.revision,status:'approved'});
  const changed=structuredClone(catalog);
  changed.specs[0].summary+=' 追加の要件。';
  f.reopen(changed);
  assert.equal(f.store.snapshot().decisions[0].status,'unreviewed');
  assert.throws(()=>f.store.setDecision(old.targetId,{revision:old.revision,status:'approved'}),{status:409});
  assert.throws(()=>f.store.addComment(old),{status:409});
});
test('未知の項目・空白投稿・巨大本文・異なる項目への返信を拒否する',t=>{
  const f=fixture(t);
  assert.throws(()=>f.store.addComment(f.input({targetId:'not-a-spec'})),{status:404});
  assert.throws(()=>f.store.addComment(f.input({body:'  '})),{status:400});
  assert.throws(()=>f.store.addComment(f.input({body:'a'.repeat(10001)})),{status:400});
  assert.throws(()=>f.store.addComment(f.input({kind:'invalid'})),{status:400});
  f.store.addComment(f.input({body:"引用符 ' と <script> はデータとして保存"}));
  const root=f.store.snapshot().comments[0],other=f.store.catalog.specs[1];
  assert.throws(()=>f.store.addComment(f.input({targetId:other.id,revision:other.revision,parentId:root.id})),{status:400});
  assert.equal(f.store.snapshot().comments.length,1);
  assert.equal(f.store.snapshot().comments[0].body,"引用符 ' と <script> はデータとして保存");
});
test('設計の参照・関数の呼び出し先・シナリオが実在し、全 Figma コメントを追跡できる',()=>{
  const ids=new Set(catalog.specs.map(s=>s.id));
  assert.equal(ids.size,catalog.specs.length);
  for(const spec of catalog.specs) {
    for(const id of [...spec.related,...(spec.calls??[])]) assert.ok(ids.has(id),spec.id+' -> '+id);
    for(const id of spec.sources) assert.ok(catalog.sources.some(s=>s.id===id),spec.id+' -> '+id);
    assert.ok(spec.summary&&spec.acceptance.length,spec.id);
  }
  for(const flow of catalog.flows) for(const id of flow.steps) assert.ok(ids.has(id));
  for(let n=1;n<=12;n++) assert.ok(catalog.specs.some(s=>s.sources.includes('comment-'+n)),'Figma #'+n);
});
test('削除した設計は一覧から外し、コメント履歴と返信を維持して承認を禁止する',t=>{
  const f=fixture(t),old=f.input();
  f.store.addComment(old);const root=f.store.snapshot().comments[0];
  f.store.setDecision(old.targetId,{revision:old.revision,status:'approved'});
  const next=structuredClone(catalog),removed=next.specs.shift();
  next.archivedSpecs=[...(next.archivedSpecs??[]),{...removed,archived:true,title:'削除した項目',summary:'過去のレビューのみ保持',details:[],related:[],sources:[],acceptance:[]}];
  f.reopen(next);
  assert.ok(!f.store.catalog.specs.some(spec=>spec.id===old.targetId));
  const archived=f.store.catalog.archivedSpecs.find(spec=>spec.id===old.targetId);
  f.store.addComment({...old,revision:archived.revision,parentId:root.id,kind:'note',body:'設計を削除しました。'});
  f.store.updateComment(root.id,{status:'resolved'});
  const comments=f.store.snapshot().comments;
  assert.equal(comments.length,2);assert.equal(comments[0].body,root.body);
  assert.ok(comments.every(comment=>comment.status==='resolved'));
  assert.throws(()=>f.store.setDecision(old.targetId,{revision:archived.revision,status:'approved'}),{status:410});
  assert.throws(()=>f.store.addComment({...old,revision:archived.revision,kind:'change'}),{status:410});
  assert.equal(f.store.exportSnapshot().comments[0].targetTitle,'削除した項目');
});
test('現行設計と根拠に生成 AI の処理・サービス・設定を残さない',()=>{
  assert.doesNotMatch(JSON.stringify({specs:catalog.specs,sources:catalog.sources,flows:catalog.flows}),/\bAI\b|Haiku|Anthropic|Cloudflare|Hono|package-version-candidates/i);
});
test('直接追加とインポート先変更を現行の API・DB・フローへ再導入しない',()=>{
  const removed=['req-destination','db-path-mappings','fn-map-paths','fn-restore-paths','decision-path'];
  for(const id of removed){
    assert.ok(!catalog.specs.some(spec=>spec.id===id));
    assert.ok(catalog.archivedSpecs.some(spec=>spec.id===id&&spec.archived));
  }
  assert.ok(!catalog.flows.some(flow=>flow.id==='restore-paths'));
  const signatures=catalog.specs.map(spec=>spec.signature??'').join('\n');
  assert.doesNotMatch(signatures,/destination|originalPath|restorePaths|fileMappings|directFiles|MoveAsset/);
  assert.doesNotMatch(JSON.stringify(catalog.specs.flatMap(spec=>spec.fields??[])),/destination_path|destination_template|source_relative_path|relocate/);
});
