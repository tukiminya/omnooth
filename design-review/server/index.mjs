import { createServer } from 'node:http';
import { readFile, stat } from 'node:fs/promises';
import { resolve, dirname, extname, sep } from 'node:path';
import { fileURLToPath } from 'node:url';
import { catalog } from '../data/catalog.mjs';
import { createStore, HttpError } from './store.mjs';

const root = resolve(dirname(fileURLToPath(import.meta.url)), '..');
const isDev = process.argv.includes('--dev');
const port = Number(process.env.PORT ?? (isDev ? 4311 : 4310));
const store = createStore(process.env.REVIEW_DATA_DIR ?? resolve(root,'data'), catalog);
const allowedHosts = new Set(['127.0.0.1:'+port,'localhost:'+port,...(isDev ? ['127.0.0.1:4310','localhost:4310'] : [])]);
const allowedOrigins = new Set([...allowedHosts].map(host=>'http://'+host));
const mime = {'.html':'text/html; charset=utf-8','.js':'text/javascript; charset=utf-8','.css':'text/css; charset=utf-8','.png':'image/png','.svg':'image/svg+xml','.json':'application/json; charset=utf-8'};
const server = createServer(async (req,res)=>{
  const send = (status,body,headers={})=>{
    res.writeHead(status, {'Content-Type':'application/json; charset=utf-8','Cache-Control':'no-store','X-Content-Type-Options':'nosniff',...headers});
    res.end(typeof body==='string' ? body : JSON.stringify(body));
  };
  try {
    if (!allowedHosts.has(req.headers.host)) throw new HttpError(403,'このホストからは接続できません');
    if (req.headers.origin && !allowedOrigins.has(req.headers.origin)) throw new HttpError(403,'このオリジンからは接続できません');
    const url = new URL(req.url,'http://127.0.0.1:'+port);
    if (url.pathname.startsWith('/api/')) {
      if (req.method === 'GET') {
        if (url.pathname === '/api/health') return send(200,{ok:true});
        if (url.pathname === '/api/catalog') return send(200,store.catalog);
        if (url.pathname === '/api/reviews') return send(200,store.snapshot());
        if (url.pathname === '/api/export') {
          return send(200,store.exportSnapshot(),{'Content-Disposition':'attachment; filename="omnooth-review.json"'});
        }
        throw new HttpError(404,'API が見つかりません');
      }
      if (!['POST','PATCH'].includes(req.method)) throw new HttpError(405,'この操作は利用できません');
      if (!req.headers['content-type']?.startsWith('application/json')) throw new HttpError(415,'JSON で送信してください');
      let body = '';
      for await (const chunk of req) {
        body += chunk;
        if (Buffer.byteLength(body)>64000) throw new HttpError(413,'送信サイズが大きすぎます');
      }
      let input;
      try { input = JSON.parse(body); } catch { throw new HttpError(400,'JSON が不正です'); }
      if (!input || typeof input!=='object' || Array.isArray(input)) throw new HttpError(400,'入力が不正です');
      if (req.method==='POST' && url.pathname==='/api/comments') return send(201,store.addComment(input));
      const comment = /^\/api\/comments\/([^/]+)$/.exec(url.pathname);
      if (req.method==='PATCH' && comment) return send(200,store.updateComment(comment[1],input));
      const decision = /^\/api\/decisions\/([^/]+)$/.exec(url.pathname);
      if (req.method==='PATCH' && decision) return send(200,store.setDecision(decision[1],input));
      throw new HttpError(404,'API が見つかりません');
    }
    if (req.method !== 'GET' && req.method !== 'HEAD') throw new HttpError(405,'この操作は利用できません');
    const dist = resolve(root,'dist');
    let path = resolve(dist,'.'+decodeURIComponent(url.pathname));
    if (path!==dist && !path.startsWith(dist+sep)) throw new HttpError(403,'無効なパスです');
    if (path===dist) path=resolve(dist,'index.html');
    try { await stat(path); } catch {
      if (extname(path)) throw new HttpError(404,'ファイルが見つかりません');
      path=resolve(dist,'index.html');
    }
    const file = await readFile(path);
    res.writeHead(200,{'Content-Type':mime[extname(path)]??'application/octet-stream','X-Content-Type-Options':'nosniff','Cache-Control':'no-cache'});
    res.end(req.method==='HEAD' ? undefined : file);
  } catch (error) {
    if (!error.status) console.error(error);
    send(error.status??500,{error:error.status ? error.message : '保存に失敗しました。サーバーの状態を確認してください'});
  }
});
server.on('error',error=>{console.error('サーバー起動失敗:',error.message);process.exitCode=1;store.close();});
await new Promise((resolve,reject)=>{server.once('error',reject);server.listen(port,'127.0.0.1',resolve);});
console.log('omnooth design workspace: http://127.0.0.1:'+(isDev?4310:port));
let vite;
if (isDev) {
  const {createServer:createViteServer} = await import('vite');
  vite = await createViteServer({root});
  await vite.listen();
  vite.printUrls();
}
let closing = false;
async function stop() {
  if (closing) return;
  closing=true;
  await vite?.close();
  server.close(()=>{store.close();process.exit(0);});
}
process.on('SIGTERM',stop);
process.on('SIGINT',stop);
