import {test} from 'node:test';
import assert from 'node:assert/strict';
import {readFileSync} from 'node:fs';
import {adaptBoothItemMetadata,findDownloadUpdatedAt} from '../src/domain/booth-metadata.ts';
const evidence=JSON.parse(readFileSync(new URL('./fixtures/booth-items.json',import.meta.url),'utf8'));
const adapt=payload=>adaptBoothItemMetadata(payload,{expectedItemId:String(payload.id),fetchedAt:evidence.checkedAt});
test('サンドボックス外で取得した2商品の実フィールドを内部 DTO に変換する',()=>{
  assert.deepEqual(evidence.requests.map(r=>r.status),[200,200]);
  for(const payload of evidence.items){
    const result=adapt(payload);
    assert.equal(result.boothItemId,String(payload.id));
    assert.equal(result.title,payload.name);
    assert.equal(result.shop.externalKey,String(payload.shop.id));
    assert.equal(result.ageRating,payload.is_adult?'r18':'all');
    assert.ok(result.images[0].originalUrl.startsWith('https://booth.pximg.net/'));
  }
  assert.equal(adapt(evidence.items[1]).variations[0].name,null);
});
test('公開日時ではなく、一致するバリエーションとファイルの更新日時を使う',()=>{
  const metadata=adapt(evidence.items[1]);
  assert.equal(findDownloadUpdatedAt(metadata,{variationId:'14346065',filename:'Kirisame_ver1.01.zip'}),'2026-08-18T09:45:08.000Z');
  assert.equal(findDownloadUpdatedAt(metadata,{variationId:'unknown',filename:'Kirisame_ver1.01.zip'}),null);
  assert.equal(findDownloadUpdatedAt(metadata,{variationId:'14346065',filename:'Kirisame_ver9.99.zip'}),null);
  const file=metadata.variations[0].downloadables.find(f=>f.filename==='Kirisame_ver1.01.zip');
  metadata.variations[0].downloadables.push(file);
  assert.equal(findDownloadUpdatedAt(metadata,{variationId:'14346065',filename:file.filename}),null);
});
test('別商品の応答・不正な ID・外部画像や非 HTTPS URL を拒否する',()=>{
  assert.throws(()=>adaptBoothItemMetadata(evidence.items[0],{expectedItemId:'8596249',fetchedAt:evidence.checkedAt}),/INVALID_RESPONSE/);
  for(const mutate of [
    value=>value.id=Number.MAX_SAFE_INTEGER+1,
    value=>value.is_adult='false',
    value=>value.url='http://booth.pm/items/7328764',
    value=>value.shop.url='https://booth.pm.example.com/',
    value=>value.images[0].original='https://other.example/image.png',
  ]){
    const payload=structuredClone(evidence.items[0]);mutate(payload);
    assert.throws(()=>adapt(payload),/INVALID_RESPONSE/);
  }
});
