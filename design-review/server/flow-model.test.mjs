import {test} from 'node:test';
import assert from 'node:assert/strict';
import {catalog} from '../data/catalog.mjs';
import {createFlowModel,architectureFlow} from '../src/pages/flow-model.ts';

test('全シナリオの関数を実在する仕様と結び、入口から矢印でたどれる',()=>{
  for(const flow of [...catalog.flows,architectureFlow]){
    const {nodes,edges}=createFlowModel(flow,catalog.specs);
    assert.equal(new Set(nodes.map(n=>n.id)).size,nodes.length,flow.id);
    for(const id of flow.steps)assert.ok(nodes.some(n=>n.specId===id),flow.id+' / '+id);
    const roots=nodes.filter(node=>!edges.some(edge=>edge.to===node.id));
    assert.ok(roots.length>0,flow.id+' has an entry');
    const reached=new Set(roots.map(node=>node.id));
    for(let i=0;i<nodes.length;i++)for(const edge of edges)if(reached.has(edge.from))reached.add(edge.to);
    assert.deepEqual([...reached].sort(),nodes.map(n=>n.id).sort(),flow.id);
  }
});
test('GUI と拡張は共通処理へ入り、公開商品 API へ接続する',()=>{
  const {nodes,edges}=createFlowModel(architectureFlow,catalog.specs);
  assert.equal(edges.find(e=>e.from==='gui').to,'shared');
  assert.equal(edges.find(e=>e.from==='stdio').to,'shared');
  assert.equal(nodes.find(n=>n.id==='shared').specId,'infra-application');
  assert.equal(nodes.find(n=>n.id==='booth').specId,'infra-booth');
  assert.ok(edges.some(e=>e.from==='shared'&&e.to==='booth'));
  assert.ok(!nodes.some(n=>n.id==='cloud'));
});
test('矢印は図の範囲に収まり、別の関数や条件の箱を横切らない',()=>{
  for(const flow of [...catalog.flows,architectureFlow]){
    const {nodes,edges,width,height}=createFlowModel(flow,catalog.specs);
    for(const edge of edges){
      for(const [x,y] of edge.points)assert.ok(x>=0&&y>=0&&x<=width&&y<=height,edge.id);
      for(let i=0;i<edge.points.length-1;i++){
        const [a,b]=[edge.points[i],edge.points[i+1]];
        assert.ok(a[0]===b[0]||a[1]===b[1],edge.id+' must be orthogonal');
        for(const n of nodes.filter(n=>n.id!==edge.from&&n.id!==edge.to)){
          const crossing=a[0]===b[0]
            ?a[0]>n.x&&a[0]<n.x+n.width&&Math.max(a[1],b[1])>n.y&&Math.min(a[1],b[1])<n.y+n.height
            :a[1]>n.y&&a[1]<n.y+n.height&&Math.max(a[0],b[0])>n.x&&Math.min(a[0],b[0])<n.x+n.width;
          assert.ok(!crossing,flow.id+' / '+edge.from+' → '+edge.to+' crosses '+n.id);
        }
      }
    }
  }
});
test('Deep Link の ZIP 設定分岐と URL・パスワードの再開経路を区別する',()=>{
  const model=createFlowModel(catalog.flows.find(f=>f.id==='deep-link-import'),catalog.specs);
  const edge=(from,to)=>model.edges.find(e=>e.from===from&&e.to===to);
  assert.equal(edge('extract','fn-inspect').label,'ON');
  assert.match(edge('extract','done').label,/OFF/);
  assert.ok(edge('awaiting-url','fn-enqueue-import').feedback);
  assert.ok(edge('fn-update-import','fn-inspect').feedback);
  assert.equal(edge('download-result','awaiting-url').label,'期限切れ・拒否');
});
